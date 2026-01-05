use std::collections::HashMap;
use std::convert::TryInto;
use std::sync::Arc;

use crate::pbcode::raft::{self as pb, Node};
use crate::types::{AppNodeId, AppTypeConfig};
use futures::Stream;
use futures::StreamExt;
use openraft::OptionalSend;
use openraft::base::BoxFuture;
use openraft::base::BoxStream;
use openraft::error::{NetworkError, RPCError, StreamingError, Unreachable};
use openraft::network::RPCOption;
use openraft::network::RaftNetworkFactory;
use openraft::network::v2::RaftNetworkV2;
use openraft::raft::StreamAppendError;
use openraft::raft::StreamAppendResult;
use openraft::raft::{
    AppendEntriesRequest, AppendEntriesResponse, SnapshotResponse, VoteRequest, VoteResponse,
};
use openraft::{AnyError, Snapshot};
use tokio::sync::RwLock;
use tonic::codegen::tokio_stream::wrappers::ReceiverStream;
use tonic::transport::Channel;

pub struct RlrNetworkFactory {
    nodes: Arc<RwLock<HashMap<AppNodeId, NetworkConnection>>>,
}

impl RlrNetworkFactory {}

impl RlrNetworkFactory {
    pub async fn new(
        nodes: &std::collections::HashMap<AppNodeId, pb::Node>,
        _check_peers: bool,
    ) -> Result<Self, anyhow::Error> {
        // create a channel for each node
        let mut node_conns = HashMap::new();
        for (id, node) in nodes.iter() {
            node_conns.insert(*id, NetworkConnection::new(node.clone()));
        }

        Ok(RlrNetworkFactory {
            nodes: Arc::new(RwLock::new(node_conns)),
        })
    }
}

/// Implementation of the RaftNetworkFactory trait for creating new network connections.
/// This factory creates gRPC client connections to other Raft nodes.
impl RaftNetworkFactory<AppTypeConfig> for RlrNetworkFactory {
    type Network = NetworkConnection;

    #[tracing::instrument(level = "debug", skip_all)]
    async fn new_client(&mut self, node_id: AppNodeId, node: &Node) -> Self::Network {
        // NetworkConnection::new(node.clone())
        if let Some(c) = self.nodes.read().await.get(&node_id) {
            return c.clone();
        }

        let new_connection = NetworkConnection::new(node.clone());
        self.nodes
            .write()
            .await
            .insert(node_id, new_connection.clone());
        new_connection
    }
}

/// Represents an active network connection to a remote Raft node.
/// Handles serialization and deserialization of Raft messages over gRPC.
#[derive(Clone)]
pub struct NetworkConnection {
    target_node: pb::Node,
}

impl NetworkConnection {
    /// Creates a new NetworkConnection with the provided gRPC client.
    pub fn new(target_node: pb::Node) -> Self {
        NetworkConnection { target_node }
    }

    /// Creates a gRPC channel to the target node.
    async fn create_channel(&self) -> Result<Channel, RPCError<AppTypeConfig>> {
        let server_addr = &self.target_node.rpc_addr;
        let channel = Channel::builder(format!("http://{}", server_addr).parse().unwrap())
            .connect()
            .await
            .map_err(|e| RPCError::Unreachable(Unreachable::<AppTypeConfig>::new(&e)))?;
        Ok(channel)
    }

    /// Convert pb::AppendEntriesResponse to StreamAppendResult.
    ///
    /// For `StreamAppend`, conflict is encoded as `conflict = true` plus a required `last_log_id`
    /// carrying the conflict log id.
    fn pb_to_stream_result(
        resp: pb::AppendEntriesRsp,
    ) -> Result<StreamAppendResult<AppTypeConfig>, RPCError<AppTypeConfig>> {
        if let Some(higher_vote) = resp.rejected_by {
            return Ok(Err(StreamAppendError::HigherVote(higher_vote)));
        }

        if resp.conflict {
            let conflict_log_id = resp.last_log_id.ok_or_else(|| {
                RPCError::Network(NetworkError::<AppTypeConfig>::new(&AnyError::error(
                    "Missing `last_log_id` in conflict stream-append response",
                )))
            })?;
            let log_id = conflict_log_id.try_into().map_err(|e| {
                RPCError::Network(NetworkError::<AppTypeConfig>::new(&AnyError::error(
                    format!("Invalid conflict `last_log_id`: {}", e),
                )))
            })?;
            return Ok(Err(StreamAppendError::Conflict(log_id)));
        }

        Ok(Ok(resp
            .last_log_id
            .map(|log_id| log_id.try_into())
            .transpose()
            .map_err(|e| {
                RPCError::Network(NetworkError::<AppTypeConfig>::new(&AnyError::error(
                    format!("Invalid `last_log_id`: {}", e),
                )))
            })?))
    }

    /// Sends snapshot data in chunks through the provided channel.
    async fn send_snapshot_chunks(
        tx: &tokio::sync::mpsc::Sender<pb::SnapshotReq>,
        snapshot_data: &[u8],
    ) -> Result<(), NetworkError<AppTypeConfig>> {
        let chunk_size = 1024 * 1024;
        for chunk in snapshot_data.chunks(chunk_size) {
            let request = pb::SnapshotReq {
                payload: Some(pb::snapshot_req::Payload::Chunk(chunk.to_vec())),
            };
            tx.send(request)
                .await
                .map_err(|e| NetworkError::<AppTypeConfig>::new(&e))?;
        }
        Ok(())
    }
}

/// Implementation of RaftNetwork trait for handling Raft protocol communications.
#[allow(clippy::blocks_in_conditions)]
impl RaftNetworkV2<AppTypeConfig> for NetworkConnection {
    async fn append_entries(
        &mut self,
        req: AppendEntriesRequest<AppTypeConfig>,
        _option: RPCOption,
    ) -> Result<AppendEntriesResponse<AppTypeConfig>, RPCError<AppTypeConfig>> {
        let channel = self.create_channel().await?;
        let mut client = pb::raft_client::RaftClient::new(channel);

        let response = client
            .append_entries(tonic::Request::new(req.try_into().map_err(|e| {
                RPCError::Network(NetworkError::<AppTypeConfig>::new(&AnyError::error(
                    format!("Invalid AppendEntriesReq: {}", e),
                )))
            })?))
            .await
            .map_err(|e| RPCError::Network(NetworkError::<AppTypeConfig>::new(&e)))?;

        Ok(response.into_inner().try_into().map_err(|e| {
            RPCError::Network(NetworkError::<AppTypeConfig>::new(&AnyError::error(
                format!("Invalid AppendEntriesRsp: {}", e),
            )))
        })?)
    }

    fn stream_append<'s, S>(
        &'s mut self,
        input: S,
        _option: RPCOption,
    ) -> BoxFuture<
        's,
        Result<
            BoxStream<'s, Result<StreamAppendResult<AppTypeConfig>, RPCError<AppTypeConfig>>>,
            RPCError<AppTypeConfig>,
        >,
    >
    where
        S: Stream<Item = AppendEntriesRequest<AppTypeConfig>> + OptionalSend + Unpin + 'static,
    {
        let fu = async move {
            let channel = self.create_channel().await?;
            let mut client = pb::raft_client::RaftClient::new(channel);

            let response = client
                .stream_append(input.map(pb::AppendEntriesReq::from))
                .await
                .map_err(|e| RPCError::Network(NetworkError::<AppTypeConfig>::new(&e)))?;

            let output = response.into_inner().map(|result| {
                let resp = result
                    .map_err(|e| RPCError::Network(NetworkError::<AppTypeConfig>::new(&e)))?;
                Self::pb_to_stream_result(resp)
            });

            Ok(Box::pin(output) as BoxStream<'s, _>)
        };

        Box::pin(fu)
    }

    async fn full_snapshot(
        &mut self,
        vote: pb::Vote,
        snapshot: Snapshot<AppTypeConfig>,
        _cancel: impl std::future::Future<Output = openraft::error::ReplicationClosed>
        + OptionalSend
        + 'static,
        _option: RPCOption,
    ) -> Result<SnapshotResponse<AppTypeConfig>, StreamingError<AppTypeConfig>> {
        let channel = self.create_channel().await?;
        let mut client = pb::raft_client::RaftClient::new(channel);

        let (tx, rx) = tokio::sync::mpsc::channel(1024);
        let strm = ReceiverStream::new(rx);
        let response = client
            .snapshot(strm)
            .await
            .map_err(|e| NetworkError::<AppTypeConfig>::new(&e))?;

        // 1. Send meta chunk

        let meta = &snapshot.meta;

        let request = pb::SnapshotReq {
            payload: Some(pb::snapshot_req::Payload::Meta(pb::SnapshotReqMeta {
                vote: Some(vote.into()),
                last_log_id: meta.last_log_id.map(|log_id| log_id.into()),
                last_membership_log_id: meta.last_membership.log_id().map(|log_id| log_id.into()),
                last_membership: Some(meta.last_membership.membership().clone().into()),
                snapshot_id: meta.snapshot_id.to_string(),
            })),
        };

        tx.send(request)
            .await
            .map_err(|e| NetworkError::<AppTypeConfig>::new(&e))?;

        // 2. Send data chunks
        Self::send_snapshot_chunks(&tx, &snapshot.snapshot).await?;

        // 3. receive response

        let message = response.into_inner();

        Ok(SnapshotResponse {
            vote: message
                .vote
                .ok_or_else(|| {
                    NetworkError::<AppTypeConfig>::new(&AnyError::error(
                        "Missing `vote` in snapshot response",
                    ))
                })?
                .into(),
        })
    }

    async fn vote(
        &mut self,
        req: VoteRequest<AppTypeConfig>,
        _option: RPCOption,
    ) -> Result<VoteResponse<AppTypeConfig>, RPCError<AppTypeConfig>> {
        let channel = self.create_channel().await?;
        let mut client = pb::raft_client::RaftClient::new(channel);
        // Convert the openraft VoteRequest to protobuf VoteRequest
        let proto_vote_req: pb::VoteReq = req.try_into().map_err(|e| {
            RPCError::Network(NetworkError::<AppTypeConfig>::new(&AnyError::error(
                format!("Invalid VoteRequest: {}", e),
            )))
        })?;
        let request = tonic::Request::new(proto_vote_req);

        let response = client
            .vote(request)
            .await
            .map_err(|e| RPCError::Network(NetworkError::<AppTypeConfig>::new(&e)))?;

        // Convert the response back to openraft VoteResponse
        Ok(response.into_inner().try_into().map_err(|e| {
            RPCError::Network(NetworkError::<AppTypeConfig>::new(&AnyError::error(
                format!("Invalid VoteResponse: {}", e),
            )))
        })?)
    }
}

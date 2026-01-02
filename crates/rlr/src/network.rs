use crate::pbcode::raft as pb;
use crate::types::{AppNodeId, AppTypeConfig};
use openraft::BasicNode;
use openraft::OptionalSend;
use openraft::Vote;
use openraft::error::{
    NetworkError, RPCError, ReplicationClosed, StreamingError, Timeout, Unreachable,
};
use openraft::network::{RPCOption, RPCTypes, RaftNetworkFactory};
use openraft::raft::{
    AppendEntriesRequest, AppendEntriesResponse, SnapshotResponse, VoteRequest, VoteResponse,
};
use openraft::type_config::alias::{LogIdOf, VoteOf};
use openraft::vote::RaftLeaderId;
use std::collections::HashMap;
use std::future::Future;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::RwLock;
use tonic::transport::{Channel, Endpoint};

#[derive(Debug, Clone)]
pub struct RlrNetworkFactory {
    clients: Arc<RwLock<HashMap<AppNodeId, Channel>>>,
}

impl RlrNetworkFactory {
    // connect_now: true时立即连接所有节点, false时在第一次RPC调用时连接
    pub async fn new(
        nodes: &HashMap<AppNodeId, BasicNode>,
        connect_now: bool,
    ) -> Result<Self, anyhow::Error> {
        let mut clients = HashMap::with_capacity(nodes.len());

        for (id, node) in nodes.iter() {
            let addr = normalize_http_addr(&node.addr);
            let endpoint = Endpoint::from_shared(addr).expect("invalid node address");
            let channel = if connect_now {
                // 尝试ping最多30次，约耗时1分钟
                Some(
                    Self::build_channel_with_retry(
                        *id,
                        node,
                        &endpoint,
                        30,
                        Duration::from_millis(2000),
                    )
                    .await?,
                )
            } else {
                // connect_lazy
                Some(endpoint.connect_lazy())
            };
            clients.insert(*id, channel.unwrap());
        }

        Ok(Self {
            clients: Arc::new(RwLock::new(clients)),
        })
    }

    async fn build_channel_with_retry(
        id: AppNodeId,
        node: &BasicNode,
        endpoint: &Endpoint,
        retry: usize,
        sleep_dur: Duration,
    ) -> Result<Channel, anyhow::Error> {
        let mut channel = None;
        {
            for _ in 0..retry {
                match endpoint.connect().await {
                    Ok(c) => {
                        channel = Some(c);
                        break;
                    }
                    Err(e) => {
                        tracing::warn!(
                            "failed to connect to node {} at {}, retrying...: {}",
                            id,
                            node.addr,
                            e
                        );
                        tokio::time::sleep(sleep_dur).await;
                        continue;
                    }
                }
            }
        }
        if channel.is_some() {
            Ok(channel.unwrap())
        } else {
            Err(anyhow::anyhow!(
                "failed to connect to node {} at {} after retries",
                id,
                node.addr
            ))
        }
    }
}

impl RaftNetworkFactory<AppTypeConfig> for RlrNetworkFactory {
    type Network = RlrNetwork;

    async fn new_client(
        &mut self,
        target: AppNodeId,
        node: &openraft::impls::BasicNode,
    ) -> Self::Network {
        if let Some(channel) = self.clients.read().await.get(&target) {
            return RlrNetwork {
                target,
                channel: channel.clone(),
            };
        }

        // create a channel
        let addr = normalize_http_addr(&node.addr);
        let endpoint = Endpoint::from_shared(addr).expect("invalid node address");
        let mut clients = self.clients.write().await;
        clients.insert(target, endpoint.connect_lazy());
        let channel = clients.get(&target).unwrap().clone();
        drop(clients);

        RlrNetwork { target, channel }
    }
}

#[derive(Debug, Clone)]
pub struct RlrNetwork {
    target: AppNodeId,
    channel: Channel,
}

impl RlrNetwork {
    #[inline]
    fn client(&self) -> pb::raft_service_client::RaftServiceClient<Channel> {
        pb::raft_service_client::RaftServiceClient::new(self.channel.clone())
    }
}

impl openraft::network::v2::RaftNetworkV2<AppTypeConfig> for RlrNetwork {
    async fn append_entries(
        &mut self,
        rpc: AppendEntriesRequest<AppTypeConfig>,
        option: RPCOption,
    ) -> Result<AppendEntriesResponse<AppTypeConfig>, RPCError<AppTypeConfig>> {
        let ttl = option.hard_ttl();
        let leader_term = rpc.vote.leader_id.term();
        let leader_id = *rpc.vote.leader_id.node_id();

        let mut entries = Vec::with_capacity(rpc.entries.len());
        for ent in rpc.entries {
            let payload = serde_json::to_vec(&ent.payload)
                .map_err(|e| RPCError::Network(NetworkError::from_string(e.to_string())))?;
            entries.push(pb::LogEntry {
                log_id: Some(log_id_to_pb(&ent.log_id)),
                payload,
            });
        }

        let req = pb::AppendEntriesReq {
            term: leader_term,
            leader_id,
            prev_log_id: rpc.prev_log_id.as_ref().map(log_id_to_pb),
            entries,
            // protobuf uses an index; keep `0` as "none" sentinel.
            leader_commit: rpc.leader_commit.as_ref().map(|x| x.index).unwrap_or(0),
        };

        let mut client = self.client();
        let mut tonic_req = tonic::Request::new(req);
        tonic_req.set_timeout(ttl);

        let rsp = run_rpc_with_timeout(
            ttl,
            RPCTypes::AppendEntries,
            leader_id,
            self.target,
            async move {
                client
                    .append_entries(tonic_req)
                    .await
                    .map(|r| r.into_inner())
            },
        )
        .await?;

        // If the target reports a higher term, convert it to a higher vote.
        if rsp.term > leader_term {
            return Ok(AppendEntriesResponse::HigherVote(Vote::new_committed(
                rsp.term,
                self.target,
            )));
        }

        let Some(result) = rsp.result else {
            return Err(RPCError::Network(NetworkError::from_string(
                "AppendEntriesRsp missing result",
            )));
        };

        match result {
            pb::append_entries_rsp::Result::Success(ok) => {
                if ok {
                    Ok(AppendEntriesResponse::Success)
                } else {
                    Ok(AppendEntriesResponse::Conflict)
                }
            }
            pb::append_entries_rsp::Result::Conflict(conflict) => {
                // Treat the returned LogId as a partial success hint for faster backtracking.
                Ok(AppendEntriesResponse::PartialSuccess(Some(pb_to_log_id(
                    &conflict,
                ))))
            }
        }
    }

    async fn vote(
        &mut self,
        rpc: VoteRequest<AppTypeConfig>,
        option: RPCOption,
    ) -> Result<VoteResponse<AppTypeConfig>, RPCError<AppTypeConfig>> {
        let ttl = option.hard_ttl();
        let term = rpc.vote.leader_id.term();
        let candidate_id = *rpc.vote.leader_id.node_id();

        let req = pb::VoteReq {
            term,
            candidate_id,
            last_log_id: rpc.last_log_id.as_ref().map(log_id_to_pb),
        };

        let mut client = self.client();
        let mut tonic_req = tonic::Request::new(req);
        tonic_req.set_timeout(ttl);

        let rsp =
            run_rpc_with_timeout(ttl, RPCTypes::Vote, candidate_id, self.target, async move {
                client.vote(tonic_req).await.map(|r| r.into_inner())
            })
            .await?;

        // Keep response vote comparable with the request vote: reuse candidate_id.
        let vote = Vote::new(rsp.term, candidate_id);

        Ok(VoteResponse::new(
            &vote,
            rsp.last_log_id.as_ref().map(pb_to_log_id),
            rsp.vote_granted,
        ))
    }

    async fn full_snapshot(
        &mut self,
        vote: VoteOf<AppTypeConfig>,
        snapshot: openraft::Snapshot<AppTypeConfig>,
        cancel: impl Future<Output = ReplicationClosed> + OptionalSend + 'static,
        option: RPCOption,
    ) -> Result<SnapshotResponse<AppTypeConfig>, StreamingError<AppTypeConfig>> {
        let ttl = option.hard_ttl();
        let term = vote.leader_id.term();
        let leader_id = *vote.leader_id.node_id();

        let chunk_size = option.snapshot_chunk_size().unwrap_or(64 * 1024);

        // AppTypeConfig uses the default SnapshotData = Cursor<Vec<u8>>.
        let data = snapshot.snapshot.into_inner();
        let total_len = data.len();

        let last_log_id = snapshot.meta.last_log_id.as_ref().map(log_id_to_pb);

        let mut offset: usize = 0;
        let mut cancel = std::pin::pin!(cancel);

        while offset < total_len {
            let end = (offset + chunk_size).min(total_len);
            let done = end == total_len;
            let chunk = data[offset..end].to_vec();

            let req = pb::SendFullSnapshotReq {
                term,
                leader_id,
                last_log_id: last_log_id.clone(),
                offset: offset as u64,
                data: chunk,
                done,
            };

            let mut client = self.client();
            let mut tonic_req = tonic::Request::new(req);
            tonic_req.set_timeout(ttl);

            let send = async move {
                client
                    .send_full_snapshot(tonic_req)
                    .await
                    .map(|r| r.into_inner())
            };

            let rsp = tokio::select! {
                closed = &mut cancel => return Err(StreamingError::Closed(closed)),
                res = run_rpc_with_timeout(ttl, RPCTypes::InstallSnapshot, leader_id, self.target, send) => res.map_err(StreamingError::from)?,
            };

            // If follower reports a higher term, stop streaming and return it to OpenRaft.
            if rsp.term > term {
                return Ok(SnapshotResponse::new(Vote::new_committed(
                    rsp.term,
                    self.target,
                )));
            }

            offset = end;
        }

        Ok(SnapshotResponse::new(vote))
    }
}

#[inline]
fn normalize_http_addr(addr: &str) -> String {
    if addr.starts_with("http://") || addr.starts_with("https://") {
        addr.to_string()
    } else {
        let mut s = String::with_capacity("http://".len() + addr.len());
        s.push_str("http://");
        s.push_str(addr);
        s
    }
}

#[inline]
fn rpc_timeout(
    action: RPCTypes,
    id: AppNodeId,
    target: AppNodeId,
    timeout: Duration,
) -> RPCError<AppTypeConfig> {
    RPCError::Timeout(Timeout {
        action,
        id,
        target,
        timeout,
    })
}

#[inline]
fn map_status_to_rpc_error(
    status: tonic::Status,
    action: RPCTypes,
    id: AppNodeId,
    target: AppNodeId,
    timeout: Duration,
) -> RPCError<AppTypeConfig> {
    match status.code() {
        tonic::Code::DeadlineExceeded => rpc_timeout(action, id, target, timeout),
        tonic::Code::Unavailable => {
            RPCError::Unreachable(Unreachable::from_string(status.to_string()))
        }
        _ => RPCError::Network(NetworkError::from_string(status.to_string())),
    }
}

#[inline]
async fn run_rpc_with_timeout<T>(
    ttl: Duration,
    action: RPCTypes,
    id: AppNodeId,
    target: AppNodeId,
    fut: impl Future<Output = Result<T, tonic::Status>>,
) -> Result<T, RPCError<AppTypeConfig>> {
    match tokio::time::timeout(ttl, fut).await {
        Ok(Ok(v)) => Ok(v),
        Ok(Err(status)) => Err(map_status_to_rpc_error(status, action, id, target, ttl)),
        Err(_) => Err(rpc_timeout(action, id, target, ttl)),
    }
}

#[inline]
fn log_id_to_pb(log_id: &LogIdOf<AppTypeConfig>) -> pb::LogId {
    pb::LogId {
        term: log_id.leader_id.term,
        index: log_id.index,
    }
}

#[inline]
fn pb_to_log_id(pb: &pb::LogId) -> LogIdOf<AppTypeConfig> {
    use openraft::vote::leader_id_std::CommittedLeaderId;
    LogIdOf::<AppTypeConfig>::new(CommittedLeaderId::new(pb.term), pb.index)
}

use std::{
    collections::HashMap,
    convert::TryInto,
    pin::Pin,
    sync::{Arc, OnceLock},
};

use crate::{
    AppStateMachine, AppStateMachineHandler, network,
    storage::{self, RlrLogStore},
    types::{AppNodeId, AppTypeConfig},
};
use futures::{Stream, StreamExt};
use openraft::{Raft, Snapshot, SnapshotMeta, StoredMembership};
use rocksdb::{ColumnFamilyDescriptor, DB, Options};
use tokio::io;
use tonic::{Status, async_trait};
use tracing::{debug, error, instrument};

use crate::pbcode::raft as pb;

pub type Rlr = Raft<AppTypeConfig>;

// struct SnapshotSession {
//     last_log_id: Option<pb::LogId>,
//     buf: Vec<u8>,
// }

// #[inline]
// fn snapshot_sessions() -> &'static Mutex<HashMap<(u64, u64), SnapshotSession>> {
//     static SESSIONS: OnceLock<Mutex<HashMap<(u64, u64), SnapshotSession>>> = OnceLock::new();
//     SESSIONS.get_or_init(|| Mutex::new(HashMap::new()))
// }

pub struct RlrBuilder {
    raft_config: Option<openraft::Config>,
    check_peers: bool, // 启动时检查其余节点是否连通, 默认false
}
impl RlrBuilder {
    pub fn new() -> Self {
        Self {
            check_peers: true,
            raft_config: None,
        }
    }

    pub fn with_check_peers(mut self, check: bool) -> Self {
        self.check_peers = check;
        self
    }

    pub fn with_raft_config(mut self, config: openraft::Config) -> Self {
        self.raft_config = Some(config);
        self
    }

    // 此方法调用时立刻从LogStore加载最新状态机快照
    pub async fn build<S: AppStateMachine>(
        self,
        node_id: u64,
        db_path: &std::path::Path,
        snapshot_dir: &std::path::Path,
        nodes: &HashMap<AppNodeId, pb::Node>,
        state_machine: S,
    ) -> Result<Rlr, anyhow::Error> {
        let (raft_config, network, rlr_storage, app_statemachine) = self
            .build_components_only::<S>(db_path, snapshot_dir, nodes, state_machine)
            .await?;
        let raft =
            openraft::Raft::new(node_id, raft_config, network, rlr_storage, app_statemachine)
                .await?;

        Ok(raft)
    }

    pub async fn build_components_only<S: AppStateMachine>(
        self,
        db_dir: &std::path::Path,
        snapshot_dir: &std::path::Path,
        nodes: &HashMap<AppNodeId, pb::Node>,
        state_machine: S,
    ) -> Result<
        (
            Arc<openraft::Config>,
            network::callee::RlrNetworkFactory,
            RlrLogStore<AppTypeConfig>,
            AppStateMachineHandler<S>,
        ),
        anyhow::Error,
    > {
        // let snapshot_dir = db_path.to_path_buf().join("snapshots");
        if !snapshot_dir.exists() {
            tokio::fs::create_dir_all(&snapshot_dir).await?;
        }

        // todo: 可配置

        let raft_config = Arc::new(self.raft_config.unwrap());

        // 初始化存储
        let mut db_opts = Options::default();
        db_opts.create_missing_column_families(true);
        db_opts.create_if_missing(true);

        let cfs = vec![
            ColumnFamilyDescriptor::new(storage::CF_META, Options::default()),
            ColumnFamilyDescriptor::new(storage::CF_LOGS, Options::default()),
            ColumnFamilyDescriptor::new(storage::CF_PURGE_ID, Options::default()),
            ColumnFamilyDescriptor::new(storage::CF_VOTE, Options::default()),
        ];
        let db = DB::open_cf_descriptors(&db_opts, db_dir, cfs).map_err(io::Error::other)?;
        let db = Arc::new(db);
        let rlr_storage = RlrLogStore::new(db.clone());

        let app_statemachine: AppStateMachineHandler<S> =
            AppStateMachineHandler::new(snapshot_dir.to_path_buf(), state_machine);
        let network = network::callee::RlrNetworkFactory::new(nodes, false).await?;

        Ok((raft_config, network, rlr_storage, app_statemachine))
    }
}

#[async_trait]
impl pb::raft_server::Raft for Rlr {
    // 收到追加日志请求
    async fn append_entries(
        &self,
        request: tonic::Request<pb::AppendEntriesReq>,
    ) -> Result<tonic::Response<pb::AppendEntriesRsp>, tonic::Status> {
        let req = request
            .into_inner()
            .try_into()
            .map_err(|e| tonic::Status::internal(format!("Invalid AppendEntriesReq: {}", e)))?;
        let rsp = self.append_entries(req).await.map_err(|e| {
            tonic::Status::internal(format!("AppendEntries operation failed: {}", e))
        })?;

        Ok(tonic::Response::new(rsp.into()))
    }

    // 收到投票请求
    async fn vote(
        &self,
        request: tonic::Request<pb::VoteReq>,
    ) -> Result<tonic::Response<pb::VoteRsp>, tonic::Status> {
        debug!("Processing vote request");

        let vote_req = request
            .into_inner()
            .try_into()
            .map_err(|e| tonic::Status::internal(format!("Invalid VoteReq conversion: {}", e)))?;

        let vote_rsp = self
            .vote(vote_req)
            .await
            .map_err(|e| tonic::Status::internal(format!("Vote operation failed: {}", e)))?;

        debug!("Vote request processed successfully");
        Ok(tonic::Response::new(vote_rsp.try_into().map_err(|e| {
            tonic::Status::internal(format!("Failed to convert VoteRsp: {}", e))
        })?))
    }

    type StreamAppendStream =
        Pin<Box<dyn Stream<Item = Result<pb::AppendEntriesRsp, Status>> + Send>>;
    async fn stream_append(
        &self,
        request: tonic::Request<tonic::Streaming<pb::AppendEntriesReq>>,
    ) -> std::result::Result<tonic::Response<Self::StreamAppendStream>, tonic::Status> {
        debug!("Processing stream_append request");
        let input = request.into_inner();
        let input_stream =
            input.filter_map(|r| async move { r.ok().and_then(|req| req.try_into().ok()) });
        let output = self.stream_append(input_stream);
        let output_stream = output.map(|result| Ok(result.into()));
        Ok(tonic::Response::new(Box::pin(output_stream)))
    }
    /// Snapshot handles install snapshot RPC
    #[instrument(level = "info", skip_all)]
    async fn snapshot(
        &self,
        request: tonic::Request<tonic::Streaming<pb::SnapshotReq>>,
    ) -> std::result::Result<tonic::Response<pb::SnapshotRsp>, tonic::Status> {
        debug!("Processing streaming snapshot installation request");
        let mut stream = request.into_inner();

        // Get the first chunk which contains metadata
        let first_chunk = stream
            .next()
            .await
            .ok_or_else(|| Status::invalid_argument("Empty snapshot stream"))??;

        let vote;
        let snapshot_meta;
        {
            let meta = first_chunk.into_meta().ok_or_else(|| {
                error!("First snapshot chunk must be metadata");
                Status::invalid_argument("First snapshot chunk must be metadata")
            })?;

            debug!("Received snapshot metadata chunk: {:?}", meta);

            // todo: remove unwrap()
            vote = meta.vote.unwrap();
            snapshot_meta = SnapshotMeta {
                last_log_id: meta.last_log_id.map(|log_id| log_id.into()),
                last_membership: StoredMembership::new(
                    meta.last_membership_log_id.map(|x| x.into()),
                    meta.last_membership.unwrap().try_into().map_err(|e| {
                        Status::invalid_argument(format!(
                            "Invalid membership data in snapshot meta: {}",
                            e
                        ))
                    })?,
                ),
                snapshot_id: meta.snapshot_id,
            };
        }

        // Collect snapshot data
        let mut snapshot_data_bytes = Vec::new();

        while let Some(chunk) = stream.next().await {
            let data = chunk?
                .into_data_chunk()
                .ok_or_else(|| Status::invalid_argument("Snapshot chunk must be data"))?;
            snapshot_data_bytes.extend_from_slice(&data);
        }

        let snapshot = Snapshot {
            meta: snapshot_meta,
            snapshot: snapshot_data_bytes,
        };

        // Install the full snapshot
        let snapshot_resp = self
            .install_full_snapshot(vote, snapshot)
            .await
            .map_err(|e| Status::internal(format!("Snapshot installation failed: {}", e)))?;

        debug!("Streaming snapshot installation request processed successfully");
        Ok(tonic::Response::new(pb::SnapshotRsp {
            vote: Some(snapshot_resp.vote),
        }))
    }
}

#[inline]
fn log_id_to_pb(log_id: &openraft::type_config::alias::LogIdOf<AppTypeConfig>) -> pb::LogId {
    pb::LogId {
        term: log_id.leader_id,
        index: log_id.index,
    }
}

#[inline]
fn pb_to_log_id(pb: &pb::LogId) -> openraft::type_config::alias::LogIdOf<AppTypeConfig> {
    openraft::type_config::alias::LogIdOf::<AppTypeConfig>::new(pb.term, pb.index)
}

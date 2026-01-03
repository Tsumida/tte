use std::{collections::HashMap, sync::Arc};

use openraft::{BasicNode, Raft};
use rocksdb::{ColumnFamilyDescriptor, DB, Options};
use tokio::io;

use crate::{
    AppStateMachine, AppStateMachineHandler, network,
    storage::{self, RlrLogStore},
    types::{AppNodeId, AppTypeConfig},
};

pub type Rlr = Raft<AppTypeConfig>;

pub struct RlrBuilder {
    check_peers: bool, // 启动时检查其余节点是否连通, 默认false
}
impl RlrBuilder {
    pub fn new() -> Self {
        Self { check_peers: true }
    }

    pub fn with_check_peers(mut self, check: bool) -> Self {
        self.check_peers = check;
        self
    }

    // 此方法调用时立刻从LogStore加载最新状态机快照
    pub async fn build<S: AppStateMachine>(
        self,
        node_id: u64,
        db_path: &std::path::Path,
        snapshot_dir: &std::path::Path,
        nodes: &HashMap<AppNodeId, BasicNode>,
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
        &self,
        db_dir: &std::path::Path,
        snapshot_dir: &std::path::Path,
        nodes: &HashMap<AppNodeId, BasicNode>,
        state_machine: S,
    ) -> Result<
        (
            Arc<openraft::Config>,
            network::RlrNetworkFactory,
            RlrLogStore<AppTypeConfig>,
            AppStateMachineHandler<S>,
        ),
        anyhow::Error,
    > {
        // let snapshot_dir = db_path.to_path_buf().join("snapshots");
        if !snapshot_dir.exists() {
            tokio::fs::create_dir_all(&snapshot_dir).await?;
        }

        let raft_config = Arc::new(openraft::Config {
            heartbeat_interval: 500,
            election_timeout_min: 1500,
            election_timeout_max: 3000,
            max_payload_entries: 1024,
            snapshot_policy: openraft::SnapshotPolicy::LogsSinceLast(1000),
            ..Default::default()
        });

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
        let network = network::RlrNetworkFactory::new(nodes, false).await?;

        Ok((raft_config, network, rlr_storage, app_statemachine))
    }
}

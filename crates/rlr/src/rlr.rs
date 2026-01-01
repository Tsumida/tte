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

// 此方法调用时立刻从LogStore加载最新状态机快照
pub async fn new_rlr<S: AppStateMachine>(
    node_id: u64,
    db_path: &std::path::Path,
    nodes: &HashMap<AppNodeId, BasicNode>,
) -> Result<Rlr, anyhow::Error> {
    let snapshot_dir = db_path.to_path_buf().join("snapshots");
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
    let db = DB::open_cf_descriptors(&db_opts, db_path, cfs).map_err(io::Error::other)?;
    let db = Arc::new(db);
    let rlr_storage = RlrLogStore::new(db.clone());

    let app_statemachine: AppStateMachineHandler<S> =
        AppStateMachineHandler::new(snapshot_dir.clone());

    let network = network::RlrNetworkFactory::new(nodes).await?;
    let raft =
        openraft::Raft::new(node_id, raft_config, network, rlr_storage, app_statemachine).await?;

    Ok(raft)
}

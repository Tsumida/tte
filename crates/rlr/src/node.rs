use std::{collections::HashMap, io::Cursor, path::PathBuf, sync::Arc};

use futures::TryStreamExt;
use openraft::{
    Membership, RaftTypeConfig, Snapshot, alias::SnapshotDataOf, storage::RaftStateMachine,
};
use tokio::sync::RwLock;
use tracing::instrument;

use crate::{
    storage::AppSnapshotBuilder,
    types::{AppStateMachineInput, AppStateMachineOutput, AppTypeConfig},
};

// todo:
pub trait AppStateMachine {
    async fn apply(&mut self, req: AppStateMachineInput) -> anyhow::Result<AppStateMachineOutput>;

    // install_from_snapshot and then replay logs after snapshot.last_log_id
    async fn recover(&mut self, snapshot: SnapshotDataOf<AppTypeConfig>) -> anyhow::Result<()>;
}

struct RaftMetaData {
    last_applied_log_id: Option<openraft::alias::LogIdOf<AppTypeConfig>>,
    effective_membership: openraft::StoredMembership<AppTypeConfig>,
}

pub struct AppStateMachineHandler {
    // 更稳妥做法: 把Data和meta用同一个RwLock容器
    // 防止死锁，要求先拿meta, 后拿data
    raft_meta: Arc<RwLock<RaftMetaData>>,
    data: Arc<RwLock<HashMap<String, String>>>, // todo: Box<dyn AppStateMachine>
    current_snapshot: Option<Snapshot<AppTypeConfig>>,
    snapshot_dir: PathBuf, // 本地存放快照文件
}

impl AppStateMachineHandler {
    pub(crate) fn new(snapshot_dir: PathBuf) -> Self {
        Self {
            data: Arc::new(RwLock::new(HashMap::new())),
            raft_meta: Arc::new(RwLock::new(RaftMetaData {
                last_applied_log_id: None,
                effective_membership: openraft::StoredMembership::new(None, Membership::default()),
            })),
            current_snapshot: None,
            snapshot_dir,
        }
    }
}

impl RaftStateMachine<AppTypeConfig> for AppStateMachineHandler {
    type SnapshotBuilder = AppSnapshotBuilder<AppTypeConfig>;

    async fn applied_state(
        &mut self,
    ) -> Result<
        (
            Option<openraft::alias::LogIdOf<AppTypeConfig>>,
            openraft::StoredMembership<AppTypeConfig>,
        ),
        std::io::Error,
    > {
        let (last_log_id, membership);
        {
            let meta = self.raft_meta.read().await;
            last_log_id = meta.last_applied_log_id;
            membership = meta.effective_membership.clone();
        }
        Ok((last_log_id, membership))
    }

    #[instrument(level = "info", skip_all)]
    async fn apply<EntryStream>(&mut self, mut entries: EntryStream) -> Result<(), std::io::Error>
    where
        EntryStream: futures::Stream<
                Item = Result<openraft::storage::EntryResponder<AppTypeConfig>, std::io::Error>,
            > + Unpin
            + openraft::OptionalSend,
    {
        while let Some((entry, responder)) = entries.try_next().await? {
            // AppEntry -> StateMachine -> AppResponse, Openraft不关心
            let mut self_meta = self.raft_meta.write().await;
            if let Some(prev) = self_meta.last_applied_log_id {
                if prev >= entry.log_id {
                    tracing::warn!(
                        "skip applying log id {:?}, last_applied_log_id={:?}",
                        entry.log_id,
                        self_meta.last_applied_log_id
                    );
                    if let Some(r) = responder {
                        let _ = r.send(AppStateMachineOutput(b"skipped".to_vec()));
                    }
                    continue;
                }
            }
            self_meta.last_applied_log_id = Some(entry.log_id);
            // the entry is ok to apply
            let rsp = match &entry.payload {
                openraft::EntryPayload::Normal(data) => {
                    let s = String::from_utf8(data.0.clone()).unwrap();
                    let key = format!("{}", entry.log_id.index);
                    self.data.write().await.insert(key, s);
                    AppStateMachineOutput(b"kv updated".to_vec())
                }
                openraft::EntryPayload::Membership(m) => {
                    self_meta.effective_membership =
                        openraft::StoredMembership::new(Some(entry.log_id), m.clone());
                    AppStateMachineOutput(b"ok".to_vec())
                }
                // ignore blank
                openraft::EntryPayload::Blank => AppStateMachineOutput(Vec::new()),
            };

            if let Some(r) = responder {
                let _ = r.send(rsp);
            }
        }
        Ok(())
    }

    async fn get_snapshot_builder(&mut self) -> Self::SnapshotBuilder {
        let (data, membership, membership_log_id, last_applied_log_id) = {
            let raft_meta = self.raft_meta.read().await;
            let map = self.data.read().await;
            (
                serde_json::to_vec(&*map).unwrap(),
                raft_meta.effective_membership.membership().clone(),
                raft_meta.effective_membership.log_id().clone(), // 注意, 此数据是membership entry对应的log id，并非最新applied id
                raft_meta.last_applied_log_id,
            )
        };
        AppSnapshotBuilder::new(
            self.snapshot_dir.clone(),
            data,
            membership,
            membership_log_id,
            last_applied_log_id,
        )
    }

    async fn begin_receiving_snapshot(
        &mut self,
    ) -> Result<SnapshotDataOf<AppTypeConfig>, std::io::Error> {
        Ok(Cursor::new(Vec::new()))
    }

    async fn install_snapshot(
        &mut self,
        meta: &openraft::SnapshotMeta<AppTypeConfig>,
        snapshot: <AppTypeConfig as RaftTypeConfig>::SnapshotData,
    ) -> Result<(), std::io::Error> {
        self.data.write().await.clear();

        // 更新raft meta
        let mut raft_meta = self.raft_meta.write().await;
        raft_meta.last_applied_log_id = meta.last_log_id;
        raft_meta.effective_membership = meta.last_membership.clone();

        // 更新业务数据
        match serde_json::from_slice(snapshot.get_ref()) {
            Ok(m) => {
                self.data = Arc::new(RwLock::new(m));
            }
            Err(e) => {
                tracing::error!("failed to install snapshot: {}", e);
                return Err(std::io::Error::new(std::io::ErrorKind::InvalidData, e));
            }
        }

        // 更新快照
        self.current_snapshot = Some(Snapshot {
            meta: meta.clone(),
            snapshot,
        });

        Ok(())
    }

    async fn get_current_snapshot(
        &mut self,
    ) -> Result<Option<openraft::storage::Snapshot<AppTypeConfig>>, std::io::Error> {
        Ok(self.current_snapshot.clone())
    }
}

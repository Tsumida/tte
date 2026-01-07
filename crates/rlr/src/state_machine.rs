use std::{path::PathBuf, sync::Arc};

use futures::TryStreamExt;
use getset::Getters;
use openraft::{
    Membership, RaftTypeConfig, Snapshot, alias::SnapshotDataOf, entry::RaftEntry,
    storage::RaftStateMachine,
};
use tokio::sync::RwLock;
use tracing::instrument;

use crate::{
    pbcode::raft as pb,
    storage::AppSnapshotBuilder,
    types::{AppStateMachineOutput, AppTypeConfig},
};

// 业务状态机抽象
pub trait AppStateMachine: Send + Sync + 'static {
    type Input: for<'a> TryFrom<&'a [u8]> + Send + 'static;
    type Output: TryInto<Vec<u8>> + Send + 'static;

    // Atomic apply one entry, if failed, state machine keep unchanged.
    fn apply(&mut self, req: Self::Input) -> anyhow::Result<Self::Output>;
    // todo: replace with TryFrom<&[u8]> and TryInto<Vec<u8>>
    fn take_snapshot(&self) -> Vec<u8>;
    fn from_snapshot(data: &[u8]) -> Result<Self, anyhow::Error>
    where
        Self: Sized;
}

struct RaftMetaData {
    last_applied_log_id: Option<openraft::alias::LogIdOf<AppTypeConfig>>,
    effective_membership: openraft::StoredMembership<AppTypeConfig>,
}

pub struct AppStateMachineHandler<S: AppStateMachine> {
    snapshot_dir: PathBuf, // 本地存放快照文件
    // 更稳妥做法: 把Data和meta用同一个RwLock容器
    // 加锁顺序: meta -> data -> current_snapshot
    raft_meta: Arc<RwLock<RaftMetaData>>,
    data: Arc<RwLock<S>>,
    current_snapshot: Arc<RwLock<Option<Snapshot<AppTypeConfig>>>>,
}

impl<S: AppStateMachine> AppStateMachineHandler<S> {
    pub fn new(snapshot_dir: PathBuf, s: S) -> Self {
        Self {
            data: Arc::new(RwLock::new(s)),
            raft_meta: Arc::new(RwLock::new(RaftMetaData {
                last_applied_log_id: None,
                effective_membership: openraft::StoredMembership::new(None, Membership::default()),
            })),
            current_snapshot: Arc::new(RwLock::new(None)),
            snapshot_dir,
        }
    }

    pub async fn export_snapshot(&self) -> Vec<u8> {
        self.data.read().await.take_snapshot()
    }

    pub fn read_state(&self) -> Arc<RwLock<S>> {
        self.data.clone()
    }
}

impl<S: AppStateMachine> RaftStateMachine<AppTypeConfig> for AppStateMachineHandler<S> {
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
            let log_id = entry.log_id();
            let mut self_meta = self.raft_meta.write().await;
            if let Some(prev) = self_meta.last_applied_log_id {
                if prev >= log_id {
                    tracing::warn!(
                        "skip applying log id {:?}, last_applied_log_id={:?}",
                        log_id,
                        self_meta.last_applied_log_id
                    );
                    if let Some(r) = responder {
                        let _ = r.send(AppStateMachineOutput {
                            data: b"skipped".to_vec(),
                        });
                    }
                    continue;
                }
            }
            self_meta.last_applied_log_id = Some(log_id);
            // the entry is ok to apply
            let rsp = match &entry.entry_type {
                x if *x == pb::EntryType::Blank as u32 => AppStateMachineOutput {
                    data: b"ok".to_vec(),
                },
                x if *x == pb::EntryType::Normal as u32 => {
                    let input = S::Input::try_from(&entry.data[..]).map_err(|_| {
                        std::io::Error::new(
                            std::io::ErrorKind::InvalidData,
                            "failed to convert input",
                        )
                    })?;
                    let output = self.data.write().await.apply(input).map_err(|e| {
                        tracing::error!("failed to apply entry: {}", e);
                        std::io::Error::new(std::io::ErrorKind::Other, e)
                    })?;
                    let data = output.try_into().map_err(|_| {
                        std::io::Error::new(
                            std::io::ErrorKind::InvalidData,
                            "failed to convert output",
                        )
                    })?;
                    AppStateMachineOutput { data }
                }
                x if *x == pb::EntryType::Membership as u32 => {
                    let m: openraft::Membership<AppTypeConfig> =
                        entry.membership.unwrap().try_into().map_err(|_| {
                            std::io::Error::new(
                                std::io::ErrorKind::InvalidData,
                                "failed to convert membership",
                            )
                        })?;
                    self_meta.effective_membership =
                        openraft::StoredMembership::new(Some(log_id), m.clone());
                    AppStateMachineOutput {
                        data: b"ok".to_vec(),
                    }
                }
                _ => {
                    tracing::error!("unknown entry type: {}", entry.entry_type);
                    AppStateMachineOutput {
                        data: b"unknown entry type".to_vec(),
                    }
                }
            };

            if let Some(r) = responder {
                let _ = r.send(rsp);
            }
        }
        Ok(())
    }

    async fn get_snapshot_builder(&mut self) -> Self::SnapshotBuilder {
        let raft_meta = self.raft_meta.read().await;
        let inner = self.data.read().await;
        AppSnapshotBuilder::new(
            self.snapshot_dir.clone(),
            inner.take_snapshot(),
            raft_meta.effective_membership.membership().clone(),
            raft_meta.effective_membership.log_id().clone(), // 注意, 此数据是membership entry对应的log id，并非最新applied id
            raft_meta.last_applied_log_id,
            self.current_snapshot.clone(),
        )
    }

    async fn begin_receiving_snapshot(
        &mut self,
    ) -> Result<SnapshotDataOf<AppTypeConfig>, std::io::Error> {
        Ok(Vec::new())
    }

    async fn install_snapshot(
        &mut self,
        meta: &openraft::SnapshotMeta<AppTypeConfig>,
        snapshot: <AppTypeConfig as RaftTypeConfig>::SnapshotData,
    ) -> Result<(), std::io::Error> {
        // 更新raft meta
        let mut raft_meta = self.raft_meta.write().await;
        let mut data = self.data.write().await;
        let mut current_snapshot = self.current_snapshot.write().await;

        // 元数据
        raft_meta.last_applied_log_id = meta.last_log_id;
        raft_meta.effective_membership = meta.last_membership.clone();
        // 更新业务状态机
        *data = <S as AppStateMachine>::from_snapshot(&snapshot).map_err(|e| {
            std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("failed to install snapshot: {}", e),
            )
        })?;
        // 更新快照
        *current_snapshot = Some(Snapshot {
            meta: meta.clone(),
            snapshot,
        });

        Ok(())
    }

    async fn get_current_snapshot(
        &mut self,
    ) -> Result<Option<openraft::storage::Snapshot<AppTypeConfig>>, std::io::Error> {
        Ok(self.current_snapshot.read().await.clone())
    }
}

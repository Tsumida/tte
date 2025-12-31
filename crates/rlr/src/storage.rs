//! source from https://github.com/databendlabs/openraft/blob/main/examples/rocksstore/src/log_store.rs

use std::fmt::Debug;
use std::io;
use std::io::Cursor;
use std::marker::PhantomData;
use std::ops::RangeBounds;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;

use meta::StoreMeta;
use openraft::LogState;
use openraft::Membership;
use openraft::OptionalSend;
use openraft::RaftLogReader;
use openraft::RaftSnapshotBuilder;
use openraft::RaftTypeConfig;
use openraft::Snapshot;
use openraft::TokioRuntime;
use openraft::alias::EntryOf;
use openraft::alias::LogIdOf;
use openraft::alias::VoteOf;
use openraft::entry::RaftEntry;
use openraft::storage::IOFlushed;
use openraft::storage::RaftLogStorage;
use rocksdb::ColumnFamily;
use rocksdb::DB;
use rocksdb::Direction;
use serde_json;
use tokio::fs;
use tokio::sync::RwLock;
use tokio::task::spawn_blocking;
use tracing::instrument;

use crate::types::AppTypeConfig;
use crate::util::{bin2id, err_read_logs, id2bin};

pub(crate) const CF_META: &'static str = "meta";
pub(crate) const CF_LOGS: &'static str = "logs";
pub(crate) const CF_PURGE_ID: &'static str = "last_purged_log_id";
pub(crate) const CF_VOTE: &'static str = "vote";

#[derive(Debug, Clone)]
pub struct RlrLogStore<C>
where
    C: RaftTypeConfig,
{
    db: Arc<DB>,
    _p: PhantomData<C>,
}

impl<C> RlrLogStore<C>
where
    C: RaftTypeConfig,
{
    pub fn new(db: Arc<DB>) -> Self {
        db.cf_handle(CF_META)
            .expect("column family `meta` not found");
        db.cf_handle(CF_LOGS)
            .expect("column family `logs` not found");
        db.cf_handle(CF_PURGE_ID)
            .expect("column family `last_purged_log_id` not found");
        db.cf_handle(CF_VOTE)
            .expect("column family `vote` not found");

        Self {
            db,
            _p: Default::default(),
        }
    }

    fn cf_meta(&self) -> &ColumnFamily {
        self.db.cf_handle(CF_META).unwrap()
    }

    fn cf_logs(&self) -> &ColumnFamily {
        self.db.cf_handle(CF_LOGS).unwrap()
    }

    /// Get a store metadata.
    ///
    /// It returns `None` if the store does not have such a metadata stored.
    fn get_meta<M: StoreMeta<C>>(&self) -> Result<Option<M::Value>, io::Error> {
        let bytes = self
            .db
            .get_cf(self.cf_meta(), M::KEY)
            .map_err(|e| io::Error::other(e.to_string()))?;

        let Some(bytes) = bytes else {
            return Ok(None);
        };

        let t = serde_json::from_slice(&bytes)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;

        Ok(Some(t))
    }

    /// Save a store metadata.
    fn put_meta<M: StoreMeta<C>>(&self, value: &M::Value) -> Result<(), io::Error> {
        let json_value =
            serde_json::to_vec(value).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;

        self.db
            .put_cf(self.cf_meta(), M::KEY, json_value)
            .map_err(|e| io::Error::other(e.to_string()))?;

        Ok(())
    }
}

impl<C> RaftLogReader<C> for RlrLogStore<C>
where
    C: RaftTypeConfig,
{
    async fn try_get_log_entries<RB: RangeBounds<u64> + Clone + Debug + OptionalSend>(
        &mut self,
        range: RB,
    ) -> Result<Vec<C::Entry>, io::Error> {
        let start = match range.start_bound() {
            std::ops::Bound::Included(x) => id2bin(*x),
            std::ops::Bound::Excluded(x) => id2bin(*x + 1),
            std::ops::Bound::Unbounded => id2bin(0),
        };

        let mut res = Vec::new();

        let it = self.db.iterator_cf(
            self.cf_logs(),
            rocksdb::IteratorMode::From(&start, Direction::Forward),
        );
        for item_res in it {
            let (id, val) = item_res.map_err(err_read_logs)?;

            let id = bin2id(&id);
            if !range.contains(&id) {
                break;
            }

            let entry: EntryOf<C> = serde_json::from_slice(&val).map_err(err_read_logs)?;

            assert_eq!(id, entry.index());

            res.push(entry);
        }
        Ok(res)
    }

    async fn read_vote(&mut self) -> Result<Option<VoteOf<C>>, io::Error> {
        self.get_meta::<meta::Vote>()
    }
}

impl<C> RaftLogStorage<C> for RlrLogStore<C>
where
    C: RaftTypeConfig<AsyncRuntime = TokioRuntime>,
{
    type LogReader = Self;

    async fn get_log_state(&mut self) -> Result<LogState<C>, io::Error> {
        let last = self
            .db
            .iterator_cf(self.cf_logs(), rocksdb::IteratorMode::End)
            .next();

        let last_log_id = match last {
            None => None,
            Some(res) => {
                let (_log_index, entry_bytes) = res.map_err(err_read_logs)?;
                let ent =
                    serde_json::from_slice::<EntryOf<C>>(&entry_bytes).map_err(err_read_logs)?;
                Some(ent.log_id())
            }
        };

        let last_purged_log_id = self.get_meta::<meta::LastPurged>()?;

        let last_log_id = match last_log_id {
            None => last_purged_log_id.clone(),
            Some(x) => Some(x),
        };

        Ok(LogState {
            last_purged_log_id,
            last_log_id,
        })
    }

    async fn get_log_reader(&mut self) -> Self::LogReader {
        self.clone()
    }

    async fn save_vote(&mut self, vote: &VoteOf<C>) -> Result<(), io::Error> {
        self.put_meta::<meta::Vote>(vote)?;

        // Vote must be persisted to disk before returning.
        let db = self.db.clone();
        spawn_blocking(move || db.flush_wal(true))
            .await
            .map_err(|e| io::Error::other(e.to_string()))?
            .map_err(|e| io::Error::other(e.to_string()))?;

        Ok(())
    }

    // 将日志持久化到本地存储, key=log index, value=bytes
    async fn append<I>(&mut self, entries: I, callback: IOFlushed<C>) -> Result<(), io::Error>
    where
        I: IntoIterator<Item = EntryOf<C>> + Send,
    {
        for entry in entries {
            let id = id2bin(entry.index());
            self.db
                .put_cf(
                    self.cf_logs(),
                    id,
                    serde_json::to_vec(&entry)
                        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?,
                )
                .map_err(|e| io::Error::other(e.to_string()))?;
        }

        // spown_block()用于同步IO, 避免阻塞调度器
        let db = self.db.clone();
        let handle = spawn_blocking(move || {
            let res = db.flush_wal(true).map_err(io::Error::other);
            // io_completed有什么作用
            callback.io_completed(res);
        });
        drop(handle);

        // Return now, and the callback will be invoked later when IO is done.
        Ok(())
    }

    // 删除[log_id, +oo)的日志
    async fn truncate(&mut self, log_id: LogIdOf<C>) -> Result<(), io::Error> {
        tracing::debug!("truncate: [{:?}, +oo)", log_id);

        let from = id2bin(log_id.index());
        let to = id2bin(u64::MAX);
        self.db
            .delete_range_cf(self.cf_logs(), &from, &to)
            .map_err(|e| io::Error::other(e.to_string()))?;

        // Truncating does not need to be persisted.
        Ok(())
    }

    // 删除[0, log_id]的日志
    async fn purge(&mut self, log_id: LogIdOf<C>) -> Result<(), io::Error> {
        tracing::debug!("delete_log: [0, {:?}]", log_id);

        // Write the last-purged log id before purging the logs.
        // The logs at and before last-purged log id will be ignored by openraft.
        // Therefore, there is no need to do it in a transaction.
        self.put_meta::<meta::LastPurged>(&log_id)?;

        let from = id2bin(0);
        let to = id2bin(log_id.index() + 1);
        self.db
            .delete_range_cf(self.cf_logs(), &from, &to)
            .map_err(|e| io::Error::other(e.to_string()))?;

        // Purging does not need to be persistent.
        Ok(())
    }
}

// 管理本地磁盘的快照
pub(crate) trait SnapshotManagerAPI {
    async fn dump_snapshot(&mut self, snapshot: &Snapshot<AppTypeConfig>) -> Result<(), io::Error>;

    async fn get_latest_snapshot_path(&self) -> anyhow::Result<Option<PathBuf>>;

    async fn locate_latest_snapshot_and_install(
        &self,
    ) -> anyhow::Result<Option<Snapshot<AppTypeConfig>>>;
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct InnerSnapshot {
    last_applied_log_id: Option<openraft::alias::LogIdOf<AppTypeConfig>>,
    membership_log_id: Option<openraft::alias::LogIdOf<AppTypeConfig>>,
    // todo: crc32
    membership: Membership<AppTypeConfig>,
    data: Vec<u8>,
}

impl From<Snapshot<AppTypeConfig>> for InnerSnapshot {
    fn from(snap: Snapshot<AppTypeConfig>) -> Self {
        InnerSnapshot {
            data: snap.snapshot.get_ref().clone(),
            membership: snap.meta.last_membership.membership().clone(),
            membership_log_id: snap.meta.last_membership.log_id().clone(),
            last_applied_log_id: snap.meta.last_log_id,
        }
    }
}

impl Into<Snapshot<AppTypeConfig>> for InnerSnapshot {
    fn into(self) -> Snapshot<AppTypeConfig> {
        Snapshot {
            meta: openraft::SnapshotMeta {
                snapshot_id: format!("ss{}", uuid::Uuid::new_v4()),
                last_log_id: self.last_applied_log_id,
                last_membership: openraft::StoredMembership::new(
                    self.membership_log_id,
                    self.membership,
                ),
            },
            snapshot: Cursor::new(self.data),
        }
    }
}

// 快照管理器, 管理本地磁盘的快照文件
// 快照文件名格式 "snapshot_{name}_{log_id:014}_{ts:ms}", 是完全有序的
#[derive(Debug, Clone)]
pub struct DefaultSnapshotManager {
    name: &'static str,
    snapshot_dir: PathBuf,
}

impl DefaultSnapshotManager {
    pub fn new(name: &'static str, snapshot_dir: impl AsRef<Path>) -> Self {
        Self {
            name: if name != "" { name } else { "default" },
            snapshot_dir: snapshot_dir.as_ref().to_path_buf(),
        }
    }
}

impl SnapshotManagerAPI for DefaultSnapshotManager {
    #[instrument(level = "info", skip_all)]
    // SnapshotBuilder.build_snapshot()调用
    async fn dump_snapshot(&mut self, snapshot: &Snapshot<AppTypeConfig>) -> Result<(), io::Error> {
        let now_in_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis();

        let last_applied_log_id = snapshot.meta.last_log_id().map_or(0, |l| l.index());
        let snapshot_name = format!(
            "snapshot_{}_{:014}_{}",
            self.name, last_applied_log_id, now_in_ms
        );
        let snapshot_path = self.snapshot_dir.join(&snapshot_name);

        // create_dir and dump snapshot
        tokio::fs::create_dir_all(&self.snapshot_dir).await?;

        // todo: 高效序列化
        let buf = serde_json::to_vec(&InnerSnapshot::from(snapshot.clone()))
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;

        tokio::fs::write(&snapshot_path, buf).await?;
        tracing::info!(
            "dump_snapshot at: {}, last_applied_log_id: {:?}",
            snapshot_path.display(),
            last_applied_log_id
        );
        Ok(())
    }

    async fn get_latest_snapshot_path(&self) -> anyhow::Result<Option<PathBuf>> {
        let mut dir = fs::read_dir(&self.snapshot_dir).await?;
        let mut snapshots: Vec<PathBuf> = Vec::new();
        let prefix = format!("snapshot_{}_", self.name);

        while let Some(entry) = dir.next_entry().await? {
            let path = entry.path();

            let name = match path.file_name().and_then(|n| n.to_str()) {
                Some(n) => n,
                None => continue,
            };

            if name.starts_with(&prefix) {
                snapshots.push(path);
            }
        }

        snapshots.sort_by(|a, b| a.file_name().cmp(&b.file_name()));
        Ok(snapshots.pop())
    }

    // install_snapshot()中使用，从本地加载最新的快照更新状态机
    #[instrument(level = "info", skip_all)]
    async fn locate_latest_snapshot_and_install(
        &self,
    ) -> anyhow::Result<Option<Snapshot<AppTypeConfig>>> {
        if let Some(path) = self.get_latest_snapshot_path().await? {
            tracing::debug!("load snapshot from: {}", path.display());
            let data = tokio::fs::read(&path).await?;
            let inner_snapshot: InnerSnapshot = serde_json::from_slice(&data)
                .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
            Ok(Some(inner_snapshot.into()))
        } else {
            Ok(None)
        }
    }
}

pub struct AppSnapshotBuilder<C: RaftTypeConfig> {
    _phantom: std::marker::PhantomData<C>,
    membership: Membership<C>,
    membership_log_id: Option<openraft::alias::LogIdOf<C>>,
    last_applied_log_id: Option<openraft::alias::LogIdOf<C>>,
    data: Vec<u8>,
    current_snapshot: Arc<RwLock<Option<Snapshot<C>>>>,
    db_path: PathBuf,
}

impl AppSnapshotBuilder<AppTypeConfig> {
    pub(crate) fn new(
        snapshot_dir: PathBuf,
        data: Vec<u8>,
        membership: Membership<AppTypeConfig>,
        membership_log_id: Option<openraft::alias::LogIdOf<AppTypeConfig>>,
        last_applied_log_id: Option<openraft::alias::LogIdOf<AppTypeConfig>>,
        current_snapshot: Arc<RwLock<Option<Snapshot<AppTypeConfig>>>>,
    ) -> Self {
        Self {
            _phantom: std::marker::PhantomData,
            data,
            membership,
            membership_log_id,
            last_applied_log_id,
            current_snapshot,
            db_path: snapshot_dir,
        }
    }
}

impl RaftSnapshotBuilder<AppTypeConfig> for AppSnapshotBuilder<AppTypeConfig> {
    // 注意, build_snapshot()是异步调用的，会更新 current_snapshot变量
    async fn build_snapshot(&mut self) -> Result<Snapshot<AppTypeConfig>, std::io::Error> {
        let rand_snapshot_id = format!("ss{}", uuid::Uuid::new_v4());
        let snapshot = Snapshot {
            meta: openraft::SnapshotMeta {
                snapshot_id: rand_snapshot_id,
                last_log_id: self.last_applied_log_id,
                last_membership: openraft::StoredMembership::new(
                    self.membership_log_id,
                    self.membership.clone(),
                ),
            },
            snapshot: Cursor::new(self.data.clone()),
        };

        // 更新 current_snapshot
        {
            *self.current_snapshot.write().await = Some(snapshot.clone());
        }

        DefaultSnapshotManager::new("test", self.db_path.clone())
            .dump_snapshot(&snapshot)
            .await?;

        Ok(snapshot)
    }
}

mod meta {
    use openraft::RaftTypeConfig;
    use openraft::alias::LogIdOf;
    use openraft::alias::VoteOf;

    use crate::storage::{CF_PURGE_ID, CF_VOTE};

    /// Defines metadata key and value
    pub(crate) trait StoreMeta<C>
    where
        C: RaftTypeConfig,
    {
        /// The key used to store in rocksdb
        const KEY: &'static str;

        /// The type of the value to store
        type Value: serde::Serialize + serde::de::DeserializeOwned;
    }

    pub(crate) struct LastPurged {}
    pub(crate) struct Vote {}

    impl<C> StoreMeta<C> for LastPurged
    where
        C: RaftTypeConfig,
    {
        const KEY: &'static str = CF_PURGE_ID;
        type Value = LogIdOf<C>;
    }

    impl<C> StoreMeta<C> for Vote
    where
        C: RaftTypeConfig,
    {
        const KEY: &'static str = CF_VOTE;
        type Value = VoteOf<C>;
    }
}

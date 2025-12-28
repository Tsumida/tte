// use crate::state_machine::StateMachine;
// use crate::wal::WriteAheadLog;
// use bytes::Bytes;
use getset::Getters;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fmt::Debug;
use std::io::{self, Cursor};
use std::ops::RangeBounds;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use crate::types::{AppEntry, AppNodeId, AppResponse, AppTypeConfig};
use futures::{Stream, TryStreamExt};

use openraft::{
    Entry,
    EntryPayload,
    LogId,
    OptionalSend,
    RaftLogReader,
    RaftTypeConfig,
    StorageError,
    StoredMembership,
    // alias::SnapshotDataOf,
    storage::{
        EntryResponder, IOFlushed, LogState, RaftLogStorage, RaftSnapshotBuilder, RaftStateMachine,
        Snapshot, SnapshotMeta,
    },
};

#[derive(Debug, Getters)]
struct RlrStore<S: Send + Sync + 'static> {
    /// The Raft log.
    // pub log: sled::Tree, //RwLock<BTreeMap<u64, Entry<StorageRaftTypeConfig>>>,
    // vote: sled::Tree,

    /// The Raft state machine.
    #[getset(get = "pub")]
    state_machine: tokio::sync::RwLock<S>,

    #[getset(get = "pub")]
    node_id: AppNodeId,

    /// The current granted vote.
    current_snapshot: tokio::sync::RwLock<Option<RlrSnapshot>>,
    last_purged_log_id: Option<LogId<AppNodeId>>,
    config: AppTypeConfig,
}

impl<S: Send + Sync + 'static> RaftLogReader<AppTypeConfig> for RlrStore<S> {
    async fn try_get_log_entries<RB: RangeBounds<u64> + Clone + Debug + Send>(
        &mut self,
        range: RB,
    ) -> Result<Vec<Entry<AppTypeConfig>>, io::Error> {
        todo!()
    }

    async fn read_vote(&mut self) -> Result<Option<openraft::Vote<AppTypeConfig>>, io::Error> {
        todo!()
    }
}

impl<S: Send + Sync + 'static> RaftLogStorage<AppTypeConfig> for RlrStore<S> {
    type LogReader = Self;

    async fn get_log_state(&mut self) -> Result<LogState<AppTypeConfig>, io::Error> {
        todo!()
    }

    async fn save_committed(
        &mut self,
        committed: Option<LogId<AppTypeConfig>>,
    ) -> Result<(), io::Error> {
        todo!()
    }

    async fn read_committed(&mut self) -> Result<Option<LogId<AppTypeConfig>>, io::Error> {
        todo!()
    }

    async fn save_vote(&mut self, vote: &openraft::Vote<AppTypeConfig>) -> Result<(), io::Error> {
        todo!()
    }

    async fn append<I>(
        &mut self,
        entries: I,
        callback: IOFlushed<AppTypeConfig>,
    ) -> Result<(), io::Error>
    where
        I: IntoIterator<Item = Entry<AppTypeConfig>> + OptionalSend,
        I::IntoIter: OptionalSend,
    {
        todo!()
    }

    async fn truncate(&mut self, log_id: LogId<AppTypeConfig>) -> Result<(), io::Error> {
        todo!()
    }

    async fn purge(&mut self, log_id: LogId<AppTypeConfig>) -> Result<(), io::Error> {
        todo!()
    }

    async fn get_log_reader(&mut self) -> Self::LogReader {
        todo!()
    }
}

#[derive(Debug)]
struct RlrSnapshot {
    pub meta: SnapshotMeta<AppNodeId>,

    /// The data of the state machine at the time of this snapshot.
    pub data: Vec<u8>,
}

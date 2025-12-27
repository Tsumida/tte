#![forbid(unsafe_code)]

//! RLR: Reliable Log Replicator (sequencer).
//!
//! This crate defines a minimal, role-aware API for an external sequencer service
//! that provides strict ordering and a committed-log subscription stream.
//!
//! ## Scope
//! This crate currently provides:
//! - A small core API (`RlrApi`) designed around the constraints in `crates/rlr/AGENTS.md`.
//! - A lightweight in-process implementation (`StandaloneRlr`) for demos/dev.
//! - Pluggable storage via `LogStore`, with `MemLogStore` and an append-only WAL `FileLogStore`
//!   (feature: `file-store`, enabled by default for this crate).
//!
//! The OpenRaft and RocksDB dependencies are intentionally behind feature flags and are not
//! wired up yet:
//! - `openraft-rlr`: future OpenRaft-backed cluster implementation.
//! - `rocksdb-store`: future RocksDB-backed log/meta storage.
//!
//! Notes:
//! - `propose()` is leader-only and returns redirection info when called on a non-leader.
//! - `subscribe(from_index)` replays committed log entries starting at `from_index` (inclusive),
//!   then continues with live commits.
//! - The default build provides a small append-only file-backed store (feature: `file-store`).

use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::task::{Context, Poll};

use async_trait::async_trait;
use bytes::Bytes;
use futures::{Stream, StreamExt};
use tokio::sync::broadcast;

/// A node identifier inside an RLR cluster.
pub type NodeId = u64;

/// Raft term (monotonically increasing across leader elections).
pub type Term = u64;

/// Globally ordered committed log index, starting at 1.
///
/// This corresponds to the Raft paper's `log index` and is what downstream state machines
/// typically checkpoint on (e.g. "last applied index").
pub type GlobalIndex = u64;

/// A stable identifier for a committed entry.
///
/// This is intentionally close to Raft terminology: `(term, index)`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct LogId {
    pub term: Term,
    pub index: GlobalIndex,
}

/// Role of a node from the perspective of clients consuming committed logs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NodeRole {
    Leader,
    Follower,
    Learner,
}

/// Leader discovery info returned when a proposal is sent to a non-leader.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LeaderInfo {
    pub node_id: NodeId,
    pub advertise_addr: String,
}

/// Minimal cluster metadata snapshot.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClusterInfo {
    pub self_node_id: NodeId,
    pub role: NodeRole,
    pub leader: Option<LeaderInfo>,
}

/// A committed log entry delivered to subscribers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommittedEntry {
    pub log_id: LogId,
    pub payload: Bytes,
}

#[derive(Debug, thiserror::Error)]
pub enum RlrError {
    #[error("not leader")]
    NotLeader { leader: Option<LeaderInfo> },

    #[error("storage error: {0}")]
    Storage(#[from] std::io::Error),

    #[error("corrupt log: {0}")]
    CorruptLog(String),

    #[error("subscription lagged by {0} messages")]
    SubscriptionLagged(u64),
}

/// A stream of committed entries.
pub struct Subscription {
    inner: Pin<Box<dyn Stream<Item = Result<CommittedEntry, RlrError>> + Send + 'static>>,
}

impl Subscription {
    pub fn new<S>(stream: S) -> Self
    where
        S: Stream<Item = Result<CommittedEntry, RlrError>> + Send + 'static,
    {
        Self {
            inner: Box::pin(stream),
        }
    }
}

impl Stream for Subscription {
    type Item = Result<CommittedEntry, RlrError>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        self.inner.as_mut().poll_next(cx)
    }
}

/// Storage for committed log entries.
///
/// This is intentionally small: RLR is a log replicator, not a business state machine.
#[async_trait]
pub trait LogStore: Send + Sync + 'static {
    /// Returns the highest committed log index persisted in this store.
    async fn last_index(&self) -> Result<GlobalIndex, RlrError>;

    /// Appends a new committed entry payload and returns its assigned committed log index.
    ///
    /// Implementations must assign strictly increasing indices (1, 2, 3, ...).
    async fn append(&self, term: Term, payload: Bytes) -> Result<LogId, RlrError>;

    /// Appends a batch of payloads under the same term.
    ///
    /// Implementations should optimize this path (e.g. single lock + single flush) because
    /// it can materially improve throughput by amortizing per-append overhead.
    async fn append_batch(&self, term: Term, payloads: Vec<Bytes>) -> Result<Vec<LogId>, RlrError> {
        let mut out = Vec::with_capacity(payloads.len());
        for payload in payloads {
            out.push(self.append(term, payload).await?);
        }
        Ok(out)
    }

    /// Reads committed entries in the inclusive range `[from, to_inclusive]`.
    ///
    /// Semantics:
    /// - `from` is inclusive; passing `from=0` is treated as `from=1`.
    /// - Returning fewer entries than requested is allowed when data does not exist.
    /// - Entries must be returned in ascending `index` order.
    async fn read_range(
        &self,
        from: GlobalIndex,
        to_inclusive: GlobalIndex,
    ) -> Result<Vec<CommittedEntry>, RlrError>;
}

/// Minimal, role-aware RLR API.
///
/// Conceptually:
/// - **Ingress (writes)**: only the leader can accept `propose()`.
/// - **Egress (reads)**: any node can serve `subscribe()` for committed entries.
///   Business systems (OMS/ME) consume committed entries and apply them deterministically.
#[async_trait]
pub trait RlrApi: Send + Sync {
    /// Returns a point-in-time view of cluster metadata as observed by this node.
    fn cluster_info(&self) -> ClusterInfo;

    /// Propose a new command to be committed (leader-only).
    ///
    /// Errors:
    /// - `RlrError::NotLeader { leader }` if called on a follower/learner; `leader` may include
    ///   redirection info for the current leader.
    async fn propose(&self, command: Bytes) -> Result<LogId, RlrError>;

    /// Propose multiple commands to be committed (leader-only).
    ///
    /// This exists to reduce per-request overhead and allow the underlying storage to write
    /// multiple entries efficiently (e.g. one flush).
    ///
    /// Notes on Raft/OpenRaft:
    /// - Raft replication uses batched AppendEntries RPCs, but applications often still submit
    ///   proposals one-by-one.
    /// - Even if the Raft implementation coalesces work internally, a batch API can reduce
    ///   network round-trips and storage flushes at the service boundary.
    async fn propose_batch(&self, commands: Vec<Bytes>) -> Result<Vec<LogId>, RlrError> {
        let mut out = Vec::with_capacity(commands.len());
        for cmd in commands {
            out.push(self.propose(cmd).await?);
        }
        Ok(out)
    }

    /// Subscribe to committed log entries starting at `from_index` (inclusive).
    ///
    /// `from_index` semantics:
    /// - `from_index = 1` means "start from the first committed entry".
    /// - `from_index = N` means "replay entries with index >= N, then continue with new commits".
    /// - `from_index = 0` is treated as `1`.
    /// - If `from_index` is greater than the current last committed index, the subscription
    ///   yields only future commits.
    ///
    /// Delivery semantics (this crate's `StandaloneRlr` implementation):
    /// - The stream is **ordered** by `CommittedEntry.log_id.index` (monotonically increasing).
    /// - The stream is composed of two phases:
    ///   1) **Replay** from storage: `[from_index, upper]`, where `upper` is the store's
    ///      last committed index observed when `subscribe()` is called.
    ///   2) **Live** commits: entries with `index > upper` broadcast by this node.
    /// - If the consumer lags (broadcast buffer overflow), an
    ///   `Err(RlrError::SubscriptionLagged(_))` is yielded. Typical handling is to reconnect and
    ///   resubscribe from the last successfully applied index.
    async fn subscribe(&self, from_index: GlobalIndex) -> Result<Subscription, RlrError>;
}

/// Configuration for a standalone node.
#[derive(Debug, Clone)]
pub struct StandaloneRlrConfig {
    pub node_id: NodeId,
    /// Term to attach to newly committed entries in this standalone implementation.
    ///
    /// In a real Raft-backed implementation, term is managed by the consensus module and can
    /// change on leader elections. Here it's a fixed value suitable for demos/dev.
    pub term: Term,
    pub role: NodeRole,
    pub leader: Option<LeaderInfo>,
    /// Broadcast channel capacity for committed entries.
    pub commit_buffer: usize,
}

impl Default for StandaloneRlrConfig {
    fn default() -> Self {
        Self {
            node_id: 1,
            term: 1,
            role: NodeRole::Leader,
            leader: Some(LeaderInfo {
                node_id: 1,
                advertise_addr: "127.0.0.1:0".to_string(),
            }),
            commit_buffer: 1024,
        }
    }
}

/// An in-process implementation that enforces leader-only proposals and provides subscriptions.
///
/// This is a building block for future OpenRaft / RocksDB integration; the public API is stable.
#[derive(Debug)]
pub struct StandaloneRlr<S: LogStore> {
    cfg: StandaloneRlrConfig,
    store: S,
    commit_tx: broadcast::Sender<CommittedEntry>,
}

impl<S: LogStore> StandaloneRlr<S> {
    pub fn new(cfg: StandaloneRlrConfig, store: S) -> Self {
        let (commit_tx, _rx) = broadcast::channel(cfg.commit_buffer);
        Self {
            cfg,
            store,
            commit_tx,
        }
    }

    pub fn role(&self) -> NodeRole {
        self.cfg.role
    }

    pub fn leader(&self) -> Option<LeaderInfo> {
        self.cfg.leader.clone()
    }
}

#[async_trait]
impl<S: LogStore> RlrApi for StandaloneRlr<S> {
    fn cluster_info(&self) -> ClusterInfo {
        ClusterInfo {
            self_node_id: self.cfg.node_id,
            role: self.cfg.role,
            leader: self.cfg.leader.clone(),
        }
    }

    async fn propose(&self, command: Bytes) -> Result<LogId, RlrError> {
        if self.cfg.role != NodeRole::Leader {
            return Err(RlrError::NotLeader {
                leader: self.cfg.leader.clone(),
            });
        }

        // Append first (durable ordering), then broadcast.
        let broadcast_payload = command.clone();
        let log_id = self.store.append(self.cfg.term, command).await?;

        // Best-effort fanout: if there are no subscribers, this is still a successful commit.
        let _ = self.commit_tx.send(CommittedEntry {
            log_id,
            payload: broadcast_payload,
        });

        Ok(log_id)
    }

    async fn propose_batch(&self, commands: Vec<Bytes>) -> Result<Vec<LogId>, RlrError> {
        if self.cfg.role != NodeRole::Leader {
            return Err(RlrError::NotLeader {
                leader: self.cfg.leader.clone(),
            });
        }

        let log_ids = self
            .store
            .append_batch(self.cfg.term, commands.clone())
            .await?;

        for (log_id, payload) in log_ids.iter().copied().zip(commands.into_iter()) {
            let _ = self.commit_tx.send(CommittedEntry { log_id, payload });
        }

        Ok(log_ids)
    }

    async fn subscribe(&self, from_index: GlobalIndex) -> Result<Subscription, RlrError> {
        let from = from_index.max(1);
        let rx = self.commit_tx.subscribe();

        // How `from_index` works (practical):
        // - `from_index = 1`: replay everything this node has, then follow new commits.
        // - `from_index = last_applied + 1`: resume from a consumer's checkpoint.
        // - `from_index > last_index`: skip replay and only receive future commits.
        //
        // How we avoid gaps between replay and live:
        // - Subscribe to the live broadcast first (so we don't miss concurrent commits).
        // - Snapshot an upper bound from the store (`upper`) and replay `[from, upper]`.
        // - Then yield only live commits with `index > upper`.
        //
        // This can drop some queued live messages that are already covered by replay,
        // but preserves ordered, gap-free delivery.
        let upper = self.store.last_index().await?;
        let replay = if from <= upper {
            self.store.read_range(from, upper).await?
        } else {
            Vec::new()
        };

        let replay_stream = futures::stream::iter(replay.into_iter().map(Ok));

        let live_stream =
            futures::stream::unfold((rx, from, upper), |(mut rx, from, upper)| async move {
                loop {
                    match rx.recv().await {
                        Ok(entry) => {
                            let idx = entry.log_id.index;
                            if idx > upper && idx >= from {
                                return Some((Ok(entry), (rx, from, upper)));
                            }
                            continue;
                        }
                        Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                            return Some((Err(RlrError::SubscriptionLagged(n)), (rx, from, upper)));
                        }
                        Err(tokio::sync::broadcast::error::RecvError::Closed) => return None,
                    }
                }
            });

        Ok(Subscription::new(replay_stream.chain(live_stream)))
    }
}

/// An in-memory store (fast) used for tests and in-process demos.
#[derive(Debug, Default)]
pub struct MemLogStore {
    inner: tokio::sync::Mutex<Vec<CommittedEntry>>,
}

impl MemLogStore {
    pub fn new() -> Self {
        Self::default()
    }
}

#[async_trait]
impl LogStore for MemLogStore {
    async fn last_index(&self) -> Result<GlobalIndex, RlrError> {
        let guard = self.inner.lock().await;
        Ok(guard.last().map(|e| e.log_id.index).unwrap_or(0))
    }

    async fn append(&self, term: Term, payload: Bytes) -> Result<LogId, RlrError> {
        let mut guard = self.inner.lock().await;
        let next = guard.last().map(|e| e.log_id.index + 1).unwrap_or(1);
        let log_id = LogId { term, index: next };
        guard.push(CommittedEntry { log_id, payload });
        Ok(log_id)
    }

    async fn read_range(
        &self,
        from: GlobalIndex,
        to_inclusive: GlobalIndex,
    ) -> Result<Vec<CommittedEntry>, RlrError> {
        if to_inclusive < from {
            return Ok(Vec::new());
        }
        let guard = self.inner.lock().await;
        let from = from.max(1);
        Ok(guard
            .iter()
            .filter(|e| e.log_id.index >= from && e.log_id.index <= to_inclusive)
            .cloned()
            .collect())
    }
}

#[cfg(feature = "file-store")]
mod file_store {
    use super::*;
    use tokio::fs::{self, File, OpenOptions};
    use tokio::io::{AsyncReadExt, AsyncWriteExt, BufReader, BufWriter};
    use tokio::sync::Mutex;

    // WAL format v2:
    // [MAGIC:4] then repeated records:
    // [TERM:u64][INDEX:u64][LEN:u32][PAYLOAD:LEN]
    const MAGIC: &[u8; 4] = b"RLR2";

    #[derive(Debug)]
    pub struct FileLogStore {
        path: PathBuf,
        writer: Mutex<BufWriter<File>>,
        last_index: std::sync::atomic::AtomicU64,
    }

    impl FileLogStore {
        pub async fn open(path: impl AsRef<Path>) -> Result<Self, RlrError> {
            let path = path.as_ref().to_path_buf();
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent).await?;
            }

            let mut last = 0u64;

            // Ensure file exists and has the magic header.
            let mut init = OpenOptions::new()
                .create(true)
                .read(true)
                .write(true)
                .open(&path)
                .await?;

            let meta = init.metadata().await?;
            if meta.len() == 0 {
                init.write_all(MAGIC).await?;
                init.flush().await?;
            } else {
                let mut header = [0u8; 4];
                init.read_exact(&mut header).await?;
                if &header != MAGIC {
                    return Err(RlrError::CorruptLog("bad magic".to_string()));
                }
            }

            // Scan for last index.
            {
                let reader_file = File::open(&path).await?;
                let mut reader = BufReader::new(reader_file);

                let mut header = [0u8; 4];
                reader.read_exact(&mut header).await?;
                if &header != MAGIC {
                    return Err(RlrError::CorruptLog("bad magic".to_string()));
                }

                loop {
                    let Some(_term) = read_u64_opt(&mut reader).await? else {
                        break;
                    };
                    let Some(index) = read_u64_opt(&mut reader).await? else {
                        return Err(RlrError::CorruptLog("truncated index".to_string()));
                    };
                    let Some(len) = read_u32_opt(&mut reader).await? else {
                        return Err(RlrError::CorruptLog("truncated length".to_string()));
                    };
                    skip_bytes(&mut reader, len as usize).await?;
                    last = index;
                }
            }

            let writer_file = OpenOptions::new()
                .create(true)
                .append(true)
                .open(&path)
                .await?;
            let writer = Mutex::new(BufWriter::new(writer_file));

            Ok(Self {
                path,
                writer,
                last_index: std::sync::atomic::AtomicU64::new(last),
            })
        }
    }

    async fn read_u64_opt<R: tokio::io::AsyncRead + Unpin>(
        reader: &mut R,
    ) -> Result<Option<u64>, RlrError> {
        let mut buf = [0u8; 8];
        match reader.read_exact(&mut buf).await {
            Ok(_) => Ok(Some(u64::from_le_bytes(buf))),
            Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => Ok(None),
            Err(e) => Err(RlrError::Storage(e)),
        }
    }

    async fn read_u32_opt<R: tokio::io::AsyncRead + Unpin>(
        reader: &mut R,
    ) -> Result<Option<u32>, RlrError> {
        let mut buf = [0u8; 4];
        match reader.read_exact(&mut buf).await {
            Ok(_) => Ok(Some(u32::from_le_bytes(buf))),
            Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => Ok(None),
            Err(e) => Err(RlrError::Storage(e)),
        }
    }

    async fn skip_bytes<R: tokio::io::AsyncRead + Unpin>(
        reader: &mut R,
        n: usize,
    ) -> Result<(), RlrError> {
        let mut remaining = n;
        let mut scratch = [0u8; 8192];
        while remaining > 0 {
            let take = remaining.min(scratch.len());
            let read = reader.read(&mut scratch[..take]).await?;
            if read == 0 {
                return Err(RlrError::CorruptLog("truncated payload".to_string()));
            }
            remaining -= read;
        }
        Ok(())
    }

    #[async_trait]
    impl LogStore for FileLogStore {
        async fn last_index(&self) -> Result<GlobalIndex, RlrError> {
            Ok(self.last_index.load(std::sync::atomic::Ordering::Relaxed))
        }

        async fn append(&self, term: Term, payload: Bytes) -> Result<LogId, RlrError> {
            let index = self
                .last_index
                .fetch_add(1, std::sync::atomic::Ordering::SeqCst)
                + 1;

            let mut writer = self.writer.lock().await;
            writer.write_all(&term.to_le_bytes()).await?;
            writer.write_all(&index.to_le_bytes()).await?;
            let len: u32 = payload
                .len()
                .try_into()
                .map_err(|_| RlrError::CorruptLog("payload too large".to_string()))?;
            writer.write_all(&len.to_le_bytes()).await?;
            writer.write_all(&payload).await?;
            writer.flush().await?;
            Ok(LogId { term, index })
        }

        async fn append_batch(
            &self,
            term: Term,
            payloads: Vec<Bytes>,
        ) -> Result<Vec<LogId>, RlrError> {
            if payloads.is_empty() {
                return Ok(Vec::new());
            }

            let mut writer = self.writer.lock().await;
            let mut out = Vec::with_capacity(payloads.len());

            for payload in payloads {
                let index = self
                    .last_index
                    .fetch_add(1, std::sync::atomic::Ordering::SeqCst)
                    + 1;

                writer.write_all(&term.to_le_bytes()).await?;
                writer.write_all(&index.to_le_bytes()).await?;
                let len: u32 = payload
                    .len()
                    .try_into()
                    .map_err(|_| RlrError::CorruptLog("payload too large".to_string()))?;
                writer.write_all(&len.to_le_bytes()).await?;
                writer.write_all(&payload).await?;
                out.push(LogId { term, index });
            }

            writer.flush().await?;
            Ok(out)
        }

        async fn read_range(
            &self,
            from: GlobalIndex,
            to_inclusive: GlobalIndex,
        ) -> Result<Vec<CommittedEntry>, RlrError> {
            if to_inclusive < from {
                return Ok(Vec::new());
            }

            let file = File::open(&self.path).await?;
            let mut reader = BufReader::new(file);

            let mut header = [0u8; 4];
            reader.read_exact(&mut header).await?;
            if &header != MAGIC {
                return Err(RlrError::CorruptLog("bad magic".to_string()));
            }

            let mut out = Vec::new();
            let from = from.max(1);

            loop {
                let Some(term) = read_u64_opt(&mut reader).await? else {
                    break;
                };
                let Some(index) = read_u64_opt(&mut reader).await? else {
                    return Err(RlrError::CorruptLog("truncated index".to_string()));
                };
                let Some(len) = read_u32_opt(&mut reader).await? else {
                    return Err(RlrError::CorruptLog("truncated length".to_string()));
                };
                let mut payload = vec![0u8; len as usize];
                reader.read_exact(&mut payload).await?;

                if index >= from && index <= to_inclusive {
                    out.push(CommittedEntry {
                        log_id: LogId { term, index },
                        payload: Bytes::from(payload),
                    });
                }
                if index >= to_inclusive {
                    // Index is strictly increasing; we can stop early.
                    break;
                }
            }

            Ok(out)
        }
    }

    pub type DefaultFileRlr = StandaloneRlr<FileLogStore>;

    pub async fn open_default_file_rlr(
        cfg: StandaloneRlrConfig,
        path: impl AsRef<Path>,
    ) -> Result<DefaultFileRlr, RlrError> {
        let store = FileLogStore::open(path).await?;
        Ok(StandaloneRlr::new(cfg, store))
    }
}

#[cfg(feature = "file-store")]
pub use file_store::{DefaultFileRlr, FileLogStore, open_default_file_rlr};

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn leader_propose_and_subscribe_live() {
        let cfg = StandaloneRlrConfig {
            node_id: 1,
            term: 1,
            role: NodeRole::Leader,
            leader: Some(LeaderInfo {
                node_id: 1,
                advertise_addr: "n1".to_string(),
            }),
            commit_buffer: 16,
        };
        let rlr = StandaloneRlr::new(cfg, MemLogStore::new());

        let mut sub = rlr.subscribe(1).await.unwrap();

        let log_id = rlr.propose(Bytes::from_static(b"abc")).await.unwrap();
        assert_eq!(log_id.term, 1);
        assert_eq!(log_id.index, 1);

        let entry = sub.next().await.unwrap().unwrap();
        assert_eq!(entry.log_id.term, 1);
        assert_eq!(entry.log_id.index, 1);
        assert_eq!(entry.payload, Bytes::from_static(b"abc"));
    }

    #[tokio::test]
    async fn follower_rejects_propose_with_redirect() {
        let cfg = StandaloneRlrConfig {
            node_id: 2,
            term: 1,
            role: NodeRole::Follower,
            leader: Some(LeaderInfo {
                node_id: 1,
                advertise_addr: "leader:123".to_string(),
            }),
            commit_buffer: 16,
        };
        let rlr = StandaloneRlr::new(cfg, MemLogStore::new());

        let err = rlr.propose(Bytes::from_static(b"x")).await.unwrap_err();
        match err {
            RlrError::NotLeader { leader } => {
                let leader = leader.unwrap();
                assert_eq!(leader.node_id, 1);
                assert_eq!(leader.advertise_addr, "leader:123");
            }
            other => panic!("unexpected error: {other:?}"),
        }
    }

    #[cfg(feature = "file-store")]
    #[tokio::test]
    async fn file_store_replays_committed_entries() {
        let path = std::env::temp_dir().join(format!("rlr-{}.wal", uuid::Uuid::new_v4()));

        let cfg = StandaloneRlrConfig {
            node_id: 1,
            term: 7,
            role: NodeRole::Leader,
            leader: Some(LeaderInfo {
                node_id: 1,
                advertise_addr: "n1".to_string(),
            }),
            commit_buffer: 16,
        };

        let rlr = open_default_file_rlr(cfg, &path).await.unwrap();
        rlr.propose(Bytes::from_static(b"a")).await.unwrap();
        rlr.propose(Bytes::from_static(b"b")).await.unwrap();

        let cfg2 = StandaloneRlrConfig {
            node_id: 2,
            term: 7,
            role: NodeRole::Follower,
            leader: Some(LeaderInfo {
                node_id: 1,
                advertise_addr: "n1".to_string(),
            }),
            commit_buffer: 16,
        };
        let rlr2 = open_default_file_rlr(cfg2, &path).await.unwrap();

        let mut sub = rlr2.subscribe(1).await.unwrap();
        let e1 = sub.next().await.unwrap().unwrap();
        let e2 = sub.next().await.unwrap().unwrap();

        assert_eq!(e1.log_id.term, 7);
        assert_eq!(e1.log_id.index, 1);
        assert_eq!(e1.payload, Bytes::from_static(b"a"));
        assert_eq!(e2.log_id.term, 7);
        assert_eq!(e2.log_id.index, 2);
        assert_eq!(e2.payload, Bytes::from_static(b"b"));

        let _ = tokio::fs::remove_file(&path).await;
    }
}

use crate::error::Result;
use serde::{Serialize, de::DeserializeOwned};

/// Trait for events that can be stored in the message buffer
/// Each event must have a unique, monotonically increasing ID
pub trait OutputEvent: Clone + Send + Sync + Serialize + DeserializeOwned + 'static {
    /// Get the unique ID of this event
    /// IDs must be monotonically increasing and unique
    fn id(&self) -> u64;

    /// Get the previous event ID (for detecting gaps)
    fn prev_id(&self) -> u64;

    /// Validate that this event follows the previous one consecutively
    /// IDs must be strictly consecutive (id == prev_id + 1) within a batch
    fn validate_sequence(&self, prev: &Self) -> bool {
        self.prev_id() == prev.id() && self.id() == prev.id() + 1
    }
}

/// Reader interface for consuming messages sequentially
#[async_trait::async_trait]
pub trait MsgBufReader<T: OutputEvent> {
    /// Get next message in order by id ascending
    /// May block until sync() is done or data becomes available
    /// Returns None if no more messages are available (reached the end)
    async fn next(&mut self) -> Result<Option<T>>;

    /// Get the current position (next ID to be returned)
    fn position(&self) -> u64;

    /// Reset reader to a specific position
    async fn seek(&mut self, start_id: u64) -> Result<()>;
}

/// Synchronization interface for gap-filling
#[async_trait::async_trait]
pub trait MsgBufSync<T: OutputEvent> {
    /// Synchronize all output from [start_id, min(start_id + limit, latest_id))
    /// The result is sorted by id in ascending order
    /// Returns empty vec if start_id is beyond current data
    async fn sync(&self, start_id: u64, limit: usize) -> Result<Vec<T>>;

    /// Get the current maximum ID available in the buffer
    async fn max_id(&self) -> Result<Option<u64>>;

    /// Get the minimum ID available in the buffer (after trimming)
    async fn min_id(&self) -> Result<Option<u64>>;

    /// Check if a specific ID exists in the buffer
    async fn contains(&self, id: u64) -> Result<bool>;
}

/// Writer interface for appending and maintaining the buffer
#[async_trait::async_trait]
pub trait MsgBufWriter<T: OutputEvent> {
    /// Append a batch of OutputEvent, events must be sorted by id in ascending order
    /// Append ensures that batch is persisted to both memory and disk
    /// Returns the number of events successfully written
    async fn append(&mut self, batch: Vec<T>) -> Result<usize>;

    /// Delete all messages with id < before_id
    /// This operation is driven by Raft to ensure cluster consistency
    /// Returns the number of messages deleted
    async fn trim(&mut self, before_id: u64) -> Result<usize>;

    /// Flush any pending writes to disk
    async fn flush(&mut self) -> Result<()>;

    /// Get buffer statistics
    async fn stats(&self) -> Result<BufferStats>;
}

/// Statistics about the message buffer
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct BufferStats {
    /// Total number of messages in buffer
    pub total_count: u64,
    /// Number of messages in hot (RingBuffer) storage
    pub hot_count: usize,
    /// Number of messages in cold (RocksDB) storage
    pub cold_count: u64,
    /// Minimum ID available
    pub min_id: Option<u64>,
    /// Maximum ID available
    pub max_id: Option<u64>,
    /// RingBuffer hit rate (0.0 - 1.0)
    pub hit_rate: f64,
    /// Total bytes used (approximate)
    pub bytes_used: u64,
    /// Last trim timestamp
    pub last_trim_ts: Option<u64>,
}

impl Default for BufferStats {
    fn default() -> Self {
        Self {
            total_count: 0,
            hot_count: 0,
            cold_count: 0,
            min_id: None,
            max_id: None,
            hit_rate: 0.0,
            bytes_used: 0,
            last_trim_ts: None,
        }
    }
}

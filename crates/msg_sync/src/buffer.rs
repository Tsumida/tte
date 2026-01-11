use crate::{
    error::{MsgBufError, Result},
    metrics::MsgBufMetrics,
    storage::{PersistentStorage, StorageConfig},
    traits::{BufferStats, MsgBufReader, MsgBufSync, MsgBufWriter, OutputEvent},
};
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::RwLock;
use tracing::{debug, error, info};

/// Configuration for the message buffer
#[derive(Debug, Clone)]
pub struct MsgBufferConfig {
    /// Configuration for the cold (persistent) storage
    pub storage: StorageConfig,
    /// Component name for metrics (e.g., "match_engine", "oms")
    pub component: String,
}

impl Default for MsgBufferConfig {
    fn default() -> Self {
        Self {
            storage: StorageConfig::default(),
            component: "msg_buffer".to_string(),
        }
    }
}

/// Unified message buffer combining hot (RingBuffer) and cold (RocksDB) storage
/// This is the main implementation of the message synchronization system
pub struct MsgBuffer<T: OutputEvent> {
    cold: PersistentStorage<T>,
    metrics: Option<Arc<MsgBufMetrics>>,
    config: MsgBufferConfig,
    // Track metadata for efficient operations
    metadata: Arc<RwLock<BufferMetadata>>,
}

#[derive(Debug, Clone)]
struct BufferMetadata {
    min_id: Option<u64>,
    max_id: Option<u64>,
    total_appends: u64,
    total_trims: u64,
    last_trim_ts: Option<u64>,
}

impl Default for BufferMetadata {
    fn default() -> Self {
        Self {
            min_id: None,
            max_id: None,
            total_appends: 0,
            total_trims: 0,
            last_trim_ts: None,
        }
    }
}

impl<T: OutputEvent> MsgBuffer<T> {
    /// Create a new message buffer
    pub fn new(config: MsgBufferConfig) -> Result<Self> {
        let cold = PersistentStorage::new(config.storage.clone())?;

        Ok(Self {
            cold,
            metrics: None,
            config,
            metadata: Arc::new(RwLock::new(BufferMetadata::default())),
        })
    }

    /// Create a new message buffer with metrics
    pub fn with_metrics(config: MsgBufferConfig, metrics: Arc<MsgBufMetrics>) -> Result<Self> {
        let mut buffer = Self::new(config)?;
        buffer.metrics = Some(metrics);
        Ok(buffer)
    }

    /// Get a message by ID from persistent storage
    pub async fn get(&self, id: u64) -> Result<Option<T>> {
        let start = Instant::now();
        let result = self.cold.get(id);
        let duration = start.elapsed().as_secs_f64() * 1000.0;
        if let Some(metrics) = &self.metrics {
            metrics.record_query(duration, result.is_ok(), false);
        }
        result
    }

    /// Get a range of messages [start_id, start_id + limit)
    /// First tries to get from hot buffer, then fills gaps from cold storage
    pub async fn get_range_may_sync(&self, start_id: u64, limit: usize) -> Result<Vec<T>> {
        if limit == 0 {
            return Ok(vec![]);
        }

        let start = Instant::now();

        // error: max_id < start_id
        let metadata = self.metadata.read().await;
        if let Some(max_id) = metadata.max_id {
            if start_id > max_id {
                if let Some(metrics) = &self.metrics {
                    metrics.record_sync_lag(start_id, max_id);
                }
                return Err(MsgBufError::DataNotYetAvailable {
                    requested_id: start_id,
                    current_max_id: max_id,
                });
            }
        }

        // error: start_id < min_id, data trimmed
        if let Some(min_id) = metadata.min_id {
            if start_id < min_id {
                if let Some(metrics) = &self.metrics {
                    metrics.record_data_loss(start_id, min_id);
                }
                return Err(MsgBufError::DataTrimmed {
                    requested_id: start_id,
                    min_available_id: min_id,
                });
            }
        }
        drop(metadata);

        // Fetch directly from persistent storage (single source of truth)
        let cold_result = self.cold.get_range(start_id, limit)?;
        let duration = start.elapsed().as_secs_f64() * 1000.0;
        if let Some(metrics) = &self.metrics {
            metrics.record_query(duration, true, false);
        }
        Ok(cold_result)
    }

    /// Update metadata after operations
    async fn update_metadata(&self, new_min: Option<u64>, new_max: Option<u64>) -> Result<()> {
        let mut metadata = self.metadata.write().await;

        if let Some(min) = new_min {
            metadata.min_id = Some(metadata.min_id.map_or(min, |old| old.min(min)));
        }

        if let Some(max) = new_max {
            metadata.max_id = Some(metadata.max_id.map_or(max, |old| old.max(max)));
        }

        if let Some(metrics) = &self.metrics {
            metrics.update_id_range(metadata.min_id, metadata.max_id);
        }

        Ok(())
    }
}

impl<T: OutputEvent> Clone for MsgBuffer<T> {
    fn clone(&self) -> Self {
        Self {
            cold: self.cold.clone(),
            metrics: self.metrics.clone(),
            config: self.config.clone(),
            metadata: self.metadata.clone(),
        }
    }
}

#[async_trait::async_trait]
impl<T: OutputEvent> MsgBufSync<T> for MsgBuffer<T> {
    async fn sync(&self, start_id: u64, limit: usize) -> Result<Vec<T>> {
        debug!(
            start_id = start_id,
            limit = limit,
            "Syncing messages from buffer"
        );
        self.get_range_may_sync(start_id, limit).await
    }

    async fn max_id(&self) -> Result<Option<u64>> {
        let metadata = self.metadata.read().await;
        Ok(metadata.max_id)
    }

    async fn min_id(&self) -> Result<Option<u64>> {
        let metadata = self.metadata.read().await;
        Ok(metadata.min_id)
    }

    async fn contains(&self, id: u64) -> Result<bool> {
        // Check cold storage (single truth)
        self.cold.contains(id)
    }
}

#[async_trait::async_trait]
impl<T: OutputEvent> MsgBufWriter<T> for MsgBuffer<T> {
    async fn append(&mut self, batch: Vec<T>) -> Result<usize> {
        if batch.is_empty() {
            return Ok(0);
        }

        let start = Instant::now();
        let count = batch.len();

        // Validate sequence
        for window in batch.windows(2) {
            if !window[1].validate_sequence(&window[0]) {
                return Err(MsgBufError::DataCorrupted {
                    expected_id: window[0].id() + 1,
                    found_id: window[1].id(),
                });
            }
        }

        let min_id = batch.first().map(|m| m.id());
        let max_id = batch.last().map(|m| m.id());

        // Write to persistent storage first (durability)
        match self.cold.put_batch(batch.clone()) {
            Ok(_) => {}
            Err(e) => {
                error!(error = ?e, "Failed to persist batch to cold storage");
                if let Some(metrics) = &self.metrics {
                    metrics.record_append(count as u64, 0.0, false);
                }
                return Err(e);
            }
        }

        // Update metadata
        self.update_metadata(min_id, max_id).await?;

        let duration = start.elapsed().as_secs_f64() * 1000.0;
        if let Some(metrics) = &self.metrics {
            metrics.record_append(count as u64, duration, true);

            // Update buffer size metrics (no hot buffer)
            let cold_count = self.cold.count().unwrap_or(0);
            metrics.update_buffer_size(cold_count, 0);
        }

        let mut metadata = self.metadata.write().await;
        metadata.total_appends += count as u64;

        debug!(count = count, duration_ms = duration, "Appended batch");
        Ok(count)
    }

    async fn trim(&mut self, before_id: u64) -> Result<usize> {
        info!(before_id = before_id, "Trimming messages");

        let start = Instant::now();

        // Trim cold storage first
        let cold_deleted = match self.cold.trim(before_id) {
            Ok(count) => count,
            Err(e) => {
                error!(error = ?e, "Failed to trim cold storage");
                if let Some(metrics) = &self.metrics {
                    metrics.record_trim(0, 0.0, false);
                }
                return Err(e);
            }
        };

        let total_deleted = cold_deleted;

        // Update metadata
        let mut metadata = self.metadata.write().await;
        metadata.min_id = Some(before_id);
        metadata.total_trims += 1;
        metadata.last_trim_ts = Some(
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs(),
        );

        let duration = start.elapsed().as_secs_f64() * 1000.0;
        if let Some(metrics) = &self.metrics {
            metrics.record_trim(total_deleted as u64, duration, true);
            metrics.update_id_range(metadata.min_id, metadata.max_id);
        }

        info!(
            before_id = before_id,
            deleted = total_deleted,
            duration_ms = duration,
            "Trimmed messages"
        );

        Ok(total_deleted)
    }

    async fn flush(&mut self) -> Result<()> {
        self.cold.flush()
    }

    async fn stats(&self) -> Result<BufferStats> {
        let metadata = self.metadata.read().await;
        let cold_count = self.cold.count()?;

        Ok(BufferStats {
            total_count: cold_count,
            hot_count: 0,
            cold_count,
            min_id: metadata.min_id,
            max_id: metadata.max_id,
            hit_rate: 0.0,
            bytes_used: 0, // TODO: implement proper size calculation
            last_trim_ts: metadata.last_trim_ts,
        })
    }
}

/// Reader implementation for sequential consumption
pub struct MsgBufReaderImpl<T: OutputEvent> {
    buffer: Arc<MsgBuffer<T>>,
    current_position: u64,
}

impl<T: OutputEvent> MsgBufReaderImpl<T> {
    pub fn new(buffer: Arc<MsgBuffer<T>>, start_position: u64) -> Self {
        Self {
            buffer,
            current_position: start_position,
        }
    }
}

#[async_trait::async_trait]
impl<T: OutputEvent> MsgBufReader<T> for MsgBufReaderImpl<T> {
    async fn next(&mut self) -> Result<Option<T>> {
        match self.buffer.get(self.current_position).await {
            Ok(Some(msg)) => {
                self.current_position = msg.id() + 1;
                Ok(Some(msg))
            }
            Ok(None) => Ok(None),
            Err(e) if e.is_temporary() => {
                // todo:
                // Wait a bit and retry
                tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
                Err(e)
            }
            Err(e) => Err(e),
        }
    }

    fn position(&self) -> u64 {
        self.current_position
    }

    async fn seek(&mut self, start_id: u64) -> Result<()> {
        self.current_position = start_id;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::{Deserialize, Serialize};
    use tempfile::TempDir;

    #[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
    struct TestEvent {
        id: u64,
        prev_id: u64,
        data: String,
    }

    impl OutputEvent for TestEvent {
        fn id(&self) -> u64 {
            self.id
        }

        fn prev_id(&self) -> u64 {
            self.prev_id
        }
    }

    fn create_test_buffer() -> (MsgBuffer<TestEvent>, TempDir) {
        let tmp_dir = TempDir::new().unwrap();
        let config = MsgBufferConfig {
            storage: StorageConfig {
                db_path: tmp_dir.path().to_str().unwrap().to_string(),
                cf_name: "test_buffer".to_string(),
                use_compression: false,
                write_buffer_size: 1024 * 1024,
            },
            component: "test".to_string(),
        };

        let buffer = MsgBuffer::new(config).unwrap();
        (buffer, tmp_dir)
    }

    #[tokio::test]
    async fn test_msg_buffer_append_and_get() {
        let (mut buffer, _tmp) = create_test_buffer();

        let events: Vec<TestEvent> = (1..=5)
            .map(|i| TestEvent {
                id: i,
                prev_id: i - 1,
                data: format!("msg{}", i),
            })
            .collect();

        let count = buffer.append(events.clone()).await.unwrap();
        assert_eq!(count, 5);

        // Get from hot buffer
        let msg = buffer.get(3).await.unwrap().unwrap();
        assert_eq!(msg.id(), 3);

        // Get range
        let range = buffer.get_range_may_sync(2, 3).await.unwrap();
        assert_eq!(range.len(), 3);
        assert_eq!(range[0].id(), 2);
        assert_eq!(range[2].id(), 4);
    }

    #[tokio::test]
    async fn test_msg_buffer_cold_storage_fallback() {
        let (mut buffer, _tmp) = create_test_buffer();

        // Add more than ring buffer capacity (10)
        let events: Vec<TestEvent> = (1..=20)
            .map(|i| TestEvent {
                id: i,
                prev_id: i - 1,
                data: format!("msg{}", i),
            })
            .collect();

        buffer.append(events).await.unwrap();

        // Earlier messages should be evicted from hot buffer but still in cold
        let msg = buffer.get(1).await.unwrap().unwrap();
        assert_eq!(msg.id(), 1);
    }

    #[tokio::test]
    async fn test_msg_buffer_trim() {
        let (mut buffer, _tmp) = create_test_buffer();

        let events: Vec<TestEvent> = (1..=10)
            .map(|i| TestEvent {
                id: i,
                prev_id: i - 1,
                data: format!("msg{}", i),
            })
            .collect();

        buffer.append(events).await.unwrap();

        // Trim messages < 5
        let deleted = buffer.trim(5).await.unwrap();
        assert!(deleted >= 4);

        // Should get DataTrimmed error for id < 5
        let result = buffer.get_range_may_sync(1, 10).await;
        assert!(matches!(result, Err(MsgBufError::DataTrimmed { .. })));

        // Should still be able to get id >= 5
        let msg = buffer.get(5).await.unwrap().unwrap();
        assert_eq!(msg.id(), 5);
    }

    #[tokio::test]
    async fn test_msg_buffer_stats() {
        let (mut buffer, _tmp) = create_test_buffer();

        let events: Vec<TestEvent> = (1..=5)
            .map(|i| TestEvent {
                id: i,
                prev_id: i - 1,
                data: format!("msg{}", i),
            })
            .collect();

        buffer.append(events).await.unwrap();

        let stats = buffer.stats().await.unwrap();
        assert_eq!(stats.min_id, Some(1));
        assert_eq!(stats.max_id, Some(5));
        assert_eq!(stats.total_count, 5);
    }
}

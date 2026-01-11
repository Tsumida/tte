use crate::{error::Result, traits::OutputEvent, MsgBufError};
use std::collections::VecDeque;
use std::sync::{Arc, RwLock};
use tracing::{debug, trace};

/// Configuration for RingBuffer
#[derive(Debug, Clone)]
pub struct RingBufferConfig {
    /// Maximum number of messages to keep in memory
    pub capacity: usize,
    /// Whether to overwrite old messages when buffer is full
    pub overwrite_on_full: bool,
}

impl Default for RingBufferConfig {
    fn default() -> Self {
        Self {
            capacity: 10_000, // Keep last 10k messages in memory
            overwrite_on_full: true,
        }
    }
}

/// Thread-safe ring buffer for hot (recent) messages
/// Uses a VecDeque for efficient insertion/removal at both ends
pub struct RingBuffer<T: OutputEvent> {
    buffer: Arc<RwLock<VecDeque<T>>>,
    config: RingBufferConfig,
    stats: Arc<RwLock<RingBufferStats>>,
}

#[derive(Debug, Clone, Default)]
struct RingBufferStats {
    hits: u64,
    misses: u64,
    evictions: u64,
}

impl<T: OutputEvent> RingBuffer<T> {
    pub fn new(config: RingBufferConfig) -> Self {
        Self {
            buffer: Arc::new(RwLock::new(VecDeque::with_capacity(config.capacity))),
            config,
            stats: Arc::new(RwLock::new(RingBufferStats::default())),
        }
    }

    /// Push a batch of messages to the buffer
    /// Messages must be sorted by ID in ascending order
    pub fn push_batch(&self, batch: Vec<T>) -> Result<usize> {
        if batch.is_empty() {
            return Ok(0);
        }

        let mut buffer = self.buffer.write().map_err(|e| {
            MsgBufError::StorageError(anyhow::anyhow!("Lock poisoned: {}", e))
        })?;

        let mut pushed = 0;
        for msg in batch {
            // Validate ordering if buffer is not empty
            if let Some(last) = buffer.back() {
                if msg.id() <= last.id() {
                    return Err(MsgBufError::InvalidOperation(format!(
                        "Messages must be in ascending order: {} <= {}",
                        msg.id(),
                        last.id()
                    )));
                }
            }

            // Check capacity
            if buffer.len() >= self.config.capacity {
                if self.config.overwrite_on_full {
                    // Remove oldest message
                    buffer.pop_front();
                    let mut stats = self.stats.write().map_err(|e| {
                        MsgBufError::StorageError(anyhow::anyhow!("Lock poisoned: {}", e))
                    })?;
                    stats.evictions += 1;
                    debug!(id = msg.id(), "Evicted oldest message from RingBuffer");
                } else {
                    return Err(MsgBufError::BufferFull {
                        capacity: self.config.capacity,
                        size: buffer.len(),
                    });
                }
            }

            buffer.push_back(msg);
            pushed += 1;
        }

        trace!(count = pushed, "Pushed messages to RingBuffer");
        Ok(pushed)
    }

    /// Get a message by ID
    pub fn get(&self, id: u64) -> Result<Option<T>> {
        let buffer = self.buffer.read().map_err(|e| {
            MsgBufError::StorageError(anyhow::anyhow!("Lock poisoned: {}", e))
        })?;

        let result = buffer.iter().find(|msg| msg.id() == id).cloned();

        let mut stats = self.stats.write().map_err(|e| {
            MsgBufError::StorageError(anyhow::anyhow!("Lock poisoned: {}", e))
        })?;

        if result.is_some() {
            stats.hits += 1;
            trace!(id = id, "RingBuffer hit");
        } else {
            stats.misses += 1;
            trace!(id = id, "RingBuffer miss");
        }

        Ok(result)
    }

    /// Get a range of messages [start_id, end_id)
    pub fn get_range(&self, start_id: u64, limit: usize) -> Result<Vec<T>> {
        let buffer = self.buffer.read().map_err(|e| {
            MsgBufError::StorageError(anyhow::anyhow!("Lock poisoned: {}", e))
        })?;

        let result: Vec<T> = buffer
            .iter()
            .skip_while(|msg| msg.id() < start_id)
            .take(limit)
            .cloned()
            .collect();

        trace!(
            start_id = start_id,
            limit = limit,
            found = result.len(),
            "RingBuffer range query"
        );

        Ok(result)
    }

    /// Check if a message with the given ID exists
    pub fn contains(&self, id: u64) -> Result<bool> {
        let buffer = self.buffer.read().map_err(|e| {
            MsgBufError::StorageError(anyhow::anyhow!("Lock poisoned: {}", e))
        })?;

        Ok(buffer.iter().any(|msg| msg.id() == id))
    }

    /// Get the minimum ID in the buffer
    pub fn min_id(&self) -> Result<Option<u64>> {
        let buffer = self.buffer.read().map_err(|e| {
            MsgBufError::StorageError(anyhow::anyhow!("Lock poisoned: {}", e))
        })?;

        Ok(buffer.front().map(|msg| msg.id()))
    }

    /// Get the maximum ID in the buffer
    pub fn max_id(&self) -> Result<Option<u64>> {
        let buffer = self.buffer.read().map_err(|e| {
            MsgBufError::StorageError(anyhow::anyhow!("Lock poisoned: {}", e))
        })?;

        Ok(buffer.back().map(|msg| msg.id()))
    }

    /// Remove all messages with id < before_id
    pub fn trim(&self, before_id: u64) -> Result<usize> {
        let mut buffer = self.buffer.write().map_err(|e| {
            MsgBufError::StorageError(anyhow::anyhow!("Lock poisoned: {}", e))
        })?;

        let initial_len = buffer.len();

        // Remove from front while id < before_id
        while let Some(front) = buffer.front() {
            if front.id() < before_id {
                buffer.pop_front();
            } else {
                break;
            }
        }

        let removed = initial_len - buffer.len();
        debug!(
            before_id = before_id,
            removed = removed,
            "Trimmed RingBuffer"
        );

        Ok(removed)
    }

    /// Get the current size of the buffer
    pub fn len(&self) -> Result<usize> {
        let buffer = self.buffer.read().map_err(|e| {
            MsgBufError::StorageError(anyhow::anyhow!("Lock poisoned: {}", e))
        })?;

        Ok(buffer.len())
    }

    /// Check if the buffer is empty
    pub fn is_empty(&self) -> Result<bool> {
        Ok(self.len()? == 0)
    }

    /// Get hit rate (hits / (hits + misses))
    pub fn hit_rate(&self) -> Result<f64> {
        let stats = self.stats.read().map_err(|e| {
            MsgBufError::StorageError(anyhow::anyhow!("Lock poisoned: {}", e))
        })?;

        let total = stats.hits + stats.misses;
        if total == 0 {
            Ok(0.0)
        } else {
            Ok(stats.hits as f64 / total as f64)
        }
    }

    /// Get the number of evictions
    pub fn evictions(&self) -> Result<u64> {
        let stats = self.stats.read().map_err(|e| {
            MsgBufError::StorageError(anyhow::anyhow!("Lock poisoned: {}", e))
        })?;

        Ok(stats.evictions)
    }

    /// Clear all messages from the buffer
    pub fn clear(&self) -> Result<()> {
        let mut buffer = self.buffer.write().map_err(|e| {
            MsgBufError::StorageError(anyhow::anyhow!("Lock poisoned: {}", e))
        })?;

        buffer.clear();
        debug!("Cleared RingBuffer");
        Ok(())
    }
}

impl<T: OutputEvent> Clone for RingBuffer<T> {
    fn clone(&self) -> Self {
        Self {
            buffer: self.buffer.clone(),
            config: self.config.clone(),
            stats: self.stats.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::{Deserialize, Serialize};

    #[derive(Debug, Clone, Serialize, Deserialize)]
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

    #[test]
    fn test_ring_buffer_basic() {
        let config = RingBufferConfig {
            capacity: 3,
            overwrite_on_full: true,
        };
        let buffer = RingBuffer::new(config);

        // Push messages
        let events = vec![
            TestEvent {
                id: 1,
                prev_id: 0,
                data: "msg1".to_string(),
            },
            TestEvent {
                id: 2,
                prev_id: 1,
                data: "msg2".to_string(),
            },
        ];

        assert_eq!(buffer.push_batch(events).unwrap(), 2);
        assert_eq!(buffer.len().unwrap(), 2);
        assert_eq!(buffer.min_id().unwrap(), Some(1));
        assert_eq!(buffer.max_id().unwrap(), Some(2));
    }

    #[test]
    fn test_ring_buffer_eviction() {
        let config = RingBufferConfig {
            capacity: 2,
            overwrite_on_full: true,
        };
        let buffer = RingBuffer::new(config);

        // Fill buffer
        let events = vec![
            TestEvent {
                id: 1,
                prev_id: 0,
                data: "msg1".to_string(),
            },
            TestEvent {
                id: 2,
                prev_id: 1,
                data: "msg2".to_string(),
            },
        ];
        buffer.push_batch(events).unwrap();

        // Push one more - should evict oldest
        let more_events = vec![TestEvent {
            id: 3,
            prev_id: 2,
            data: "msg3".to_string(),
        }];
        buffer.push_batch(more_events).unwrap();

        assert_eq!(buffer.len().unwrap(), 2);
        assert_eq!(buffer.min_id().unwrap(), Some(2)); // ID 1 was evicted
        assert_eq!(buffer.max_id().unwrap(), Some(3));
        assert_eq!(buffer.evictions().unwrap(), 1);
    }

    #[test]
    fn test_ring_buffer_trim() {
        let config = RingBufferConfig::default();
        let buffer = RingBuffer::new(config);

        let events = vec![
            TestEvent {
                id: 1,
                prev_id: 0,
                data: "msg1".to_string(),
            },
            TestEvent {
                id: 2,
                prev_id: 1,
                data: "msg2".to_string(),
            },
            TestEvent {
                id: 3,
                prev_id: 2,
                data: "msg3".to_string(),
            },
        ];
        buffer.push_batch(events).unwrap();

        // Trim messages with id < 3
        let removed = buffer.trim(3).unwrap();
        assert_eq!(removed, 2);
        assert_eq!(buffer.len().unwrap(), 1);
        assert_eq!(buffer.min_id().unwrap(), Some(3));
    }

    #[test]
    fn test_ring_buffer_range_query() {
        let config = RingBufferConfig::default();
        let buffer = RingBuffer::new(config);

        let events: Vec<TestEvent> = (1..=10)
            .map(|i| TestEvent {
                id: i,
                prev_id: i - 1,
                data: format!("msg{}", i),
            })
            .collect();
        buffer.push_batch(events).unwrap();

        // Get range [3, 7)
        let range = buffer.get_range(3, 4).unwrap();
        assert_eq!(range.len(), 4);
        assert_eq!(range[0].id(), 3);
        assert_eq!(range[3].id(), 6);
    }
}
 
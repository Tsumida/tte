use crate::{MsgBufError, error::Result, traits::OutputEvent};
use rocksdb::{ColumnFamilyDescriptor, DB, IteratorMode, Options, WriteBatch};
use std::sync::Arc;
use tracing::{debug, trace};

/// Configuration for persistent storage
#[derive(Debug, Clone)]
pub struct StorageConfig {
    /// Path to RocksDB directory
    pub db_path: String,
    /// Column family name for message buffer
    pub cf_name: String,
    /// Whether to use compression
    pub use_compression: bool,
    /// Write buffer size in bytes
    pub write_buffer_size: usize,
}

impl Default for StorageConfig {
    fn default() -> Self {
        Self {
            db_path: "/tmp/msg_buffer".to_string(),
            cf_name: "msg_buffer".to_string(),
            use_compression: true,
            write_buffer_size: 64 * 1024 * 1024, // 64MB
        }
    }
}

/// Persistent storage layer using RocksDB
pub struct PersistentStorage<T: OutputEvent> {
    db: Arc<DB>,
    cf_name: String,
    _phantom: std::marker::PhantomData<T>,
}

impl<T: OutputEvent> PersistentStorage<T> {
    /// Create a new persistent storage instance
    pub fn new(config: StorageConfig) -> Result<Self> {
        // Create database directory if it doesn't exist
        std::fs::create_dir_all(&config.db_path).map_err(|e| {
            MsgBufError::StorageError(anyhow::anyhow!("Failed to create db directory: {}", e))
        })?;

        let mut db_opts = Options::default();
        db_opts.create_if_missing(true);
        db_opts.create_missing_column_families(true);
        db_opts.set_write_buffer_size(config.write_buffer_size);

        if config.use_compression {
            db_opts.set_compression_type(rocksdb::DBCompressionType::Lz4);
        }

        let cf_descriptor = ColumnFamilyDescriptor::new(&config.cf_name, Options::default());

        let db = DB::open_cf_descriptors(&db_opts, &config.db_path, vec![cf_descriptor]).map_err(
            |e| MsgBufError::StorageError(anyhow::anyhow!("Failed to open RocksDB: {}", e)),
        )?;

        Ok(Self {
            db: Arc::new(db),
            cf_name: config.cf_name,
            _phantom: std::marker::PhantomData,
        })
    }

    /// Put a single message into storage
    pub fn put(&self, id: u64, msg: &T) -> Result<()> {
        let key = Self::id_to_key(id);
        let value = self.serialize(msg)?;

        let cf = self
            .db
            .cf_handle(&self.cf_name)
            .ok_or_else(|| MsgBufError::StorageError(anyhow::anyhow!("Column family not found")))?;

        self.db
            .put_cf(cf, &key, &value)
            .map_err(|e| MsgBufError::StorageError(anyhow::anyhow!("RocksDB put error: {}", e)))?;

        trace!(id = id, "Persisted message to RocksDB");
        Ok(())
    }

    /// Put a batch of messages into storage
    pub fn put_batch(&self, batch: Vec<T>) -> Result<usize> {
        if batch.is_empty() {
            return Ok(0);
        }

        let cf = self
            .db
            .cf_handle(&self.cf_name)
            .ok_or_else(|| MsgBufError::StorageError(anyhow::anyhow!("Column family not found")))?;

        let mut wb = WriteBatch::default();
        for msg in &batch {
            let key = Self::id_to_key(msg.id());
            let value = self.serialize(msg)?;
            wb.put_cf(cf, &key, &value);
        }

        let count = batch.len();
        self.db.write(wb).map_err(|e| {
            MsgBufError::StorageError(anyhow::anyhow!("RocksDB batch write error: {}", e))
        })?;

        debug!(count = count, "Persisted batch to RocksDB");
        Ok(count)
    }

    /// Get a message by ID
    pub fn get(&self, id: u64) -> Result<Option<T>> {
        let key = Self::id_to_key(id);

        let cf = self
            .db
            .cf_handle(&self.cf_name)
            .ok_or_else(|| MsgBufError::StorageError(anyhow::anyhow!("Column family not found")))?;

        match self.db.get_cf(cf, &key) {
            Ok(Some(value)) => {
                let msg = self.deserialize(&value)?;
                trace!(id = id, "Retrieved message from RocksDB");
                Ok(Some(msg))
            }
            Ok(None) => {
                trace!(id = id, "Message not found in RocksDB");
                Ok(None)
            }
            Err(e) => Err(MsgBufError::StorageError(anyhow::anyhow!(
                "RocksDB get error: {}",
                e
            ))),
        }
    }

    /// Get a range of messages [start_id, start_id + limit)
    pub fn get_range(&self, start_id: u64, limit: usize) -> Result<Vec<T>> {
        let start_key = Self::id_to_key(start_id);
        let mut result = Vec::with_capacity(limit);

        let cf = self
            .db
            .cf_handle(&self.cf_name)
            .ok_or_else(|| MsgBufError::StorageError(anyhow::anyhow!("Column family not found")))?;

        let iter = self.db.iterator_cf(
            cf,
            IteratorMode::From(&start_key, rocksdb::Direction::Forward),
        );

        for item in iter.take(limit) {
            match item {
                Ok((_, value)) => {
                    let msg = self.deserialize(&value)?;
                    result.push(msg);
                }
                Err(e) => {
                    return Err(MsgBufError::StorageError(anyhow::anyhow!(
                        "RocksDB iterator error: {}",
                        e
                    )));
                }
            }
        }

        trace!(
            start_id = start_id,
            limit = limit,
            found = result.len(),
            "Retrieved range from RocksDB"
        );

        Ok(result)
    }

    /// Check if a message exists
    pub fn contains(&self, id: u64) -> Result<bool> {
        let key = Self::id_to_key(id);
        let cf = self
            .db
            .cf_handle(&self.cf_name)
            .ok_or_else(|| MsgBufError::StorageError(anyhow::anyhow!("Column family not found")))?;

        self.db
            .get_cf(cf, &key)
            .map(|opt| opt.is_some())
            .map_err(|e| {
                MsgBufError::StorageError(anyhow::anyhow!("RocksDB contains error: {}", e))
            })
    }

    /// Get the minimum ID in storage
    pub fn min_id(&self) -> Result<Option<u64>> {
        let cf = self
            .db
            .cf_handle(&self.cf_name)
            .ok_or_else(|| MsgBufError::StorageError(anyhow::anyhow!("Column family not found")))?;

        let iter = self.db.iterator_cf(cf, IteratorMode::Start);

        if let Some(Ok((key, _))) = iter.take(1).next() {
            Ok(Some(Self::key_to_id(&key)))
        } else {
            Ok(None)
        }
    }

    /// Get the maximum ID in storage (expensive - requires reverse scan)
    pub fn max_id(&self) -> Result<Option<u64>> {
        let cf = self
            .db
            .cf_handle(&self.cf_name)
            .ok_or_else(|| MsgBufError::StorageError(anyhow::anyhow!("Column family not found")))?;

        let iter = self.db.iterator_cf(cf, IteratorMode::End);

        if let Some(Ok((key, _))) = iter.take(1).next() {
            Ok(Some(Self::key_to_id(&key)))
        } else {
            Ok(None)
        }
    }

    /// Delete all messages with id < before_id
    pub fn trim(&self, before_id: u64) -> Result<usize> {
        let end_key = Self::id_to_key(before_id);
        let mut deleted = 0;

        let cf = self
            .db
            .cf_handle(&self.cf_name)
            .ok_or_else(|| MsgBufError::StorageError(anyhow::anyhow!("Column family not found")))?;

        let mut wb = WriteBatch::default();

        let iter = self.db.iterator_cf(cf, IteratorMode::Start);

        for item in iter {
            match item {
                Ok((key, _)) => {
                    if key.as_ref() < end_key.as_slice() {
                        wb.delete_cf(cf, &key);
                        deleted += 1;
                    } else {
                        break;
                    }
                }
                Err(e) => {
                    return Err(MsgBufError::StorageError(anyhow::anyhow!(
                        "RocksDB iterator error: {}",
                        e
                    )));
                }
            }
        }

        if deleted > 0 {
            self.db.write(wb).map_err(|e| {
                MsgBufError::StorageError(anyhow::anyhow!("RocksDB batch delete error: {}", e))
            })?;
            debug!(before_id = before_id, deleted = deleted, "Trimmed RocksDB");
        }

        Ok(deleted)
    }

    /// Get approximate count of messages
    pub fn count(&self) -> Result<u64> {
        let cf = self
            .db
            .cf_handle(&self.cf_name)
            .ok_or_else(|| MsgBufError::StorageError(anyhow::anyhow!("Column family not found")))?;

        let iter = self.db.iterator_cf(cf, IteratorMode::Start);

        Ok(iter.count() as u64)
    }

    /// Flush any pending writes to disk
    pub fn flush(&self) -> Result<()> {
        let cf = self
            .db
            .cf_handle(&self.cf_name)
            .ok_or_else(|| MsgBufError::StorageError(anyhow::anyhow!("Column family not found")))?;

        self.db
            .flush_cf(cf)
            .map_err(|e| MsgBufError::StorageError(anyhow::anyhow!("RocksDB flush error: {}", e)))
    }

    /// Convert ID to storage key (big-endian for proper ordering)
    fn id_to_key(id: u64) -> Vec<u8> {
        id.to_be_bytes().to_vec()
    }

    /// Convert storage key back to ID
    fn key_to_id(key: &[u8]) -> u64 {
        let mut bytes = [0u8; 8];
        bytes.copy_from_slice(&key[..8]);
        u64::from_be_bytes(bytes)
    }

    /// Serialize a message
    fn serialize(&self, msg: &T) -> Result<Vec<u8>> {
        serde_json::to_vec(msg)
            .map_err(|e| MsgBufError::SerializationError(format!("Serialization failed: {}", e)))
    }

    /// Deserialize a message
    fn deserialize(&self, data: &[u8]) -> Result<T> {
        serde_json::from_slice(data)
            .map_err(|e| MsgBufError::SerializationError(format!("Deserialization failed: {}", e)))
    }
}

impl<T: OutputEvent> Clone for PersistentStorage<T> {
    fn clone(&self) -> Self {
        Self {
            db: self.db.clone(),
            cf_name: self.cf_name.clone(),
            _phantom: std::marker::PhantomData,
        }
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

    fn create_test_storage() -> (PersistentStorage<TestEvent>, TempDir) {
        let tmp_dir = TempDir::new().unwrap();
        let config = StorageConfig {
            db_path: tmp_dir.path().to_str().unwrap().to_string(),
            cf_name: "test_msg_buffer".to_string(),
            use_compression: false,
            write_buffer_size: 1024 * 1024,
        };
        let storage = PersistentStorage::new(config).unwrap();
        (storage, tmp_dir)
    }

    #[test]
    fn test_storage_put_get() {
        let (storage, _tmp) = create_test_storage();

        let event = TestEvent {
            id: 42,
            prev_id: 41,
            data: "test".to_string(),
        };

        storage.put(42, &event).unwrap();
        let retrieved = storage.get(42).unwrap();
        assert_eq!(retrieved, Some(event));
    }

    #[test]
    fn test_storage_batch() {
        let (storage, _tmp) = create_test_storage();

        let events: Vec<TestEvent> = (1..=10)
            .map(|i| TestEvent {
                id: i,
                prev_id: i - 1,
                data: format!("msg{}", i),
            })
            .collect();

        let count = storage.put_batch(events.clone()).unwrap();
        assert_eq!(count, 10);

        // Verify all messages
        for event in events {
            let retrieved = storage.get(event.id()).unwrap().unwrap();
            assert_eq!(retrieved.id(), event.id());
        }
    }

    #[test]
    fn test_storage_range() {
        let (storage, _tmp) = create_test_storage();

        let events: Vec<TestEvent> = (1..=10)
            .map(|i| TestEvent {
                id: i,
                prev_id: i - 1,
                data: format!("msg{}", i),
            })
            .collect();

        storage.put_batch(events).unwrap();

        let range = storage.get_range(3, 4).unwrap();
        assert_eq!(range.len(), 4);
        assert_eq!(range[0].id(), 3);
        assert_eq!(range[3].id(), 6);
    }

    #[test]
    fn test_storage_trim() {
        let (storage, _tmp) = create_test_storage();

        let events: Vec<TestEvent> = (1..=10)
            .map(|i| TestEvent {
                id: i,
                prev_id: i - 1,
                data: format!("msg{}", i),
            })
            .collect();

        storage.put_batch(events).unwrap();

        // Trim messages with id < 5
        let deleted = storage.trim(5).unwrap();
        assert_eq!(deleted, 4);

        // Verify trimmed messages are gone
        assert!(storage.get(1).unwrap().is_none());
        assert!(storage.get(4).unwrap().is_none());

        // Verify remaining messages
        assert!(storage.get(5).unwrap().is_some());
        assert!(storage.get(10).unwrap().is_some());
    }
}

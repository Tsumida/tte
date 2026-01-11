use thiserror::Error;

/// Errors that can occur during message buffer operations
#[derive(Error, Debug)]
pub enum MsgBufError {
    /// The requested data is not yet available (sync lag)
    /// This is a temporary condition - data may arrive later
    #[error("Data not yet available: requested_id={requested_id}, current_max_id={current_max_id}, lag={}", .current_max_id - .requested_id)]
    DataNotYetAvailable {
        requested_id: u64,
        current_max_id: u64,
    },

    /// The requested data has been trimmed and is permanently lost
    /// This requires alerting and may indicate configuration issues
    #[error("Data trimmed: requested_id={requested_id}, min_available_id={min_available_id}, loss={}", .requested_id - .min_available_id)]
    DataTrimmed {
        requested_id: u64,
        min_available_id: u64,
    },

    /// Data corruption or gap detected in the sequence
    #[error("Data corrupted: expected_id={expected_id}, found_id={found_id}")]
    DataCorrupted { expected_id: u64, found_id: u64 },

    /// Storage layer error (RocksDB, serialization, etc)
    #[error("Storage error: {0}")]
    StorageError(#[from] anyhow::Error),

    /// Invalid operation (e.g., trim beyond max_id)
    #[error("Invalid operation: {0}")]
    InvalidOperation(String),

    /// Buffer is full and cannot accept more data
    #[error("Buffer full: capacity={capacity}, size={size}")]
    BufferFull { capacity: usize, size: usize },

    /// Serialization/deserialization error
    #[error("Serialization error: {0}")]
    SerializationError(String),

    /// Timeout waiting for data
    #[error("Timeout waiting for data: requested_id={0}")]
    Timeout(u64),
}

impl MsgBufError {
    /// Returns true if this error indicates data loss (permanent)
    pub fn is_data_loss(&self) -> bool {
        matches!(self, MsgBufError::DataTrimmed { .. })
    }

    /// Returns true if this error is temporary and may recover
    pub fn is_temporary(&self) -> bool {
        matches!(
            self,
            MsgBufError::DataNotYetAvailable { .. } | MsgBufError::Timeout(_)
        )
    }

    /// Returns true if this error indicates corruption
    pub fn is_corruption(&self) -> bool {
        matches!(self, MsgBufError::DataCorrupted { .. })
    }
}

pub type Result<T> = std::result::Result<T, MsgBufError>;

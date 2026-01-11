// Re-export main types
pub mod buffer;
pub mod error;
pub mod metrics;
pub(crate) mod pbcode;
pub mod storage;
pub mod traits;
pub mod transport;

pub use buffer::{MsgBufReaderImpl, MsgBuffer, MsgBufferConfig};
pub use error::{MsgBufError, Result};
pub use metrics::MsgBufMetrics;
// ring_buffer removed: storage-only MsgBuffer
pub use storage::{PersistentStorage, StorageConfig};
pub use traits::{BufferStats, MsgBufReader, MsgBufSync, MsgBufWriter, OutputEvent};

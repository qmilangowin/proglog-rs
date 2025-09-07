// pub mod discovery;
// pub mod proto;
// pub mod server;
pub mod errors;
pub mod storage;

use crate::errors::*;

/// Type alias for Results in this crate
pub type ProglogResult<T> = Result<T, ProglogError>;
pub type StorageResult<T> = Result<T, StorageError>;
pub type IndexResult<T> = Result<T, IndexError>;
pub type SegmentResult<T> = Result<T, SegmentError>;
pub type LogResult<T> = Result<T, LogError>;

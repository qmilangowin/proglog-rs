use crate::errors::{StorageError, StorageResult};
use std::io;
pub mod store;

pub trait StorageContext<T> {
    fn with_open_context(self, path: &str) -> StorageResult<T>;
    fn with_write_context(self, position: u64) -> StorageResult<T>;
    fn with_grow_context(self, current: u64, target: u64) -> StorageResult<T>;
    fn with_read_context(self, position: u64) -> StorageResult<T>;
    fn with_mmap_context(self, size: u64) -> StorageResult<T>;
}

impl<T> StorageContext<T> for Result<T, io::Error> {
    fn with_open_context(self, path: &str) -> StorageResult<T> {
        self.map_err(|source| StorageError::OpenFailed {
            path: path.to_string(),
            source,
        })
    }

    fn with_write_context(self, position: u64) -> StorageResult<T> {
        self.map_err(|source| StorageError::WriteFailed { position, source })
    }

    fn with_grow_context(self, current: u64, target: u64) -> StorageResult<T> {
        self.map_err(|source| StorageError::GrowFailed {
            current_size: current,
            target_size: target,
            source,
        })
    }

    fn with_read_context(self, position: u64) -> StorageResult<T> {
        self.map_err(|source| StorageError::ReadFailed { position, source })
    }

    fn with_mmap_context(self, size: u64) -> StorageResult<T> {
        self.map_err(|source| StorageError::MmapFailed { size, source })
    }
}

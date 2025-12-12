use crate::errors::IndexError;
use crate::errors::StorageError;
use crate::{IndexResult, StorageResult};
use std::io;
pub mod index;
pub mod log;
pub mod segment;
pub mod store;
pub mod traits;


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

pub trait IndexContext<T> {
    fn with_open_context(self, path: &str) -> IndexResult<T>;
    fn with_write_context(self, position: u64) -> IndexResult<T>;
    fn with_grow_context(self, current: u64, target: u64) -> IndexResult<T>;
    fn with_mmap_context(self, size: u64) -> IndexResult<T>;
}

impl<T> IndexContext<T> for Result<T, io::Error> {
    fn with_open_context(self, path: &str) -> IndexResult<T> {
        self.map_err(|source| IndexError::OpenFailed {
            path: path.to_string(),
            source,
        })
    }

    fn with_write_context(self, position: u64) -> IndexResult<T> {
        self.map_err(|source| IndexError::WriteFailed { position, source })
    }

    fn with_grow_context(self, current: u64, target: u64) -> IndexResult<T> {
        self.map_err(|source| IndexError::GrowFailed {
            current_size: current,
            target_size: target,
            source,
        })
    }

    fn with_mmap_context(self, size: u64) -> IndexResult<T> {
        self.map_err(|source| IndexError::MmapFailed { size, source })
    }
}

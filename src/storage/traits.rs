use std::path::Path;

/// StorageBackend is a trait that allows to use different backends for storage
pub trait StorageBackend {
    type Error: std::error::Error + Send + Sync + 'static;

    /// Append data and return (position, bytes_written)
    fn append(&mut self, data: &[u8]) -> Result<(u64, u64), Self::Error>;

    /// Read data at position and return (data, bytes_read)
    fn read(&self, position: u64) -> Result<(Vec<u8>, u64), Self::Error>;

    /// Return current size in bytes
    fn size(&self) -> u64;

    /// Flush any pending writes to ensure durability
    fn flush(&mut self) -> Result<(), Self::Error>;
}

/// Trait for different cleanup strategies (local filesystem, cloud storage, etc.)
pub trait StorageCleanup {
    type Error: std::error::Error + Send + Sync + 'static;

    fn delete_file(&self, path: &Path) -> Result<(), Self::Error>;

    fn cleanup_segment(&self, store_path: &Path, index_path: &Path) -> Result<(), Self::Error> {
        self.delete_file(store_path)?;
        self.delete_file(index_path)?;
        Ok(())
    }

    fn cleanup_log_directory(&self, _log_dir: &Path) -> Result<(), Self::Error> {
        Ok(())
    }
}

pub struct LocalFileSystem;

impl StorageCleanup for LocalFileSystem {
    type Error = std::io::Error;

    fn delete_file(&self, path: &Path) -> Result<(), Self::Error> {
        std::fs::remove_file(path)
    }

    fn cleanup_log_directory(&self, log_dir: &Path) -> Result<(), Self::Error> {
        if log_dir.exists() && std::fs::read_dir(log_dir)?.next().is_none() {
            std::fs::remove_dir(log_dir)?;
        }
        Ok(())
    }
}

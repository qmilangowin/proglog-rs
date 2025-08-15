//! The index file speeds up reads. It maps record offsets to the position in the store file.
//! As such, reading a record is a two-step process: first - get the entry from the index file for the record which tell you
//! the position of the record in store file, and then read the record at that position.

#![allow(dead_code)] //TODO: remove this when done with implemenation. Only adding for clippy CI to pass
use crate::IndexResult;
use crate::errors::IndexError;
use crate::storage::IndexContext;
use memmap2::{MmapMut, MmapOptions};
use std::fs::{File, OpenOptions};
use std::path::Path;
use tracing::{debug, info, instrument, warn};

// Each index entry: 8 bytes offset + 8 bytes position = 16 bytes
const OFFSET_WIDTH: u64 = 8;
const POSITION_WIDTH: u64 = 8;
const ENTRY_WIDTH: u64 = 16; // OFFSET_WIDTH + POSITION_WIDTH

/// Index provides fast lookups from log offsets/indexes to positions in the Store.
/// Each entry maps a sequential offset to a byt position in the Store file.
///
/// Format: [8-byte offset][8-byte position][8-byte offset][8-byte position] etc.
/// Entry 0: [8-byte offset][8-byte position] = bytes 0-15 where the offset denotes the log-record count.
/// Entry 1: [8-byte offset][8-byte position] = bytes 16-31  
/// Entry 2: [8-byte offset][8-byte position] = bytes 32-47
pub struct Index {
    file: File,
    mmap: MmapMut,
    size: u64, // number of entries (not bytes)
}

impl Index {
    #[instrument(skip_all, fields(path = ?path.as_ref()))]
    /// Create a new index from the given file path.
    /// If the file doesn't exist, create it
    pub fn new(path: impl AsRef<Path>) -> IndexResult<Self> {
        debug!("Opening index file");

        let path_str = path.as_ref().to_string_lossy();

        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(path.as_ref())
            .with_open_context(&path_str)?;

        let mut file_len = file.metadata().with_open_context(&path_str)?.len();

        debug!(existing_size = file_len, "Index file opened");

        // Validate the file size, must be a multiple of ENTRY_WIDTH
        if file_len % ENTRY_WIDTH != 0 {
            warn!(
                file_size = file_len,
                entry_width = ENTRY_WIDTH,
                "Index file size is not a multiple of entry size - truncating"
            );

            let valid_size = (file_len / ENTRY_WIDTH) * ENTRY_WIDTH;
            file.set_len(valid_size)
                .map_err(|e| IndexError::CorruptedFile {
                    reason: format!("Failed to truncate corrupted index file: {e}"),
                })?;

            debug!(
                original_size = file_len,
                truncated_size = valid_size,
                "Index file truncated to valid size"
            );

            file_len = valid_size;
        }

        // Ensure file has at least some size for memory mapping
        let initial_size = if file_len == 0 {
            let new_size = 1000 * ENTRY_WIDTH;
            file.set_len(new_size).with_grow_context(0, new_size)?;
            file.sync_all().with_grow_context(0, new_size)?;
            new_size
        } else {
            std::cmp::max(file_len, 1000 * ENTRY_WIDTH)
        };

        // create the memmap file for index
        let mmap = unsafe {
            MmapOptions::new()
                .len(initial_size as usize)
                .map_mut(&file)
                .with_mmap_context(initial_size)?
        };

        let num_entries = file_len / ENTRY_WIDTH;

        info!(
            file_size = file_len,
            map_size = initial_size,
            num_entries = num_entries,
            "Index created successfully"
        );

        Ok(Index {
            file,
            mmap,
            size: num_entries,
        })
    }

    /// Return the number of entries in the index
    pub fn len(&self) -> u64 {
        self.size
    }

    pub fn is_empty(&self) -> bool {
        self.size == 0
    }

    /// Return file size in bytes
    pub fn size(&self) -> u64 {
        self.size * ENTRY_WIDTH
    }

    /// Writes an entry mapping offset to the position in the store
    #[instrument(skip(self), fields(offset, position))]
    pub fn write(&mut self, offset: u64, position: u64) -> IndexResult<()> {
        debug!(offset, position, "Writing index entry");

        // Check if we need to grow the memory map
        let entry_start = self.size * ENTRY_WIDTH;
        if entry_start + ENTRY_WIDTH > self.mmap.len() as u64 {
            debug!(
                current_entries = self.size,
                needed_bytes = entry_start + ENTRY_WIDTH,
                mmap_len = self.mmap.len(),
                "Need to grow index"
            );
            self.grow()?
        };

        let entry_pos = (self.size * ENTRY_WIDTH) as usize;

        // write offset (8 bytes)
        let offset_bytes = offset.to_le_bytes();
        self.mmap[entry_pos..entry_pos + OFFSET_WIDTH as usize].copy_from_slice(&offset_bytes);

        //write position (8 bytes)
        let position_bytes = position.to_le_bytes();
        let pos_start = entry_pos + OFFSET_WIDTH as usize;
        self.mmap[pos_start..pos_start + POSITION_WIDTH as usize].copy_from_slice(&position_bytes);

        // Flush to ensure durability
        self.mmap.flush().map_err(|e| IndexError::WriteFailed {
            position: offset,
            source: e,
        })?;

        // Increment size after successful write
        self.size += 1;

        info!(
            offset,
            position,
            entry_index = self.size - 1,
            total_entries = self.size,
            "Index written successfully"
        );

        Ok(())
    }

    /// Reads the position for a given offset using linear search
    /// Note: We use linear search because entries are stored in order of arrival, not sorted by offset
    #[instrument(skip(self), fields(offset))]
    pub fn read(&self, offset: u64) -> IndexResult<u64> {
        debug!(
            offset,
            total_entries = self.size,
            "Reading position for offset"
        );

        if self.size == 0 {
            return Err(IndexError::OffsetNotFound { offset });
        }

        // We can use linear search here. Not super optimal but we can change it later if needed.
        // to a sorted segment with binary search. Used by Kafka for example and is the distributed long standard.
        for index in 0..self.size {
            let entry_offset = self.read_offset_at_index(index)?;
            if entry_offset == offset {
                let position = self.read_position_at_index(index)?;
                debug!(
                    offset,
                    position,
                    entry_index = index,
                    "Found offset in index"
                );
                return Ok(position);
            }
        }

        warn!(offset, "Offset not found at index");
        Err(IndexError::OffsetNotFound { offset })
    }

    /// Helper: Read offset at a specific entry index
    fn read_offset_at_index(&self, index: u64) -> IndexResult<u64> {
        if index >= self.size {
            return Err(IndexError::CorruptedEntry { position: index });
        }

        let entry_pos = (index * ENTRY_WIDTH) as usize;
        let offset_bytes = &self.mmap[entry_pos..entry_pos + OFFSET_WIDTH as usize];

        let offset = u64::from_le_bytes(
            offset_bytes
                .try_into()
                .map_err(|_| IndexError::CorruptedEntry { position: index })?,
        );
        Ok(offset)
    }

    /// Helper: Read position at a specific entry index
    fn read_position_at_index(&self, index: u64) -> IndexResult<u64> {
        if index >= self.size {
            return Err(IndexError::CorruptedEntry { position: index });
        }

        let entry_pos = (index * ENTRY_WIDTH) as usize;
        let pos_start = entry_pos + OFFSET_WIDTH as usize;
        let position_bytes = &self.mmap[pos_start..pos_start + POSITION_WIDTH as usize];

        let position = u64::from_le_bytes(
            position_bytes
                .try_into()
                .map_err(|_| IndexError::CorruptedEntry { position: index })?,
        );

        Ok(position)
    }

    /// Grows the memory map to accommodate more entries
    #[instrument(skip(self))]
    fn grow(&mut self) -> IndexResult<()> {
        let current_capacity = self.mmap.len() as u64;
        let new_capacity =
            std::cmp::max(current_capacity * 2, current_capacity + 1000 * ENTRY_WIDTH); //add capacity for 1000 more entries

        info!(current_capacity, new_capacity, "Growing index capacity");

        // extend the file
        self.file
            .set_len(new_capacity)
            .map_err(|e| IndexError::GrowFailed {
                current_size: current_capacity,
                target_size: new_capacity,
                source: e,
            })?;

        self.file.sync_all().map_err(|e| IndexError::GrowFailed {
            current_size: current_capacity,
            target_size: new_capacity,
            source: e,
        })?;

        //Remap our mmap
        self.mmap = unsafe {
            MmapOptions::new()
                .len(new_capacity as usize)
                .map_mut(&self.file)
                .map_err(|e| IndexError::MmapFailed {
                    size: new_capacity,
                    source: e,
                })?
        };

        info!("Index capacity extended successfully");
        Ok(())
    }
}

impl Drop for Index {
    fn drop(&mut self) {
        let _ = self.mmap.flush();
        let _ = self.file.set_len(self.size());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Once;
    use tempfile::NamedTempFile;
    use tracing_subscriber::{EnvFilter, fmt};

    static INIT_TRACING: Once = Once::new();

    fn init_tracing() {
        INIT_TRACING.call_once(|| {
            let _ = fmt()
                .with_env_filter(
                    EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("debug")),
                )
                .with_test_writer()
                .try_init();
        });
    }

    #[test]
    fn test_index_write_reaad() -> IndexResult<()> {
        init_tracing();
        let temp_file = NamedTempFile::new().unwrap();
        let mut index = Index::new(temp_file.path())?;

        // write a single entry
        index.write(0, 100)?;

        let position = index.read(0)?;
        assert_eq!(position, 100);
        assert_eq!(index.len(), 1);

        Ok(())
    }

    #[test]
    fn test_index_multiple_entries() -> IndexResult<()> {
        init_tracing();
        let temp_file = NamedTempFile::new().unwrap();
        let mut index = Index::new(temp_file.path())?;

        // Write multiple entries in order
        let entries = [(0, 0), (1, 150), (2, 300), (3, 500)];

        for (offset, position) in entries {
            index.write(offset, position)?;
        }

        // Read them all back
        for (offset, expected_position) in entries {
            let position = index.read(offset)?;
            assert_eq!(position, expected_position);
        }

        assert_eq!(index.len(), 4);
        Ok(())
    }

    #[test]
    fn test_index_out_of_order_writes() -> IndexResult<()> {
        init_tracing();
        let temp_file = NamedTempFile::new().unwrap();
        let mut index = Index::new(temp_file.path())?;

        // Write entries out of order (simulating distributed arrival)
        index.write(5, 500)?; // 6th record arrives first
        index.write(1, 100)?; // 2nd record arrives second  
        index.write(3, 300)?; // 4th record arrives third

        // Should still be able to find them
        assert_eq!(index.read(5)?, 500);
        assert_eq!(index.read(1)?, 100);
        assert_eq!(index.read(3)?, 300);

        // Non-existent offset should fail
        assert!(matches!(
            index.read(2),
            Err(IndexError::OffsetNotFound { offset: 2 })
        ));
        assert!(matches!(
            index.read(4),
            Err(IndexError::OffsetNotFound { offset: 4 })
        ));

        assert_eq!(index.len(), 3);
        Ok(())
    }

    #[test]
    fn test_index_persistence() -> IndexResult<()> {
        init_tracing();
        let temp_file = NamedTempFile::new().unwrap();
        let path = temp_file.path().to_owned();

        // Write some entries and close the index
        {
            let mut index = Index::new(&path)?;
            index.write(0, 100)?;
            index.write(1, 200)?;
            index.write(2, 300)?;
        } // Index drops here, should flush to disk

        // Reopen and verify persistence
        {
            let index = Index::new(&path)?;
            assert_eq!(index.len(), 3);
            assert_eq!(index.read(0)?, 100);
            assert_eq!(index.read(1)?, 200);
            assert_eq!(index.read(2)?, 300);
        }

        Ok(())
    }

    #[test]
    fn test_index_empty_operations() -> IndexResult<()> {
        init_tracing();
        let temp_file = NamedTempFile::new().unwrap();
        let index = Index::new(temp_file.path())?;

        // Empty index should report correct state
        assert!(index.is_empty());
        assert_eq!(index.len(), 0);
        assert_eq!(index.size(), 0);

        // Reading from empty index should fail
        assert!(matches!(
            index.read(0),
            Err(IndexError::OffsetNotFound { offset: 0 })
        ));

        Ok(())
    }
}

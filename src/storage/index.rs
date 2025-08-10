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
const ENTRY_WIDTH: u64 = 16; // OFFSET_WIDTH + ENTRY_WIDTH

/// Index provides fast lookups from log offsets to positions in the Store.
/// Each entry maps a sequential offset to a byt position in the Store file.
///
/// Format: [8-byte offset][8-byte position][8-byte offset][8-byte position] etc.
pub struct Index {
    file: File,
    mmap: MmapMut,
    size: u64, // number of entries (not bytes)
}

impl Index {
    #[instrument(skip_all, fields(path = ?path.as_ref()))]
    /// Create a new inxed from the given file path.
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
                    reason: format!("Filed to truncate corrupted index file: {e}"),
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

        let num_entires = file_len / ENTRY_WIDTH;

        info!(
            file_size = file_len,
            map_size = initial_size,
            num_entires = num_entires,
            "Index created successfully"
        );

        Ok(Index {
            file,
            mmap,
            size: num_entires,
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
}

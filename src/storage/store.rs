use anyhow::{Context, Result};
use memmap2::{MmapMut, MmapOptions};
use std::fs::{File, OpenOptions};
use std::io::{Seek, SeekFrom, Write};
use std::path::Path;
use tracing::{debug, info, instrument, subscriber, warn};

// the length of each record is stored as u64 (8 bytes) before each record
const LEN_WIDTH: u64 = 8;

/// Store represents an append-only file that holds the actual log records.
/// Each record is prefixed with its lengnth for efficiency.
///
/// Format: [8-byte length][record data][8-byte length][record data]
pub struct Store {
    file: File,
    mmap: MmapMut,
    size: u64,
}

impl Store {
    #[instrument(skip_all, fields(path = ?path.as_ref()))]
    /// Creates a new store from the given file path.
    /// If the file doesn't exist, it will be created.
    pub fn new(path: impl AsRef<Path>) -> Result<Self> {
        debug!("Opening store file");

        let file = OpenOptions::new()
            .read(true)
            .create(true)
            .append(true)
            .open(path.as_ref())
            .with_context(|| format!("Failed to open store file: {:?}", path.as_ref()))?;

        let file_len = file.metadata()?.len();
        debug!(existing_size = file_len, "File opened");

        // ensure file has at least some size for memory mapping.
        let initial_size = if file_len == 0 { 1024 * 1024 } else { file_len };

        let mut file_for_resize = file.try_clone()?;
        file_for_resize.seek(SeekFrom::Start(initial_size - 1))?;
        file_for_resize.write_all(&[0])?;
        file_for_resize.sync_all()?;

        let mmap = unsafe {
            MmapOptions::new()
                .len(initial_size as usize)
                .map_mut(&file)?
        };

        info!(
            data_size = file_len,
            map_size = initial_size,
            "Stored created successfully"
        );

        Ok(Store {
            file,
            mmap,
            size: file_len,
        })
    }

    /// Appends a record to the store and returns its position and number of bytes written.
    ///
    /// Returns: (position_where_record_starts, total_bytes_written)
    #[instrument(skip(self, data), fields(data_len = data.len()))]
    pub fn append(&mut self, data: &[u8]) -> Result<(u64, u64)> {
        debug!("Appending record to the store");

        let record_len = data.len() as u64;
        let total_len = LEN_WIDTH + record_len;

        // Check if we need to grow memory map
        if self.size + total_len > self.mmap.len() as u64 {
            debug!(
                current_size = self.size,
                needed = total_len,
                mmap_len = self.mmap.len(),
                "Need to grow store"
            );
            self.grow(total_len)?; //TODO: implement
        }

        let pos = self.size;

        // Write length prefix
        let len_bytes = record_len.to_le_bytes();
        self.mmap[self.size as usize..(self.size + LEN_WIDTH) as usize].copy_from_slice(&len_bytes);
        self.size += LEN_WIDTH;

        // Write the actual record data
        self.mmap[self.size as usize..(self.size + record_len) as usize].copy_from_slice(data);
        self.size += record_len;

        //Flush the mmap to ensure durability and contents written to disk
        self.mmap.flush()?;

        info!(
            postion = pos,
            bytes_written = total_len,
            new_size = self.size,
            "Record appended successfully"
        );

        Ok((pos, total_len))
    }

    pub fn grow(&self, _total_len: u64) -> Result<Self> {
        todo!()
    }
}

use crate::StorageResult;
use crate::errors::StorageError;
use crate::storage::StorageContext;
use memmap2::{MmapMut, MmapOptions};
use std::fs::{File, OpenOptions};
use std::path::Path;
use tracing::{debug, info, instrument, warn};

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
    pub fn new(path: impl AsRef<Path>) -> StorageResult<Self> {
        debug!("Opening store file");

        let path_str = path.as_ref().to_string_lossy();

        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(path.as_ref())
            .with_open_context(&path_str)?;

        let file_len = file.metadata().with_open_context(&path_str)?.len();

        debug!(existing_size = file_len, "File opened");

        // check if file is corrupted
        let actual_data_size = if file_len > 0 {
            Self::scan_and_repair(&file, file_len, &path_str)?
        } else {
            0
        };

        debug!(
            original_size = file_len,
            repaired_size = actual_data_size,
            "Recovery scan completed"
        );

        // ensure file has at least some size for memory mapping.
        let initial_size = if file_len == 0 {
            // New file - start with 1MB
            let new_size = 1024 * 1024;
            let file_for_resize = file.try_clone().with_open_context(&path_str)?;
            file_for_resize
                .set_len(new_size)
                .with_grow_context(file_len, new_size)?;
            file_for_resize
                .sync_all()
                .with_grow_context(file_len, new_size)?;
            new_size
        } else {
            // Existing file - use current size, it's already been truncated to the right size
            std::cmp::max(actual_data_size, 1024 * 1024)
        };

        let mmap = unsafe {
            MmapOptions::new()
                .len(initial_size as usize)
                .map_mut(&file)
                .with_mmap_context(initial_size)?
        };

        info!(
            data_size = file_len,
            map_size = initial_size,
            "Stored created successfully"
        );

        Ok(Store {
            file,
            mmap,
            size: actual_data_size,
        })
    }

    /// Appends a record to the store and returns its position and number of bytes written.
    ///
    /// Returns: (position_where_record_starts, total_bytes_written)
    #[instrument(skip(self, data), fields(data_len = data.len()))]
    pub fn append(&mut self, data: &[u8]) -> StorageResult<(u64, u64)> {
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
            self.grow(total_len)?;
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
        self.mmap.flush().with_write_context(pos)?;

        info!(
            postion = pos,
            bytes_written = total_len,
            new_size = self.size,
            "Record appended successfully"
        );

        Ok((pos, total_len))
    }

    /// Reads a record at the given position
    /// Returns the record data and the total bytes read (including length prefix)
    #[instrument(skip(self), fields(pos))]
    pub fn read(&self, pos: u64) -> StorageResult<(Vec<u8>, u64)> {
        debug!(
            position = pos,
            store_size = self.size,
            "Reading record from store"
        );

        if pos >= self.size {
            warn!(
                position = pos,
                store_size = self.size,
                "Read size beyond store size"
            );
            return Err(StorageError::ReadBeyondEnd {
                position: pos,
                size: self.size,
            });
        }

        // Read the length prefix
        if pos + LEN_WIDTH > self.size {
            warn!(position = pos, "Not enough data to read length prefix");
            return Err(StorageError::CorruptedRecord {
                position: pos,
                reason: "Not enough data to read length prefix".to_string(),
            });
        }

        let len_bytes = &self.mmap[pos as usize..(pos + LEN_WIDTH) as usize];
        let record_len = u64::from_le_bytes(len_bytes.try_into().map_err(|_| {
            StorageError::CorruptedRecord {
                position: pos,
                reason: "Invalid length bytes".to_string(),
            }
        })?);
        debug!(record_length = record_len, "Read record length");

        // Read the record length
        let data_start = pos + LEN_WIDTH;
        let data_end = data_start + record_len;

        if data_end > self.size {
            warn!(
                record_len = record_len,
                data_end = data_end,
                store_size = self.size,
                "Record extends beyond store size"
            );
            return Err(StorageError::CorruptedRecord {
                position: pos,
                reason: format!("Record length {record_len} extends beyond store size"),
            });
        }

        let data = self.mmap[data_start as usize..data_end as usize].to_vec();

        debug!(
            bytes_read = LEN_WIDTH + record_len,
            data_size = data.len(),
            "Record read successfully"
        );

        Ok((data, LEN_WIDTH + record_len))
    }

    /// Returns the current size of the store (in other words: amount of data written)
    pub fn size(&self) -> u64 {
        self.size
    }

    /// Grows the memory map to accomodate more data
    #[instrument(skip(self))]
    pub fn grow(&mut self, needed: u64) -> StorageResult<()> {
        let current_capacity = self.mmap.len() as u64;
        let new_capacity = std::cmp::max(current_capacity * 2, self.size + needed + 1024 * 1024); // add 1mb extra buffer to our target

        info!(
            current_capacity,
            new_capacity, needed, "Growing store capacity"
        );

        // Extend the file to what we need
        let file_for_resize = self
            .file
            .try_clone()
            .with_grow_context(current_capacity, new_capacity)?;
        file_for_resize
            .set_len(new_capacity)
            .with_grow_context(current_capacity, new_capacity)?;
        file_for_resize
            .sync_all()
            .with_grow_context(current_capacity, new_capacity)?;

        self.mmap = unsafe {
            MmapOptions::new()
                .len(new_capacity as usize)
                .map_mut(&self.file)
                .with_mmap_context(new_capacity)?
        };

        info!("Store capacity grown successfully");
        Ok(())
    }

    #[instrument(skip(file))]
    fn scan_and_repair(file: &File, file_len: u64, path: &str) -> StorageResult<u64> {
        if file_len == 0 {
            return Ok(0);
        }

        info!(file_len, "Starting recovery scan for torn records");

        // Memory map the file for scanning
        let mmap = unsafe {
            MmapOptions::new()
                .len(file_len as usize)
                .map(file)
                .with_mmap_context(file_len)?
        };

        let mut pos = 0u64;
        let mut last_valid_pos = 0u64;

        while pos < file_len {
            // Check if we have enough bytes for a length prefix
            // c
            if pos + LEN_WIDTH > file_len {
                warn!(
                    position = pos,
                    file_len = file_len,
                    "Incomplete length prefix at end of file - truncating"
                );
                break;
            }

            // Read the length prefix
            let len_bytes = &mmap[pos as usize..(pos + LEN_WIDTH) as usize];
            let record_len = u64::from_le_bytes(len_bytes.try_into().map_err(|_| {
                StorageError::CorruptedRecord {
                    position: pos,
                    reason: "Invalid length bytes during recovery".to_string(),
                }
            })?);

            debug!(position = pos, record_len, "Found record during scan");

            // Check if record length is reasonable (prevent runaway reads)
            if record_len > 100 * 1024 * 1024 {
                // 100MB max record size
                warn!(
                    position = pos,
                    record_len = record_len,
                    "Suspiciously large record length - treating as corruption"
                );
                break;
            }

            // Check if we have enough bytes for the full record
            let record_end = pos + LEN_WIDTH + record_len;
            if record_end > file_len {
                warn!(
                    position = pos,
                    record_len = record_len,
                    record_end = record_end,
                    file_len = file_len,
                    "Incomplete record data - truncating"
                );
                break;
            }

            // Record is complete - move to next
            last_valid_pos = record_end;
            pos = record_end;
        }

        // If we found any torn records, truncate the file
        if last_valid_pos < file_len {
            warn!(
                original_size = file_len,
                truncated_size = last_valid_pos,
                "Truncating file to remove torn records"
            );

            file.set_len(last_valid_pos).with_open_context(path)?;
            file.sync_all().with_open_context(path)?;

            info!(
                recovered_size = last_valid_pos,
                removed_bytes = file_len - last_valid_pos,
                "Recovery scan completed - file repaired"
            );
        } else {
            info!(
                file_size = file_len,
                "Recovery scan completed - no torn records found"
            );
        }

        Ok(last_valid_pos)
    }
}

impl Drop for Store {
    fn drop(&mut self) {
        // flush all data before dropping
        let _ = self.mmap.flush();
        // truncate file to actual size to avoid sparse files
        let _ = self.file.set_len(self.size);
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
    fn test_store_append_and_read() -> StorageResult<()> {
        init_tracing();
        let temp_file = NamedTempFile::new().unwrap();
        let mut store = Store::new(temp_file.path())?;

        // Test appending and reading a single record
        let data = b"Hello, World";
        let (pos, written) = store.append(data)?;

        // our record should look like this after the first append
        // | Offset | Bytes                                    | Meaning         |
        // |--------|------------------------------------------|-----------------|
        // | 0–7    | 0C 00 00 00 00 00 00 00                  | Length = 12     |
        // | 8–19   | 48 65 6C 6C 6F 2C 20 57 6F 72 6C 64      | "Hello, World"  |

        assert_eq!(pos, 0); // First record starts at position 0
        assert_eq!(written, 8 + data.len() as u64); //8 bytes length info + data

        let (read_data, read_bytes) = store.read(pos)?;
        assert_eq!(read_data, data);
        assert_eq!(read_bytes, written);
        Ok(())
    }

    #[test]
    fn test_store_multiple_records() -> StorageResult<()> {
        init_tracing();
        let temp_file = NamedTempFile::new().unwrap();
        let mut store = Store::new(temp_file.path())?;

        let records = [
            b"First record".as_slice(), //we can get type coercion by creating the first, then the rest are coerced to &[u8]
            b"Second record",
            b"Third record with more data",
        ];

        let mut positions = Vec::new();

        //Append all the records
        for record in records {
            let (pos, _) = store.append(record)?;
            positions.push(pos);
        }

        //Read all the positions back
        for (i, &pos) in positions.iter().enumerate() {
            let (data, _) = store.read(pos)?;
            assert_eq!(data, records[i]);
        }

        Ok(())
    }

    #[test]
    fn test_store_persistance() -> StorageResult<()> {
        init_tracing();
        let temp_file = NamedTempFile::new().unwrap();
        let path = temp_file.path().to_owned();

        let data = b"Persistent data";
        let pos;

        //Write data and close the store and call our drop implemenation.
        {
            let mut store = Store::new(&path)?;
            let (p, _) = store.append(data)?;
            pos = p;
        }

        //Reopen and check persistence
        {
            let store = Store::new(&path)?;
            let (read_data, _) = store.read(pos)?;
            assert_eq!(read_data, data);
        }

        Ok(())
    }

    #[test]
    fn test_read_beyond_end() {
        init_tracing();
        let temp_file = NamedTempFile::new().unwrap();
        let store = Store::new(temp_file.path()).unwrap();

        //try to read beyond the end

        let result = store.read(100);
        assert!(matches!(
            result,
            Err(StorageError::ReadBeyondEnd {
                position: 100,
                size: 0
            })
        ));
    }

    #[test]
    fn test_error_recovery_info() {
        init_tracing();
        let temp_file = NamedTempFile::new().unwrap();
        let store = Store::new(temp_file.path()).unwrap();

        let result = store.read(100);
        if let Err(err) = result {
            assert!(!err.is_recoverable());
        }
    }

    #[test]
    fn test_recovery_scan_torn_record() -> StorageResult<()> {
        init_tracing();
        let temp_file = NamedTempFile::new().unwrap();
        let path = temp_file.path().to_owned();

        // Write some valid records
        {
            let mut store = Store::new(&path)?;
            store.append(b"First store record")?;
            store.append(b"Second store record")?;
        }

        // manually create a corrupted entry which will simulate a crash during entry
        {
            use std::io::{Seek, SeekFrom, Write};

            let mut file = OpenOptions::new()
                .write(true)
                .truncate(false)
                .open(&path)
                .unwrap();

            file.seek(SeekFrom::End(0)).unwrap();

            // write a fake length header
            let fake_len: u64 = 50;
            file.write_all(&fake_len.to_le_bytes()).unwrap();

            // write only 10 bytes to simulate crash. length and actual data will be inconsistent
            file.write_all(b"i crashed!").unwrap();
            file.sync_all().unwrap();
        }

        // Reopen - recovery should detect and fix the corrupted record

        {
            let store = Store::new(&path)?;
            let (data1, _) = store.read(0)?;
            assert_eq!(data1, b"First store record");

            // second record should be at 8 bytes len + 18 bytes data = 26
            let (data2, _) = store.read(26)?;
            assert_eq!(data2, b"Second store record");

            //Total valid size should be: first record (26 bytes) + second record (27 bytes)
            let result = store.read(53);
            assert!(matches!(result, Err(StorageError::ReadBeyondEnd { .. })))
        }

        Ok(())
    }

    #[test]
    fn test_variable_sized_records() -> StorageResult<()> {
        init_tracing();
        let temp_file = NamedTempFile::new().unwrap();
        let mut store = Store::new(temp_file.path())?;

        let records = [
            b"".as_slice(),
            b"x",
            b"short",
            b"medium length record",
            &[b'a'; 100],
            &[b'b'; 1000],
            &[b'c'; 10000],
        ];

        let mut positions = Vec::new();

        for record in &records {
            let (pos, written) = store.append(record)?;
            positions.push(pos);
            assert_eq!(written, 8 + record.len() as u64);
        }

        for (i, &pos) in positions.iter().enumerate() {
            let (data, bytes_read) = store.read(pos)?;
            assert_eq!(data, records[i]);
            assert_eq!(bytes_read, 8 + records[i].len() as u64);
        }

        Ok(())
    }

    #[test]
    fn test_store_growth_with_many_records() -> StorageResult<()> {
        init_tracing();
        let temp_file = NamedTempFile::new().unwrap();
        let mut store = Store::new(temp_file.path())?;

        let num_records = 1000;
        let record = b"Test record for growth";
        let mut positions = Vec::new();

        for _ in 0..num_records {
            let (pos, _) = store.append(record)?;
            positions.push(pos);
        }

        assert_eq!(positions.len(), num_records);

        let expected_size = num_records * (8 + record.len());
        assert_eq!(store.size() as usize, expected_size);

        for (i, &pos) in positions.iter().enumerate().step_by(100) {
            let (data, _) = store.read(pos)?;
            assert_eq!(data, record, "Failed at position {}", i);
        }

        Ok(())
    }
}

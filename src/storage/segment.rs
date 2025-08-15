//! Segment combines the Store and Index to provide a logical log segment
//! Each segment handles a contiguous range of offsets and manages the coordination between storing data and indexing it.
use crate::SegmentResult;
use crate::errors::SegmentError;
use crate::storage::index::Index;
use crate::storage::store::Store;
use std::path::Path;
use tracing::{debug, info, instrument};

pub struct Segment {
    store: Store,
    index: Index,
    base_offset: u64, // First offset in this segment
    next_offset: u64,
    max_store_bytes: u64,
    max_index_entries: u64,
}

impl Segment {
    #[instrument(skip_all, fields(base_offset))]
    pub fn new(
        store_path: impl AsRef<Path>,
        index_path: impl AsRef<Path>,
        base_offset: u64,
        max_store_bytes: u64,
        max_index_entries: u64,
    ) -> SegmentResult<Self> {
        debug!(base_offset, "Creating a new segment");

        let store = Store::new(store_path)?;
        let index = Index::new(index_path)?;

        // determine next offset based on existing index entries
        let next_offset = if index.is_empty() {
            base_offset
        } else {
            // find the highest offset in the index + 1
            let mut highest_offset = base_offset;
            for i in 0..index.len() {
                let offset = index.read_offset_at_index(i)?;
                if offset > highest_offset {
                    highest_offset = offset;
                }
            }
            highest_offset + 1
        };
        info!(
            base_offset,
            next_offset,
            store_size = store.size(),
            index_entries = index.len(),
            "Segment created successfully"
        );

        Ok(Segment {
            store,
            index,
            base_offset,
            next_offset,
            max_store_bytes,
            max_index_entries,
        })
    }

    /// Appends data to the segment and returns the assigned offset
    #[instrument(skip(self, data), fields(data_len = data.len()))]
    pub fn append(&mut self, data: &[u8]) -> SegmentResult<u64> {
        if self.is_full() {
            return Err(SegmentError::SegmentFull {
                base_offset: self.base_offset,
                max_size: self.max_store_bytes,
                current_size: self.store.size(),
            });
        }

        let offset = self.next_offset;

        debug!(offset, "Appending record to segment");

        // write to store first
        let (position, _) = self.store.append(data)?;

        // record it in the index
        self.index.write(offset, position)?;

        self.next_offset += 1;

        info!(
            offset,
            position,
            segment_base = self.base_offset,
            "Record appended to segment"
        );

        Ok(offset)
    }

    /// Reads data for the given offset
    #[instrument(skip(self), fields(offset))]
    pub fn read(&self, offset: u64) -> SegmentResult<Vec<u8>> {
        debug!(
            offset,
            segment_base = self.base_offset,
            "Reading from segment"
        );

        if offset < self.base_offset || offset >= self.next_offset {
            return Err(SegmentError::OffsetOutOfRange {
                offset,
                base_offset: self.base_offset,
                next_offset: self.next_offset,
            });
        }

        let position = self.index.read(offset)?;

        //read the data from the store
        let (data, _) = self.store.read(position)?;

        debug!(
            offset,
            position,
            data_len = data.len(),
            "Successfully read from segment"
        );
        Ok(data)
    }

    /// Returns the base offset (first offset) of this segment
    pub fn base_offset(&self) -> u64 {
        self.base_offset
    }

    /// Returns the next offset that would be assigned
    pub fn next_offset(&self) -> u64 {
        self.next_offset
    }

    /// Returns true if the offset is within the segment's range
    pub fn contains_offset(&self, offset: u64) -> bool {
        offset >= self.base_offset && offset < self.next_offset
    }

    /// Returns true if the segment is full and should be rotated
    pub fn is_full(&self) -> bool {
        self.store.size() >= self.max_store_bytes || self.index.len() >= self.max_index_entries
    }

    /// Returns the current size of the store in bytes
    pub fn store_size(&self) -> u64 {
        self.store.size()
    }

    /// Returns the number of entries in the index
    pub fn index_entries(&self) -> u64 {
        self.index.len()
    }

    /// Returns true if the segment is empty
    pub fn is_empty(&self) -> bool {
        self.index.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Once;
    use tempfile::TempDir;
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
    fn test_segment_append_and_read() -> SegmentResult<()> {
        init_tracing();
        let temp_dir = TempDir::new().unwrap();
        let store_path = temp_dir.path().join("segment.log");
        let index_path = temp_dir.path().join("segment.idx");

        let mut segment = Segment::new(&store_path, &index_path, 0, 1024 * 1024, 1000)?;

        let data = b"Hello, Segment!";
        let offset = segment.append(data)?;

        assert_eq!(offset, 0);
        assert_eq!(segment.next_offset(), 1);
        assert!(!segment.is_empty());

        let read_data = segment.read(offset)?;
        assert_eq!(read_data, data);

        Ok(())
    }

    #[test]
    fn test_segment_sequential_offsets() -> SegmentResult<()> {
        init_tracing();
        let temp_dir = TempDir::new().unwrap();
        let store_path = temp_dir.path().join("segment.log");
        let index_path = temp_dir.path().join("segment.idx");

        let mut segment = Segment::new(
            &store_path,
            &index_path,
            100, // base_offset = 100
            1024 * 1024,
            1000,
        )?;

        let records = ["First", "Second", "Third"];
        let mut offsets = Vec::new();

        // Append multiple records
        for record in &records {
            let offset = segment.append(record.as_bytes())?;
            offsets.push(offset);
        }

        // Verify sequential offsets starting from base_offset
        assert_eq!(offsets, vec![100, 101, 102]);
        assert_eq!(segment.next_offset(), 103);

        // Read all records back
        for (i, &offset) in offsets.iter().enumerate() {
            let data = segment.read(offset)?;
            assert_eq!(data, records[i].as_bytes());
        }

        Ok(())
    }

    #[test]
    fn test_segment_offset_bounds_checking() -> SegmentResult<()> {
        init_tracing();
        let temp_dir = TempDir::new().unwrap();
        let store_path = temp_dir.path().join("segment.log");
        let index_path = temp_dir.path().join("segment.idx");

        let mut segment = Segment::new(&store_path, &index_path, 50, 1024 * 1024, 1000)?;

        // Add one record (gets offset 50)
        segment.append(b"test")?;

        // Test bounds checking
        assert!(segment.contains_offset(50)); // Valid
        assert!(!segment.contains_offset(49)); // Below base
        assert!(!segment.contains_offset(51)); // Beyond next

        // Reading out-of-range offsets should fail
        assert!(matches!(
            segment.read(49),
            Err(SegmentError::OffsetOutOfRange { offset: 49, .. })
        ));

        assert!(matches!(
            segment.read(51),
            Err(SegmentError::OffsetOutOfRange { offset: 51, .. })
        ));

        Ok(())
    }

    #[test]
    fn test_segment_full_detection() -> SegmentResult<()> {
        init_tracing();
        let temp_dir = TempDir::new().unwrap();
        let store_path = temp_dir.path().join("segment.log");
        let index_path = temp_dir.path().join("segment.idx");

        let mut segment = Segment::new(&store_path, &index_path, 0, 75, 10)?;

        assert!(!segment.is_full());

        // Fill up the segment (each record is 8 bytes header + 7 bytes data = 15 bytes total)
        for i in 0..5 {
            let data = format!("record{i}");
            segment.append(data.as_bytes())?;
        }

        // After 5 records: 5 * 15 = 75 bytes, which should trigger is_full()
        assert!(segment.is_full());

        assert!(matches!(
            segment.append(b"overflow"),
            Err(SegmentError::SegmentFull { .. })
        ));

        Ok(())
    }

    #[test]
    fn test_segment_persistence() -> SegmentResult<()> {
        init_tracing();
        let temp_dir = TempDir::new().unwrap();
        let store_path = temp_dir.path().join("segment.log");
        let index_path = temp_dir.path().join("segment.idx");

        let records = ["Persistent", "Data", "Test"];

        // Write data and close segment
        {
            let mut segment = Segment::new(
                &store_path,
                &index_path,
                200, // base_offset = 200
                1024 * 1024,
                1000,
            )?;

            for record in &records {
                segment.append(record.as_bytes())?;
            }
        }
        // Reopen segment and verify data
        {
            let segment = Segment::new(
                &store_path,
                &index_path,
                200, // Same base_offset
                1024 * 1024,
                1000,
            )?;

            assert_eq!(segment.next_offset(), 203);

            // Should be able to read all records
            for (i, record) in records.iter().enumerate() {
                let offset = 200 + i as u64;
                let data = segment.read(offset)?;
                assert_eq!(data, record.as_bytes());
            }
        }

        Ok(())
    }
}

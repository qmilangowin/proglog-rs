//! Segment combines the Store and Index to provide a logical log segment
//! Each stement handles a contiguous range of offsets and manages the coordination between storing data and indexing it.
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
                if offset >= highest_offset {
                    highest_offset = offset + 1;
                }
            }
            highest_offset
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

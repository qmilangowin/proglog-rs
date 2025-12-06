//! Log here is a collection of segments that abstracts a single continous distributed log.
use crate::errors::LogError;
use crate::storage::segment::Segment;
use crate::storage::traits::StorageCleanup;
use crate::{LogResult, storage::traits::LocalFileSystem};
use std::fs::{self, read_dir};
use std::path::PathBuf;
use tracing::{debug, info, instrument, warn};

/// Configuration for the log
#[derive(Debug, Clone)]
pub struct LogConfig {
    /// Maximum size of a segment's store in bytes
    pub max_store_bytes: u64,
    /// Maximum number of index entries per segment
    pub max_index_entries: u64,
    /// Directory where log segments are stored
    pub log_dir: PathBuf,
}

impl Default for LogConfig {
    fn default() -> Self {
        Self {
            max_store_bytes: 1024 * 1024, // 1 MB default
            max_index_entries: 1024,
            log_dir: PathBuf::from("data"),
        }
    }
}

/// Log manages multiple segments and provides a unified interface for a distributed log.
/// It handles segment rotation, offset assignment, and routing reads to the appropriate segment
pub struct Log {
    segments: Vec<Segment>,
    active_segment_index: usize,
    next_offset: u64,
    config: LogConfig,
}

impl Log {
    #[instrument(skip_all, fields(log_dir = ?config.log_dir))]
    pub fn new(config: LogConfig) -> LogResult<Self> {
        debug!("Creating new log");

        // Check that the log directory exists
        fs::create_dir_all(&config.log_dir).map_err(|e| LogError::DirectoryError {
            path: config.log_dir.to_string_lossy().to_string(),
            source: e,
        })?;

        let mut log = Log {
            segments: Vec::new(),
            active_segment_index: 0,
            next_offset: 0,
            config,
        };

        // load existing segments or create the first one
        log.load_segments()?;

        info!(
            segments_count = log.segments.len(),
            next_offset = log.next_offset,
            "Log created successfully"
        );

        Ok(log)
    }

    /// Appends data to the log and returns the assigned offset
    #[instrument(skip(self, data), fields(data_len = data.len()))]
    pub fn append(&mut self, data: &[u8]) -> LogResult<u64> {
        debug!("Appending data to log");

        if self.active_segment().is_full() {
            info!("Active segment is full, rotating to a new segment");
            self.rotate_segment()?;
        }

        let offset = self.active_segment_mut().append(data)?;

        self.next_offset = offset + 1;
        info!(offset, "Data append to log");
        Ok(offset)
    }

    /// Reads data for the given offset
    #[instrument(skip(self), fields(offset))]
    pub fn read(&self, offset: u64) -> LogResult<Vec<u8>> {
        debug!(offset, "Reading from log");

        let segment = self.find_segment_for_offset(offset)?;
        let data = segment.read(offset)?;

        debug!(offset, data_len = data.len(), "Successfully read from log");

        Ok(data)
    }

    pub fn next_offset(&self) -> u64 {
        self.next_offset
    }

    /// Returns the lowest offset available in the log
    pub fn base_offset(&self) -> u64 {
        self.segments.first().map(|s| s.base_offset()).unwrap_or(0)
    }

    /// Returns the highest offset ni the log (if any records exist)
    pub fn latest_offset(&self) -> Option<u64> {
        if self.next_offset > 0 {
            Some(self.next_offset - 1)
        } else {
            None
        }
    }

    pub fn segment_count(&self) -> usize {
        self.segments.len()
    }

    pub fn is_empty(&self) -> bool {
        // check also that the log is empty regardless of whether empty segment objects exist or not.
        self.segments.is_empty() || self.segments.iter().all(|s| s.is_empty())
    }

    /// Returns total size of the log which contains the total size of all segments in bytes
    pub fn total_size(&self) -> u64 {
        self.segments.iter().map(|s| s.store_size()).sum()
    }

    /// truncates the log and keeps only the segments that are less than the truncate point
    #[instrument(skip(self), fields(offset))]
    pub fn truncate(&mut self, offset: u64) -> LogResult<()> {
        info!(offset, "Truncating log");

        let cleanup = LocalFileSystem;
        let mut segments_to_remove = Vec::new();

        for segment in &self.segments {
            if segment.base_offset() >= offset {
                segments_to_remove.push(segment.base_offset());
            }
        }

        for base_offset in segments_to_remove {
            let store_path = self.config.log_dir.join(format!("{base_offset:020}.log"));
            let index_path = self.config.log_dir.join(format!("{base_offset:020}.idx"));

            cleanup
                .cleanup_segment(&store_path, &index_path)
                .map_err(|e| LogError::CleanupError {
                    base_offset,
                    source: e.into(),
                })?;
        }
        self.segments
            .retain(|segment| segment.base_offset() < offset);

        // this is for the edge case so that we always at least have one segment
        if self.segments.is_empty() {
            // Create a new segment starting at the truncate offset
            let segment = self.create_segment(offset)?;
            self.segments.push(segment);
            self.active_segment_index = 0;
        }

        // Update active segment index if needed
        if self.active_segment_index >= self.segments.len() && !self.segments.is_empty() {
            self.active_segment_index = self.segments.len() - 1;
        }

        self.next_offset = offset;

        info!(offset, "Log truncated");
        Ok(())
    }

    /// rotate_segment creates a new segment and makes it active
    #[instrument(skip(self))]
    pub fn rotate_segment(&mut self) -> LogResult<()> {
        let base_offset = self.next_offset;

        debug!(base_offset, "Creating new segment");

        let segment = self.create_segment(base_offset)?;
        self.segments.push(segment);
        self.active_segment_index = self.segments.len() - 1;

        info!(
            base_offset,
            active_segment_index = self.active_segment_index,
            total_segments = self.segments.len(),
            "Segment rotated successfully"
        );

        Ok(())
    }

    /// Loads existing segments from disk or creates the first segment
    #[instrument(skip(self))]
    fn load_segments(&mut self) -> LogResult<()> {
        debug!("Loading existing segments");

        let entries = read_dir(&self.config.log_dir).map_err(|e| LogError::DirectoryError {
            path: self.config.log_dir.to_string_lossy().to_string(),
            source: e,
        })?;

        let mut segment_offset = Vec::new();

        // find all .log files and extract their base offsets.
        for entry in entries {
            let entry = entry.map_err(|e| LogError::DirectoryError {
                path: self.config.log_dir.to_string_lossy().to_string(),
                source: e,
            })?;

            let path = entry.path();
            if let Some(extension) = path.extension()
                && extension == "log"
                && let Some(file_name) = path.file_stem()
                && let Ok(base_offset) = file_name.to_string_lossy().parse::<u64>()
            {
                segment_offset.push(base_offset);
            }
        }

        // Sort offsets to load segments in order
        segment_offset.sort_unstable();

        if segment_offset.is_empty() {
            debug!("No existing segments found, creating initial segment");
            let segment = self.create_segment(0)?;
            self.segments.push(segment);
            self.active_segment_index = 0;
            self.next_offset = 0;
        } else {
            debug!("Found {} existing segments", segment_offset.len());

            for base_offset in segment_offset {
                let segment = self.create_segment(base_offset)?;
                self.segments.push(segment);
            }

            self.active_segment_index = self.segments.len() - 1;

            let last_segment = &self.segments[self.active_segment_index];
            self.next_offset = last_segment.next_offset();

            info!(
                loaded_segments = self.segments.len(),
                next_offset = self.next_offset,
                "Loaded existing segments"
            );
        }

        Ok(())
    }

    fn create_segment(&self, base_offset: u64) -> LogResult<Segment> {
        let store_path = self.config.log_dir.join(format!("{base_offset:020}.log"));
        let index_path = self.config.log_dir.join(format!("{base_offset:020}.idx"));

        debug!(
            base_offset,
            store_path = ?store_path,
            index_path = ?index_path,
            "Creating segment files"
        );

        Segment::new(
            store_path,
            index_path,
            base_offset,
            self.config.max_store_bytes,
            self.config.max_index_entries,
        )
        .map_err(LogError::from)
    }

    fn find_segment_for_offset(&self, offset: u64) -> LogResult<&Segment> {
        for segment in &self.segments {
            if segment.contains_offset(offset) {
                return Ok(segment);
            }
        }

        Err(LogError::OffsetNotFound {
            offset,
            base_offset: self.base_offset(),
            next_offset: self.next_offset,
        })
    }

    /// Returns a reference to the active segment
    fn active_segment(&self) -> &Segment {
        &self.segments[self.active_segment_index]
    }

    /// Returns a mutable reference to the active segment
    fn active_segment_mut(&mut self) -> &mut Segment {
        &mut self.segments[self.active_segment_index]
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

    fn test_config(temp_dir: &TempDir) -> LogConfig {
        LogConfig {
            max_store_bytes: 200, //we keep this small to test rotation later
            max_index_entries: 10,
            log_dir: temp_dir.path().to_path_buf(),
        }
    }

    #[test]
    fn test_log_append_and_read() -> LogResult<()> {
        init_tracing();
        let temp_dir = TempDir::new().unwrap();
        let mut log = Log::new(test_config(&temp_dir))?;

        let data = b"Hello, Log!";
        let offset = log.append(data)?;

        assert_eq!(offset, 0);
        assert_eq!(log.next_offset(), 1);
        assert!(!log.is_empty());

        let read_data = log.read(offset)?;
        assert_eq!(read_data, data);

        Ok(())
    }

    #[test]
    fn test_multiple_records() -> LogResult<()> {
        init_tracing();
        let temp_dir = TempDir::new().unwrap();
        let mut log = Log::new(test_config(&temp_dir))?;

        let records = ["First", "Second", "Third", "Fourth"];
        let mut offsets = Vec::new();

        for record in records {
            let offset = log.append(record.as_bytes())?;
            offsets.push(offset);
        }

        assert_eq!(offsets, vec![0, 1, 2, 3]);
        assert_eq!(log.next_offset(), 4);
        assert_eq!(log.latest_offset(), Some(3));

        for (i, &offset) in offsets.iter().enumerate() {
            let data = log.read(offset)?;
            assert_eq!(data, records[i].as_bytes());
        }

        Ok(())
    }

    #[test]
    fn test_log_segment_rotation() -> LogResult<()> {
        init_tracing();
        let temp_dir = TempDir::new().unwrap();
        let mut log = Log::new(test_config(&temp_dir))?;

        assert_eq!(log.segment_count(), 1);

        // Add enough data to trigger segment rotation
        // Each record is roughly 8 + data bytes, so ~15 bytes per record
        // With max_store_bytes = 200, we need about 13-14 records
        for i in 0..15 {
            let data = format!("Record number {i}");
            log.append(data.as_bytes())?;
        }

        // Should have rotated to multiple segments
        assert!(log.segment_count() > 1);

        // Should still be able to read all records
        for i in 0..15 {
            let data = log.read(i)?;
            let expected = format!("Record number {i}");
            assert_eq!(data, expected.as_bytes());
        }

        Ok(())
    }

    #[test]
    fn test_log_offset_not_found() -> LogResult<()> {
        init_tracing();
        let temp_dir = TempDir::new().unwrap();
        let mut log = Log::new(test_config(&temp_dir))?;

        // Add one record
        log.append(b"test")?;

        // Try to read non-existent offset
        assert!(matches!(
            log.read(999),
            Err(LogError::OffsetNotFound { offset: 999, .. })
        ));

        Ok(())
    }

    #[test]
    fn test_log_empty_state() -> LogResult<()> {
        init_tracing();
        let temp_dir = TempDir::new().unwrap();
        let log = Log::new(test_config(&temp_dir))?;

        assert!(log.is_empty());
        assert_eq!(log.next_offset(), 0);
        assert_eq!(log.base_offset(), 0);
        assert_eq!(log.latest_offset(), None);
        assert_eq!(log.segment_count(), 1);

        Ok(())
    }
}

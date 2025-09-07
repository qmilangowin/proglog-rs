//! Log here is a collection of segments that abstracts a single continous distributed log.
use crate::LogResult;
use crate::errors::LogError;
use crate::storage::segment::Segment;
use std::fs;
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

    #[instrument(skip(self), fields(offset))]
    pub fn truncate(&mut self, offset: u64) -> LogResult<()> {
        info!(offset, "Truncating log");

        let mut segments_to_keep = Vec::new();

        for segment in &self.segments {
            if segment.base_offset() < offset {
                segments_to_keep.push(segment);
            } else {
                //TODO: remove sgment files
                warn!(
                    segment_base = segment.base_offset(),
                    "Segment would be removed (file cleanup not yet implemented)"
                );
            }
        }

        // TODO: implement actual segment removal. For now adjust next_offset
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

        // TODO: scan the log directory for existing segment files
        // For now, just create the first segment if none exist

        if self.segments.is_empty() {
            let segment = self.create_segment(0)?;
            self.segments.push(segment);
            self.active_segment_index = 0;
            self.next_offset = 0;
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

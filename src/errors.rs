use std::io;
use thiserror::Error;

/// Proglog-RS errors
#[derive(Debug, Error)]
pub enum ProglogError {
    #[error("Storage error: {0}")]
    Storage(#[from] StorageError),

    #[error("Index error: {0}")]
    Index(#[from] IndexError),

    #[error("Network error: {0}")]
    Network(#[from] NetworkError),

    #[error("Consensus error: {0}")]
    Consensus(#[from] ConsensusError),

    #[error("Configuration error: {message}")]
    Config { message: String },

    #[error("Internal error: {message}")]
    Internal { message: String },
}

#[derive(Debug, Error)]
pub enum StorageError {
    #[error("Failed to open store file: {path}")]
    OpenFailed {
        path: String,
        #[source]
        source: io::Error,
    },

    #[error("Failed to write to store at position {position}")]
    WriteFailed {
        position: u64,
        #[source]
        source: io::Error,
    },

    #[error("Failed to read from store at position {position}")]
    ReadFailed {
        position: u64,
        #[source]
        source: io::Error,
    },

    #[error("Read position {position} is beyond store size {size}")]
    ReadBeyondEnd { position: u64, size: u64 },

    #[error("Corrupted record at position {position}: {reason}")]
    CorruptedRecord { position: u64, reason: String },

    #[error("Failed to grow store from {current_size} to {target_size}")]
    GrowFailed {
        current_size: u64,
        target_size: u64,
        #[source]
        source: io::Error,
    },

    #[error("Memory mapping failed for size {size}")]
    MmapFailed {
        size: u64,
        #[source]
        source: io::Error,
    },

    #[error("Store is in read-only mode")]
    ReadOnly,
}

/// Index-related errors  
#[derive(Error, Debug)]
pub enum IndexError {
    #[error("Failed to open index file: {path}")]
    OpenFailed {
        path: String,
        #[source]
        source: io::Error,
    },

    #[error("Failed to write to index at position {position}")]
    WriteFailed {
        position: u64,
        #[source]
        source: io::Error,
    },

    #[error("Offset {offset} not found in index")]
    OffsetNotFound { offset: u64 },

    #[error("Index entry at position {position} is corrupted")]
    CorruptedEntry { position: u64 },

    #[error("Index file is corrupted: {reason}")]
    CorruptedFile { reason: String },

    #[error("Failed to grow index from {current_size} to {target_size}")]
    GrowFailed {
        current_size: u64,
        target_size: u64,
        #[source]
        source: io::Error,
    },

    #[error("Memory mapping failed for size {size}")]
    MmapFailed {
        size: u64,
        #[source]
        source: io::Error,
    },

    #[error("Index is full, cannot add more entries")]
    IndexFull,

    #[error("Invalid offset {offset}, must be >= {min_offset}")]
    InvalidOffset { offset: u64, min_offset: u64 },
}

#[derive(Debug, Error)]
pub enum SegmentError {
    #[error("Segment is full: base={base_offset}, size={current_size}/{max_size}")]
    SegmentFull {
        base_offset: u64,
        max_size: u64,
        current_size: u64,
    },

    #[error("Offset {offset} out of range for segment {base_offset}..{next_offset}")]
    OffsetOutOfRange {
        offset: u64,
        base_offset: u64,
        next_offset: u64,
    },

    #[error("Storage error: {0}")]
    Storage(#[from] StorageError),

    #[error("Index error: {0}")]
    Index(#[from] IndexError),
}

#[derive(Debug, Error)]
pub enum LogError {
    #[error("Directory error for path {path}")]
    DirectoryError {
        path: String,
        #[source]
        source: std::io::Error,
    },
    #[error("Offset {offset} not found (range: {base_offset}..{next_offset}")]
    OffsetNotFound {
        offset: u64,
        base_offset: u64,
        next_offset: u64,
    },
    #[error("Segment error: {0}")]
    Segment(#[from] SegmentError), //converts SegmentError to LogError via From trait implementation. Convienence macro
}

/// Network-related errors
#[derive(Error, Debug)]
pub enum NetworkError {
    #[error("Connection failed to {address}")]
    ConnectionFailed { address: String },

    #[error("Request timeout after {timeout_ms}ms")]
    Timeout { timeout_ms: u64 },

    #[error("Invalid request: {reason}")]
    InvalidRequest { reason: String },

    #[error("Authentication failed")]
    AuthenticationFailed,

    #[error("Server unavailable")]
    ServerUnavailable,
}

/// Consensus-related errors
#[derive(Error, Debug)]
pub enum ConsensusError {
    #[error("Not the leader, current leader is {leader_id:?}")]
    NotLeader { leader_id: Option<String> },

    #[error("No leader elected")]
    NoLeader,

    #[error("Consensus timeout")]
    Timeout,

    #[error("Insufficient replicas: need {required}, have {available}")]
    InsufficientReplicas { required: usize, available: usize },

    #[error("Log divergence detected at index {index}")]
    LogDivergence { index: u64 },
}

impl ProglogError {
    /// Check if this error is recoverable (e.g., can retry)
    pub fn is_recoverable(&self) -> bool {
        match self {
            ProglogError::Storage(e) => e.is_recoverable(),
            ProglogError::Network(NetworkError::Timeout { .. }) => true,
            ProglogError::Network(NetworkError::ServerUnavailable) => true,
            ProglogError::Consensus(ConsensusError::Timeout) => true,
            ProglogError::Consensus(ConsensusError::NotLeader { .. }) => true,
            _ => false,
        }
    }

    /// Check if this error indicates a temporary condition
    pub fn is_temporary(&self) -> bool {
        matches!(
            self,
            ProglogError::Storage(StorageError::WriteFailed { .. })
                | ProglogError::Storage(StorageError::ReadFailed { .. })
                | ProglogError::Network(NetworkError::Timeout { .. })
                | ProglogError::Network(NetworkError::ServerUnavailable)
                | ProglogError::Consensus(ConsensusError::NoLeader)
        )
    }
}

impl StorageError {
    pub fn is_recoverable(&self) -> bool {
        match self {
            StorageError::WriteFailed { .. } => true,
            StorageError::ReadFailed { .. } => true,
            StorageError::GrowFailed { .. } => true,
            StorageError::ReadBeyondEnd { .. } => false, // Client error
            StorageError::CorruptedRecord { .. } => false, // Data integrity issue
            StorageError::ReadOnly => false,             // Configuration issue
            _ => false,
        }
    }
}

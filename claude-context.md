# proglog-rs: Claude Context Summary

## Project Overview
Building a distributed commit log in Rust called **proglog-rs**, following "Distributed Services with Go" by Travis Jeffery. This is both a learning project to improve Rust skills and a potential foundation for rewriting a distributed alert monitoring service currently running in Go at work.

## Developer Profile
- **Name:** Milan
- **Background:** Professional Go experience, fairly proficient with Rust
- **Approach:** Methodical, bottom-up implementation with emphasis on understanding each component
- **Learning Style:** Prefers detailed explanations, asks clarifying questions, reviews code thoroughly before proceeding

## Project Philosophy
- Systematic, phase-based development - implement each layer thoroughly before moving to next
- Emphasis on understanding architectural decisions and naming conventions
- Production-ready patterns: proper trait abstractions, comprehensive error handling, observability
- Build with extensibility in mind through clean interfaces
- Prefers simplicity and YAGNI (You Aren't Gonna Need It) over premature abstraction

## Key Design Decisions Made

### 1. Consistent u64 Types
- Used `u64` throughout instead of book's mixed `u32/u64`
- Index entries: `[8-byte offset (u64)][8-byte position (u64)]` = 16 bytes
- Book uses: `[4-byte offset (u32)][8-byte position (u64)]` = 12 bytes
- Rationale: Consistency across codebase, support for massive logs (18+ quintillion records)

### 2. Linear Search in Index (Current Implementation)
- Using linear O(n) search for now
- TODO: Implement sorted segments with binary search when segment closes
- Future consideration: B+ trees for very large indexes
- Reasoning: Simple, correct, sufficient for reasonably-sized segments; optimize when needed

### 3. Minimal Traits Approach
- Only added traits that solve real, concrete problems:
  - `StorageBackend` - Different storage destinations (local disk, cloud storage)
  - `StorageCleanup` - Different deletion mechanisms across storage systems
- Avoided premature abstraction (no `DistributedLog` trait yet)
- Can extract more traits later when actually needed

### 4. Error Handling with thiserror
- `#[source]` for error chaining
- `#[from]` for automatic conversions
- Comprehensive error types with context
- Distinction between recoverable and non-recoverable errors

## Technical Architecture

### Storage Layer Hierarchy
```
Log (manages collection of segments)
├── Segment (combines Store + Index, assigns offsets)
│   ├── Store (physical data storage, append-only)
│   └── Index (offset-to-position mapping, fast lookups)
```

### Key Concepts Clarified
- **Offset** = Sequential record ID/number (0, 1, 2, 3...) - the KEY in the lookup system
- **Position** = Byte location in Store file where record starts - the VALUE
- **Index Entry** = Maps offset → position
- **Segment** = Bounded chunk of log with its own Store + Index files
- **Log** = Collection of segments appearing as single continuous log

### File Organization
```
data/
├── 00000000000000000000.log  # Segment 0 store
├── 00000000000000000000.idx  # Segment 0 index
├── 00000000000000000100.log  # Segment 1 store (starts at offset 100)
└── 00000000000000000100.idx  # Segment 1 index
```

## Implementation Status

### ✅ Completed Components

#### Store Layer (`src/storage/store.rs`)
- Append-only files with 8-byte length prefixes: `[8-byte len][data][8-byte len][data]...`
- Memory-mapped I/O for performance
- Crash recovery: forward-scan truncation removes torn writes
- Dynamic growth via remapping
- Comprehensive tests including corruption scenarios

**Critical Bug Fixed:** Removed `.append(true)` from file opening - was causing corruption with mmap

#### Index Layer (`src/storage/index.rs`)
- 16-byte entries: `[8-byte offset][8-byte position]`
- Memory-mapped access
- Linear search (O(n)) - binary search deferred
- Handles out-of-order writes (distributed arrival simulation)
- Tests cover sequential/non-sequential writes and persistence

**Critical Bug Fixed:** Integer underflow in `write()` - was calculating `self.size - 1` when size was 0

#### Segment Layer (`src/storage/segment.rs`)
- Combines Store + Index
- **Offset assignment happens here** - sequential within segment
- Rotation when full (size or entry limits)
- Bounds checking for offset ranges
- Tests cover append/read, rotation, persistence

#### Log Layer (`src/storage/log.rs`)
- **Collection of segments** - manages multiple segment files
- Automatic segment rotation
- Routes reads to correct segment
- Truncation with file cleanup using `StorageCleanup` trait
- Aggregate operations: `total_size()`, `segment_count()`

#### Storage Traits (`src/storage/traits.rs`)
- `StorageBackend` - abstraction for different storage destinations
- `StorageCleanup` - abstraction for file deletion (local vs cloud)
- `LocalFileSystem` - concrete implementation for local disk
- Used in `Log::truncate()` for segment file cleanup

### Project Structure
```
src/
├── main.rs
├── lib.rs
├── storage/
│   ├── mod.rs
│   ├── traits.rs      # StorageBackend, StorageCleanup
│   ├── store.rs       # Append-only data files
│   ├── index.rs       # Offset-to-position mapping
│   ├── segment.rs     # Store + Index coordination
│   └── log.rs         # Multi-segment management
├── server/            # (planned)
│   ├── mod.rs
│   ├── grpc.rs
│   └── auth.rs
├── discovery/         # (planned)
│   ├── mod.rs
│   └── raft.rs
├── proto/             # (planned)
│   └── log.proto
└── errors.rs          # thiserror-based error types
```

## Important Learning Moments

### Understanding Offsets vs Positions
- Offset = "Give me record #5" (the KEY - what you're looking for)
- Position = "That record starts at byte 1024 in the Store file" (the VALUE - where it is)
- Index provides the mapping: offset → position
- This is the fundamental lookup mechanism

### Why Offsets Are Sequential
- Global sequence number across the entire distributed log
- Offset 0 = 1st record ever written, offset 1 = 2nd record, etc.
- Enables ordering, replication, consumer tracking, range queries
- In distributed systems, records may arrive out-of-order but offsets remain sequential

### Segment as Offset Factory
- Segment assigns sequential offsets within its range
- Store just returns position where data was written
- Index just stores whatever mapping you give it
- Segment coordinates: "This append gets offset 100, store it at position 1024"

### Log as Collection Manager
- Log manages segments as a unified abstraction
- Routes operations to appropriate segment
- Handles rotation, truncation, aggregate stats
- Makes multiple bounded files appear as one infinite log

## Future Enhancements & TODOs

### Immediate (Documented in Code)
- [ ] Sort index entries when segment closes (enable binary search)
- [ ] Implement `StorageBackend` trait for Store
- [ ] More comprehensive integration tests

### Kafka Integration (Later Phase)
- Observer pattern for real-time log event streaming
- LogEventPublisher and LogObserver classes
- Multiple observers: AnalyticsService, AlertingService, BackupService, MetricsAggregator
- Event sourcing capability with replay
- Fan-out to multiple systems without coupling

### Performance Considerations
- Consider B+ trees for very large indexes
- Benchmark mmap vs direct I/O
- Log compaction and garbage collection
- Compression support

## Next Phase: gRPC Server

### Location
`src/server/grpc.rs` + `src/proto/log.proto`

### Goals
- Define protocol buffer schema for log operations
- Implement gRPC service wrapping Log layer
- Network API for remote clients (append, read, get_servers)

### Expected Dependencies
- `tonic` - gRPC for Rust
- `prost` - Protocol Buffers
- `tokio` - async runtime

### After gRPC Roadmap
1. Service Discovery - node registration, health checks, cluster membership
2. Raft Consensus - leader election, log replication, partition handling
3. Security - TLS, authentication, ACLs
4. Observability - metrics, tracing, monitoring

## Code Patterns & Preferences

### Tracing & Logging
- Uses `tracing` crate for structured logging
- `#[instrument]` on key methods
- Debug/info/warn levels appropriately used
- Test helper: `init_tracing()` with `Once` pattern

### Testing Approach
- Unit tests for each layer with `tempfile` for isolation
- Tests cover: basic operations, edge cases, persistence, error conditions
- Defers integration tests until more components complete
- Example: corruption scenarios, out-of-order writes, empty states

### Error Handling Style
- `thiserror` for ergonomic error definitions
- Contextual error information (positions, offsets, paths)
- `#[source]` for error chaining
- `#[from]` for automatic conversions
- Result types: `StorageResult<T>`, `IndexResult<T>`, `SegmentResult<T>`, `LogResult<T>`

### Code Review Style
- Asks for detailed explanations of specific code sections
- Questions naming conventions and design decisions
- Wants to understand "why" before "how"
- Appreciates step-by-step breakdowns with examples

## Vocabulary & Terminology

### Key Terms
- **Torn write/record** - Incomplete write due to crash (partial data)
- **Recovery scan** - Forward scan through file to detect corruption
- **Segment rotation** - Creating new segment when current one is full
- **Base offset** - First offset in a segment
- **Next offset** - Offset that will be assigned to next record
- **Active segment** - Current segment being written to
- **Memory mapping (mmap)** - File mapped directly to memory for fast access

### Common Patterns
- `retain()` - Filter Vec in-place, keeping elements matching condition
- `Drop` trait - Cleanup when value goes out of scope
- Associated type (`type Error`) - Type defined by trait implementor
- `#[instrument]` - tracing macro for automatic span creation

## Development Commands
```bash
# Run all tests
cargo test

# Run with debug logging
RUST_LOG=debug cargo test -- --nocapture

# Run specific module
cargo test store
cargo test index
cargo test segment
cargo test log

# Run specific test
cargo test test_store_append_and_read
```

## Communication Preferences
- Prefers clear, detailed explanations over brief summaries
- Values examples and visual breakdowns (memory layouts, byte diagrams)
- Appreciates being asked clarifying questions before implementation
- Likes to review and understand code before moving forward
- Open to corrections and alternative approaches

## Context for Next Session
When continuing with gRPC implementation:
1. Review this context file first
2. Reference `PROGRESS.md` for detailed technical status
3. Start with protocol buffer schema design
4. Keep explanations detailed - Milan prefers understanding over speed
5. Use concrete examples when explaining new concepts
6. Remember: methodical, bottom-up approach preferred

---

**Last Updated:** 2025-01-18
**Current Phase:** Storage Layer Complete
**Next Task:** Begin gRPC server implementation with protocol buffers
**Key Point:** Milan has solid understanding of storage architecture and is ready for networking layer

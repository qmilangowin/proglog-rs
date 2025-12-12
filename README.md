# proglog-rs

[![Ask DeepWiki](https://deepwiki.com/badge.svg)](https://deepwiki.com/qmilangowin/proglog-rs)

A distributed commit log implementation in Rust, inspired by "Distributed Services with Go" by Travis Jeffery.

NB: Still WIP when I have time. Not yet complete.

## Project Structure

```
src/
├── main.rs                 # CLI and server entry point
├── lib.rs                  # Library root with public API
├── storage/
│   ├── mod.rs             # Storage module root
│   ├── log.rs             # Main Log struct (coordinates segments)
│   ├── segment.rs         # Segment implementation (store + index)
│   ├── store.rs           # Append-only store (the actual data)
│   └── index.rs           # Offset index (fast lookups)
├── server/
│   ├── mod.rs             # Server module root
│   ├── grpc.rs            # gRPC service implementation
│   └── auth.rs            # Authentication and TLS
├── discovery/
│   ├── mod.rs             # Service discovery
│   └── raft.rs            # Raft consensus (later phases)
├── proto/
│   └── log.proto          # Protocol buffer definitions
└── errors.rs              # Custom error types
```

## Storage Architecture

The log uses a two-file approach: a **Store** (append-only data file) and an **Index** (offset-to-position mapping).

```
                    WRITE OPERATION
    ┌─────────────────────────────────────────────────────┐
    │                                                     │
    │  1. Write record to Store                           │
    │     ┌─────────────────────────┐                     │
    │     │     STORE FILE          │                     │
    │     │  ┌───────────────────┐  │                     │
    │     │  │ [8-byte len][data] │  │ ← Append record    │
    │     │  └───────────────────┘  │                     │
    │     └─────────────────────────┘                     │
    │              │                                      │
    │              │ Returns position (e.g., 1024)       │
    │              ▼                                      │
    │  2. Write mapping to Index                          │
    │     ┌─────────────────────────┐                     │
    │     │     INDEX FILE          │                     │
    │     │  ┌─────────────────────┐│                     │
    │     │  │ [offset][position]  ││ ← Map offset 5      │
    │     │  │   [5]   [1024]      ││   to position 1024  │
    │     │  └─────────────────────┘│                     │
    │     └─────────────────────────┘                     │
    └─────────────────────────────────────────────────────┘

                     READ OPERATION
    ┌─────────────────────────────────────────────────────┐
    │                                                     │
    │  1. Lookup offset in Index                          │
    │     ┌─────────────────────────┐                     │
    │     │     INDEX FILE          │                     │
    │     │  ┌─────────────────────┐│                     │
    │     │  │ Find offset 5       ││ → Returns position  │
    │     │  │ Returns: 1024       ││   1024              │
    │     │  └─────────────────────┘│                     │
    │     └─────────────────────────┘                     │
    │              │                                      │
    │              │ Position: 1024                       │
    │              ▼                                      │
    │  2. Read record from Store at position              │
    │     ┌─────────────────────────┐                     │
    │     │     STORE FILE          │                     │
    │     │  ┌───────────────────┐  │                     │
    │     │  │ Read at pos 1024  │  │ → Returns record    │
    │     │  │ [8-byte len][data] │  │   data              │
    │     │  └───────────────────┘  │                     │
    │     └─────────────────────────┘                     │
    └─────────────────────────────────────────────────────┘
```

## Storage Format

Records are stored as length-prefixed entries in the Store:

```
[8-byte length][record data][8-byte length][record data]...
```

Index entries map logical offsets to physical positions:

```
[8-byte offset][8-byte position][8-byte offset][8-byte position]...
```

where *offset* denotes the numerical key of the record.



### Example

**Store File:**

| Position | Bytes                            | Meaning                    |
|----------|----------------------------------|----------------------------|
| 0–7      | 05 00 00 00 00 00 00 00         | Length = 5                |
| 8–12     | 68 65 6C 6C 6F                  | "hello"                   |
| 13–20    | 08 00 00 00 00 00 00 00         | Length = 8                |
| 21–28    | 77 6F 72 6C 64 21 21 21         | "world!!!"                |

**Index File (maps record numbers to store positions):**

| Position | Bytes                            | Meaning                           |
|----------|----------------------------------|-----------------------------------|
| 0–7      | 00 00 00 00 00 00 00 00         | Record offset = 0                |
| 8–15     | 00 00 00 00 00 00 00 00         | Store position = 0 (→ "hello")   |
| 16–23    | 01 00 00 00 00 00 00 00         | Record offset = 1                |
| 24–31    | 0D 00 00 00 00 00 00 00         | Store position = 13 (→ "world!!!") |

### How Reading Works - Step by Step

**Example: "I want to read record #1"**

```
Step 1: Calculate Index position
  - Each Index entry is 16 bytes (8-byte offset + 8-byte position)
  - Record #1 is the 2nd entry (0-indexed)
  - Index position = 1 × 16 = byte 16

Step 2: Read from Index at byte 16
  - Read 16 bytes starting at position 16
  - Bytes 16-23: [01 00 00 00 00 00 00 00] = offset 1 ✓ (confirms we have the right entry)
  - Bytes 24-31: [0D 00 00 00 00 00 00 00] = position 13 (0x0D = 13 decimal)

Step 3: Read from Store at byte 13
  - Jump to Store file position 13
  - Read 8 bytes: [08 00 00 00 00 00 00 00] = length is 8
  - Read next 8 bytes: [77 6F 72 6C 64 21 21 21] = "world!!!"

Result: Record #1 contains "world!!!"
```

**Visual Flow:**
```
Request: "Get record 1"
         ↓
    INDEX FILE                          STORE FILE
    [offset][position]                  [length][data]
    ─────────────────                   ──────────────
    [0][0]   ← record 0      ┌─────→    [5][hello]     ← position 0
    [1][13]  ← record 1 ─────┘          [8][world!!!]  ← position 13
         ↑
    "Found it! Go to position 13"
```

## Crash Recovery

The store implements automatic crash recovery using forward-scan truncation:

1. **Scan forward** through all records on file open
2. **Detect torn writes** (incomplete length headers or data)
3. **Truncate** at the last valid record
4. **Continue** with clean, consistent state

### Recovery Checks

```rust
// Check 1: Can we read length prefix?
if pos + 8 > file_len { break; }

// Check 2: Read the length
record_len = u64::from_le_bytes(header)

// Check 3: Can we read the full data?
if pos + 8 + record_len > file_len { break; }

// Check 4: Length reasonable? (< 100MB)
if record_len > 100MB { break; }
```

## Features

## Features

## Features

### Storage Layer ✅

- ✅ **Crash-safe storage** with automatic recovery
- ✅ **Memory-mapped I/O** for high performance
- ✅ **Append-only Store** with length-prefixed records
- ✅ **Index layer** for fast offset-to-position lookups
- ✅ **Segment management** with automatic rotation
- ✅ **Log abstraction** managing multiple segments as unified log
- ✅ **Structured error handling** with comprehensive testing

### Network Layer ✅

- ✅ **gRPC server** with Protocol Buffers API
- ✅ **Produce/Consume operations** (Kafka-style naming)
- ✅ **Thread-safe concurrent access**
- ✅ **Persistence on restart** - loads existing segments automatically

### Planned Features 🚧

- 🚧 **Service Discovery** - Cluster membership
- 🚧 **Raft Consensus** - Leader election and log replication
- 🚧 **Security** - TLS, authentication, authorization
- 🚧 **Observability** - Metrics, distributed tracing

## Development

```bash
# Run tests
just test

# Run with debug logging
just test-debug

# Run specific test
just test-one test_store_persistence
```

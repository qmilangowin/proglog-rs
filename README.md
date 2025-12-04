# proglog-rs

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

| Offset | Bytes                            | Meaning                    |
|--------|----------------------------------|----------------------------|
| 0–7    | 05 00 00 00 00 00 00 00          | Length = 5                |
| 8–12   | 68 65 6C 6C 6F                   | "hello"                   |
| 13–20  | 08 00 00 00 00 00 00 00          | Length = 8                |
| 21–28  | 77 6F 72 6C 64 21 21 21          | "world!!!"                |

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

- ✅ **Crash-safe storage** with automatic recovery
- ✅ **Memory-mapped I/O** for high performance
- ✅ **Structured error handling** with recovery guidance
- ✅ **Comprehensive testing** including corruption scenarios
- 🚧 **Index layer** for fast offset lookups (in progress)
- 🚧 **gRPC networking** (planned)
- 🚧 **Raft consensus** (planned)

## Development

```bash
# Run tests
just test

# Run with debug logging
just test-debug

# Run specific test
just test-one test_store_persistence
```

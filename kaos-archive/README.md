# kaos-archive

High-performance message archive using memory-mapped files.

## Features

- **Append-only log** - Sequential writes for max throughput
- **Zero-copy reads** - mmap returns direct pointers
- **CRC32 checksums** - Data integrity verification
- **Index file** - O(1) message lookup by sequence

## Performance (Apple M3)

| Operation | Throughput |
|-----------|------------|
| Append | 118 MB/s |
| Read | ~1 ns (zero-copy) |

## Usage

```rust
use kaos_archive::Archive;

// Create archive (1GB capacity)
let mut archive = Archive::create("/tmp/messages", 1024 * 1024 * 1024)?;

// Append messages
let seq = archive.append(b"hello world")?;

// Read by sequence (zero-copy)
let msg = archive.read(seq)?;

// Replay range
archive.replay(0, 1000, |seq, data| {
    println!("{}: {:?}", seq, data);
})?;

// Persist to disk
archive.flush()?;
```

## File Format

```
messages.log:
┌──────────────────┐
│ Header (64B)     │  magic, version, write_pos, msg_count
├──────────────────┤
│ Frame 0          │  length (4B) + checksum (4B) + payload
├──────────────────┤
│ Frame 1          │
├──────────────────┤
│ ...              │
└──────────────────┘

messages.idx:
┌──────────────────┐
│ Entry 0 (16B)    │  offset (8B) + length (4B) + pad
├──────────────────┤
│ Entry 1          │
├──────────────────┤
│ ...              │
└──────────────────┘
```


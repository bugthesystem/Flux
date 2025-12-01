# kaos-ipc

Shared memory IPC using kaos ring buffers.

> **⚠️ Preview Release (0.1.0-preview)** - API may change. Uses `unsafe` for shared memory.

## Performance

Benchmarked on Apple M1 (macOS):

| Benchmark | Throughput |
|-----------|------------|
| Single message | ~137 M/s |
| Sustained batch | ~310 M/s |

```bash
cargo bench --bench bench_ipc
```

## Usage

### Publisher (Process A)

```rust
use kaos_ipc::{Publisher, Slot8};

let mut publisher = Publisher::<Slot8>::new("/tmp/channel", 64 * 1024)?;
publisher.send(&42u64.to_le_bytes())?;
```

### Subscriber (Process B)

```rust
use kaos_ipc::{Subscriber, Slot8};

let mut subscriber = Subscriber::<Slot8>::new("/tmp/channel")?;
subscriber.receive(|slot| {
    println!("{}", slot.value);
});
```

## Architecture

Uses file-backed `mmap(MAP_SHARED)` for cross-process ring buffer access.

```
Process A                  /tmp/channel                 Process B
┌───────────┐       ┌──────────────────────┐       ┌───────────┐
│ Publisher │──────▶│   SharedRingBuffer   │◀──────│Subscriber │
└───────────┘       └──────────────────────┘       └───────────┘
```

## Slot Types

| Type | Size |
|------|------|
| `Slot8` | 8B |
| `Slot16` | 16B |
| `Slot32` | 32B |
| `Slot64` | 64B |
| `MessageSlot` | 128B |

## Platform Support

| Platform | Status |
|----------|--------|
| macOS ARM64 | ✅ Tested |
| macOS x86_64 | Untested |
| Linux | Untested |

## License

MIT OR Apache-2.0

# flux

Lock-free ring buffer implementing the LMAX Disruptor pattern.

> **⚠️ Preview Release (0.1.0-preview)** - API may change. Uses `unsafe` for performance.

## Performance

Benchmarked on Apple M1 (macOS), 10M events, batch size 8192:

| Slot Size | Throughput |
|-----------|------------|
| 8B | 1.8-2.0 B/s |
| 16B | 1.8 B/s |
| 32B | 880 M/s - 1.0 B/s |
| 64B | 450-480 M/s |

```bash
cargo bench --bench bench_criterion
```

## Usage

```rust
use flux::disruptor::{RingBuffer, SmallSlot};
use std::sync::Arc;

let ring = Arc::new(RingBuffer::<SmallSlot>::new(1024)?);

// Producer
let mut cursor = 0u64;
if let Some((seq, slots)) = ring.try_claim_slots(10, cursor) {
    for (i, slot) in slots.iter_mut().enumerate() {
        slot.value = i as u64;
    }
    ring.publish(seq + slots.len() as u64 - 1);
    cursor = seq + slots.len() as u64;
}

// Consumer
let slots = ring.get_read_batch(0, 10);
ring.update_consumer(9);
```

## Ring Buffers

| Type | Pattern | Use Case |
|------|---------|----------|
| `RingBuffer<T>` | SPSC | Highest throughput |
| `MessageRingBuffer` | SPSC | Variable-length messages |
| `SharedRingBuffer<T>` | SPSC | Cross-process (mmap) |
| `MpscRingBuffer<T>` | MPSC | Multiple producers |
| `SpmcRingBuffer<T>` | SPMC | Multiple consumers |
| `MpmcRingBuffer<T>` | MPMC | Multiple producers/consumers |

## Slot Types

| Type | Size |
|------|------|
| `SmallSlot` | 8B |
| `Slot16` | 16B |
| `Slot32` | 32B |
| `Slot64` | 64B |
| `MessageSlot` | 128B |

## Memory Allocation

```rust
// Heap (default)
let ring = RingBuffer::<SmallSlot>::new(size)?;

// Memory-mapped (mlock'd)
let ring = RingBuffer::<SmallSlot>::new_mapped(size)?;
```

## Platform Support

| Platform | Status |
|----------|--------|
| macOS ARM64 | ✅ Tested |
| macOS x86_64 | Untested |
| Linux | Untested |

## License

MIT OR Apache-2.0

# kaos

Lock-free ring buffer (LMAX Disruptor pattern).

> **⚠️ Preview (0.1.0)** - API may change. Uses `unsafe`.

## Performance

Apple M1, 10M events, batch 8192:

| Slot | Throughput |
|------|------------|
| 8B | 2.0 B/s |
| 16B | 1.8 B/s |
| 32B | 1.0 B/s |
| 64B | 450 M/s |

```bash
cargo bench --bench bench_criterion
```

## Usage

```rust
use kaos::disruptor::{RingBuffer, Slot8};
use std::sync::Arc;

let ring = Arc::new(RingBuffer::<Slot8>::new(1024)?);

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
| `Slot8` | 8B |
| `Slot16` | 16B |
| `Slot32` | 32B |
| `Slot64` | 64B |
| `MessageSlot` | 128B |

## Memory Allocation

```rust
// Heap (default)
let ring = RingBuffer::<Slot8>::new(size)?;

// Memory-mapped (mlock'd)
let ring = RingBuffer::<Slot8>::new_mapped(size)?;
```

## Concurrency Testing with Loom

All ring buffer types are tested with [**Loom**](https://github.com/tokio-rs/loom) - a concurrency permutation testing tool from the Tokio team. Loom exhaustively explores all possible thread interleavings under the C11 memory model.

```bash
# Run loom tests
RUSTFLAGS="--cfg loom" cargo test --test loom_ring_buffer --release
```

**Tested patterns:**
- ✅ SPSC cursor synchronization
- ✅ MPSC compare_exchange_weak racing
- ✅ SPMC consumer claim contention
- ✅ MPMC completion tracking
- ✅ Fence synchronization

## Platform Support

| Platform | Status |
|----------|--------|
| macOS ARM64 | ✅ Tested |
| macOS x86_64 | Untested |
| Linux | Untested |

## License

MIT OR Apache-2.0

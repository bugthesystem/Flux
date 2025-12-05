# kaos

Lock-free ring buffer (LMAX Disruptor pattern).

## Usage

```rust
use kaos::disruptor::{RingBuffer, Slot8};
use std::sync::Arc;

let ring = Arc::new(RingBuffer::<Slot8>::new(1024)?);

// Producer
if let Some((seq, slots)) = ring.try_claim_slots(10, 0) {
    for (i, slot) in slots.iter_mut().enumerate() {
        slot.value = i as u64;
    }
    ring.publish(seq + slots.len() as u64);
}

// Consumer
let slots = ring.get_read_batch(0, 10);
ring.update_consumer(slots.len() as u64);
```

## Ring Buffers

| Type | Pattern |
|------|---------|
| `RingBuffer<T>` | SPSC |
| `BroadcastRingBuffer<T>` | SPSC fan-out |
| `SharedRingBuffer<T>` | SPSC (mmap) |
| `MpscRingBuffer<T>` | MPSC |
| `SpmcRingBuffer<T>` | SPMC |
| `MpmcRingBuffer<T>` | MPMC |

## Slot Types

| Type | Size |
|------|------|
| `Slot8` | 8B |
| `Slot16` | 16B |
| `Slot32` | 32B |
| `Slot64` | 64B |
| `MessageSlot` | 128B |

## Benchmarks

```bash
cargo bench -p kaos
```

## License

MIT OR Apache-2.0

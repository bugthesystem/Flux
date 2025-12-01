# Architecture

## Ring Buffer Layout

```
┌────────────────────────────────────────────────┐
│                 RingBuffer<T>                  │
├────────────────────────────────────────────────┤
│ buffer: *mut T          Raw pointer to slots   │
│ size: usize             Power of 2 capacity    │
│ mask: usize             size - 1               │
│ producer_cursor: AtomicU64                     │
│ consumer_cursor: AtomicU64                     │
└────────────────────────────────────────────────┘
```

Cache-line padding (128 bytes, `#[repr(align(128))]`) between cursors prevents false sharing.

## SPSC Flow

```
Producer                    Consumer
   │                           │
   ├─ try_claim_slots()        │
   ├─ Write slots              │
   ├─ publish()                │
   │                           ├─ load producer_cursor
   │                           ├─ get_read_batch()
   │                           ├─ update_consumer()
```

## Memory Ordering

**Producer:**
```rust
// Check space (Relaxed)
let consumer = consumer_cursor.load(Relaxed);
// Write slots via raw pointer
// Publish (Release)
producer_cursor.store(next, Release);
```

**Consumer:**
```rust
// Check available (Acquire)
let producer = producer_cursor.load(Acquire);
// Read slots
// Update (Relaxed)
consumer_cursor.store(next, Relaxed);
```

## Slot Types

| Type | Size | Use Case |
|------|------|----------|
| Slot8 | 8B | Counters, IDs |
| Slot16 | 16B | Small payloads |
| Slot32 | 32B | Typical messages |
| Slot64 | 64B | Cache-aligned |
| MessageSlot | 128B | Variable messages |

## File Structure

```
kaos/src/disruptor/
├── mod.rs              Public exports
├── ring_buffer_core.rs Shared logic
├── slots.rs            Slot definitions (Slot8/16/32/64)
├── message_slot.rs     MessageSlot (128B)
├── macros.rs           publish_batch!, consume_batch!
├── common.rs           PaddedProducerSequence, PaddedConsumerSequence
├── spsc/
│   ├── ring_buffer.rs         RingBuffer<T>
│   ├── message_ring_buffer.rs MessageRingBuffer
│   └── shared_ring_buffer.rs  SharedRingBuffer (IPC)
├── mpsc/
│   └── mpsc_ring_buffer.rs
├── spmc/
│   └── spmc_ring_buffer.rs
└── mpmc/
    └── mpmc_ring_buffer.rs
```

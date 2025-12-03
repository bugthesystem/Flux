# Kaos Design

Lock-free ring buffers implementing LMAX Disruptor pattern.

## Core Concepts

**Ring Buffer**: Fixed-size circular buffer (power of 2).
- Producer writes, advances cursor
- Consumer follows producer cursor
- Index = `sequence & (size - 1)`

**Sequences**: 64-bit monotonic, never wrap in practice.

## Slot Types

| Type | Size | Use |
|------|------|-----|
| Slot8 | 8B | Max throughput |
| Slot16 | 16B | Small payload |
| Slot32 | 32B | Balanced |
| Slot64 | 64B | Cache-aligned |
| MessageSlot | 128B | Variable msgs |

## Patterns

| Pattern | Producers | Consumers | Performance |
|---------|-----------|-----------|-------------|
| SPSC | 1 | 1 | Fastest |
| MPSC | N | 1 | CAS on write |
| SPMC | 1 | N | CAS + completion |
| MPMC | N | N | Full CAS |

## Memory Ordering

```rust
// Producer: write then Release
producer_cursor.store(next, Release);

// Consumer: Acquire then read
let avail = producer_cursor.load(Acquire);
```

## Cache Optimization

128-byte padding between cursors prevents false sharing:
```rust
#[repr(align(128))]
struct PaddedAtomicU64(AtomicU64);
```

## File Structure

```
kaos/src/disruptor/
├── mod.rs         Exports
├── slots.rs       Slot8/16/32/64
├── single.rs      SPSC (RingBuffer, BroadcastRingBuffer)
├── multi.rs       MPSC, SPMC, MPMC
├── completion.rs  Multi-consumer tracking
├── ipc.rs         SharedRingBuffer (mmap)
└── macros.rs      publish_batch!, consume_batch!
```

# Design

## Overview

Flux is a lock-free ring buffer library implementing the LMAX Disruptor pattern for high-throughput inter-thread communication.

## Core Concepts

### Ring Buffer

A fixed-size circular buffer where:
- Size must be a power of 2 (for fast modulo via bitwise AND)
- Producer writes to slots, advances cursor
- Consumer reads from slots, follows producer cursor
- Wraps around when reaching the end

### Sequence Numbers

64-bit sequences that:
- Monotonically increase (never wrap in practice)
- Map to buffer index via `sequence & (size - 1)`
- Producer cursor: last published sequence
- Consumer cursor: last consumed sequence

### Slot Types

| Type | Size | Use Case |
|------|------|----------|
| `SmallSlot` | 8B | Sequence only, max throughput |
| `Slot16` | 16B | Sequence + small payload |
| `Slot32` | 32B | Sequence + medium payload |
| `Slot64` | 64B | Cache-line aligned |
| `MessageSlot` | 128B | Variable-length messages |

All slots implement `RingBufferEntry`:
```rust
pub trait RingBufferEntry: Clone + Default + Send + Sync {
    fn sequence(&self) -> u64;
    fn set_sequence(&mut self, seq: u64);
}
```

## Concurrency Patterns

### SPSC (Single Producer Single Consumer)

The simplest and fastest pattern:
- No CAS operations needed
- Producer uses local cursor + relaxed atomics
- Consumer uses acquire/release ordering

### MPSC (Multiple Producer Single Consumer)

Multiple producers compete for write slots:
- Producers use CAS on producer cursor
- Consumer remains lock-free

### SPMC (Single Producer Multi Consumer)

Single producer, multiple consumers compete for reads:
- Producer remains lock-free
- Consumers use CAS + completion tracking
- Uses read-then-commit pattern

### MPMC (Multi Producer Multi Consumer)

Full concurrency on both sides:
- Producers use CAS for write slots
- Consumers use CAS + completion tracking
- Maximum flexibility, lower throughput

## Memory Ordering

### Producer (SPSC)

```rust
// 1. Check space available (Relaxed read of consumer)
// 2. Write slot data
// 3. Publish with Release ordering
producer_cursor.store(next, Ordering::Release);
```

### Consumer (SPSC)

```rust
// 1. Read producer cursor with Acquire
let available = producer_cursor.load(Ordering::Acquire);
// 2. Read slot data (after Acquire fence)
// 3. Update consumer cursor (Relaxed - single consumer)
```

### Multi-Producer/Consumer

Uses `compare_exchange_weak` with appropriate ordering:
- Acquire on success (to see prior writes)
- Relaxed on failure (will retry anyway)

## Batch Processing

Batching amortizes atomic operation overhead:

```rust
// One atomic check for N slots
if let Some((seq, slots)) = ring.try_claim_slots(8192, cursor) {
    for slot in slots { /* write */ }
    ring.publish(seq + 8192);  // One atomic publish
}
```

## Memory Allocation

### Heap (Default)

Standard `Box<[T]>` allocation:
- Simple and portable
- May have page faults on first access

### Memory-Mapped (mlock)

Uses `mmap` + `mlock`:
- Pages locked in RAM
- No page faults in hot path
- Better for real-time workloads

## Cache Optimization

### Padding

128-byte padding between cursors prevents false sharing:
```rust
#[repr(align(128))]
struct PaddedAtomicU64 {
    value: AtomicU64,
}
```

### Prefetching

Hardware prefetch hints for upcoming slots:
```rust
#[cfg(target_arch = "x86_64")]
_mm_prefetch(ptr, _MM_HINT_T0);

#[cfg(target_arch = "aarch64")]
asm!("prfm pldl1keep, [{0}]", in(reg) ptr);
```

## Safety

The library uses `unsafe` for:
- Volatile reads/writes (prevent compiler reordering)
- Raw pointer arithmetic (slice access)
- Send/Sync implementations

Safety invariants:
- Producer must claim before writing
- Consumer must wait for publish before reading
- Respect the concurrency pattern constraints

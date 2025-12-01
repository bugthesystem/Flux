# Flux

Lock-free ring buffers for inter-thread, inter-process, and network communication.

> **⚠️ Preview Release (0.1.0-preview)**
>
> This library is under active development. APIs may change between versions.
> Uses `unsafe` for performance-critical memory operations. Review before production use.
> Currently tested on macOS ARM64 only.

## Crates

| Crate | Purpose | Throughput* |
|-------|---------|-------------|
| [flux](./flux) | Inter-thread ring buffer | 2.0 B/s |
| [flux-ipc](./flux-ipc) | Shared memory IPC | 310 M/s |
| [flux-rudp](./flux-rudp) | Reliable UDP | 3.0 M/s |

*Criterion benchmarks, Apple M1, macOS 14. Run `cargo bench` to verify on your hardware.*

## Design

Based on the [LMAX Disruptor](https://lmax-exchange.github.io/disruptor/) pattern:

- **Single-writer sequences** with atomic visibility
- **Pre-allocated ring buffers** to avoid runtime allocation
- **Cache-line alignment** (128 bytes on Apple Silicon, 64 on x86) to prevent false sharing
- **Batch operations** to amortize synchronization overhead

### flux: Core Ring Buffer

```
Producer                              Consumer
   │                                     │
   ▼                                     ▼
┌──────────────────────────────────────────────────────┐
│  producer_cursor ──────────────────▶ consumer_cursor │
│       (AtomicU64)                      (AtomicU64)   │
├──────────────────────────────────────────────────────┤
│  [slot 0][slot 1][slot 2]...[slot N-1]  (power of 2) │
│     ▲                           ▲                    │
│     │                           │                    │
│   write                       read                   │
└──────────────────────────────────────────────────────┘
```

**Key optimizations:**
- **Sequence-based coordination**: No locks, just atomic sequence numbers
- **Batch claiming**: `try_claim_slots(count, cursor)` reserves multiple slots atomically
- **Direct slice access**: `get_read_batch()` returns `&[T]` pointing directly to buffer memory (no allocation, no memcpy)
- **Memory-mapped option**: `new_mapped()` uses `mmap` + `mlock` for page-aligned buffers

## Architecture

### flux-ipc: Shared Memory Transport

Cross-process communication using memory-mapped ring buffers.

```
Process A (Publisher)              Process B (Subscriber)
┌─────────────────────┐            ┌─────────────────────┐
│  Publisher::send()  │            │ Subscriber::recv()  │
└─────────┬───────────┘            └─────────┬───────────┘
          │                                  │
          ▼                                  ▼
┌─────────────────────────────────────────────────────────┐
│                  Shared Memory File                     │
│  ┌─────────────────────────────────────────────────┐    │
│  │ Header: producer_seq | consumer_seq (AtomicU64) │    │
│  ├─────────────────────────────────────────────────┤    │
│  │ Ring Buffer: [slot 0][slot 1]...[slot N-1]      │    │
│  └─────────────────────────────────────────────────┘    │
└─────────────────────────────────────────────────────────┘
```

**How it works:**
1. Publisher creates a file-backed mmap region with `MAP_SHARED`
2. Header contains atomic sequences for coordination (in shared memory)
3. Subscriber opens the same file and attaches to the region
4. Lock-free SPSC protocol: publisher writes slots, updates sequence; subscriber reads when sequence advances
5. No syscalls in hot path—just atomic loads/stores on shared memory

### flux-rudp: Reliable UDP Transport

NAK-based reliable delivery over UDP, inspired by [Aeron](https://github.com/real-logic/aeron).

```
Sender                                      Receiver
┌──────────────┐                           ┌──────────────┐
│ send_batch() │──── UDP Datagrams ───────▶│ recv_batch() │
│              │                           │              │
│ send_window  │◀─────── ACK ──────────────│ recv_window  │
│ (keeps msgs) │                           │ (reorders)   │
│              │◀─────── NAK ──────────────│              │
│ retransmit() │                           │ gap detect   │
└──────────────┘                           └──────────────┘
```

**Protocol:**
1. **Send window** retains messages until ACKed (not fire-and-forget)
2. **Receiver** delivers in-order, detects gaps via bitmap window
3. **NAK** (Negative Acknowledgment) requests retransmission of missing sequences
4. **ACK** confirms highest delivered sequence, allowing sender to free buffer space
5. **Batch operations** pack multiple messages per UDP datagram

**Reliability guarantees:**
- 100% delivery on localhost (verified in benchmarks)
- In-order delivery via sequence tracking
- Automatic retransmission on packet loss

## Safety

This library uses `unsafe` in the following areas:

- **Memory-mapped allocation** (`mmap`, `mlock`) for page-aligned buffers
- **Atomic operations** with explicit memory ordering
- **Raw pointer arithmetic** for zero-copy slice access
- **Shared memory** (`MAP_SHARED`) for cross-process communication

All unsafe blocks are documented with safety invariants.

## Benchmarks

```bash
# Core ring buffer
cargo bench -p flux --bench bench_criterion

# IPC
cargo bench -p flux-ipc --bench bench_ipc

# RUDP
cargo bench -p flux-rudp --bench bench_rudp
```

| Component | Metric | Result |
|-----------|--------|--------|
| flux (8B slot) | Batch throughput | 2.0 B/s |
| flux (64B slot) | Batch throughput | 450 M/s |
| flux (128B MessageSlot) | Batch throughput | 115 M/s |
| flux-ipc | Single message | 137 M/s |
| flux-ipc | Sustained batch | 310 M/s |
| flux-rudp | Localhost (100% delivery) | 3.0 M/s |

### Real-World Scenario

Mobile user interaction tracking (click, scroll, pageview, purchase, login):

| Crate | Events | Throughput | Verified |
|-------|--------|------------|----------|
| flux | 5 Billion | 408 M/s | ✅ |
| flux-ipc | 1 Billion | 224 M/s | ✅ |
| flux-rudp | 500K | 3.0 M/s | ✅ 100% delivery |

```bash
cargo bench -p flux --bench bench_trace_events
cargo bench -p flux-ipc --bench bench_trace_events
cargo bench -p flux-rudp --bench bench_trace_events
```

### Comparison vs disruptor-rs

Same trace events benchmark (10M events):

| Library | Throughput | Verified |
|---------|------------|----------|
| **flux** | **400 M/s** | ✅ |
| disruptor-rs | 391 M/s | ✅ |

```bash
cd ext-benches/disruptor-rs-bench && cargo bench --bench bench_trace_events
```

## Testing

```bash
cargo test --workspace
cargo test -p flux-test-support -- --test-threads=1
```

Test coverage: unit tests, memory ordering tests, stress tests, packet loss simulation.

## Platform Support

| Platform | Status |
|----------|--------|
| macOS ARM64 | ✅ Tested |
| macOS x86_64 | Untested |
| Linux | Untested |
| Windows | Not supported |

## Quick Start

```rust
use flux::disruptor::{RingBuffer, SmallSlot};
use std::sync::Arc;

// Create ring buffer
let ring = Arc::new(RingBuffer::<SmallSlot>::new(1024)?);

// Producer: claim slots, write, publish
let mut cursor = 0u64;
if let Some((seq, slots)) = ring.try_claim_slots(10, cursor) {
    for (i, slot) in slots.iter_mut().enumerate() {
        slot.value = i as u64;
    }
    ring.publish(seq + slots.len() as u64 - 1);
    cursor = seq + slots.len() as u64;
}

// Consumer: read and advance
let slots = ring.get_read_batch(0, 10);
ring.update_consumer(9);
```

## Roadmap

**Platform:**
- Linux testing and CI
- `sendmmsg`/`recvmmsg` for batched UDP syscalls (Linux)

**True Zero-Copy Network I/O (Linux):**
- `io_uring` with registered buffers — kernel writes directly to user memory
- `AF_XDP` sockets — bypass kernel network stack entirely
- `MSG_ZEROCOPY` flag for send path (Linux 4.14+)

**Protocol:**
- Congestion control (CUBIC) for RUDP
- Flow control with receiver feedback

## License

MIT OR Apache-2.0

## Acknowledgments

- [LMAX Disruptor](https://lmax-exchange.github.io/disruptor/)
- [Aeron](https://github.com/real-logic/aeron)

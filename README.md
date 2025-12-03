# Kaos

<p align="center">
  <img src="logo.svg" alt="Kaos Logo" width="120" height="120"/>
</p>

<p align="center">
  <strong>High-performance lock-free messaging for Rust</strong>
</p>

<p align="center">
  <a href="#features">Features</a> •
  <a href="#performance">Performance</a> •
  <a href="#quick-start">Quick Start</a> •
  <a href="#architecture">Architecture</a>
</p>

---

## Overview

Kaos provides lock-free ring buffers for inter-thread, inter-process, and network communication. Built on the [LMAX Disruptor](https://lmax-exchange.github.io/disruptor/) and [Aeron](https://github.com/real-logic/aeron) high performance networking patterns with modern Rust.

> **Note:** Preview release. Uses `unsafe` perf critical paths (soon to be optional with `safe` and `unsafe` variants) and APIs may change.

## Crates

| Crate | Description |
|-------|-------------|
| **[kaos](./kaos)** | Lock-free ring buffers (SPSC, MPSC, SPMC, MPMC) |
| **[kaos-ipc](./kaos-ipc)** | Shared memory IPC via mmap |
| **[kaos-rudp](./kaos-rudp)** | Reliable UDP with NAK/ACK |
| **[kaos-driver](./kaos-driver)** | Media driver for zero-syscall I/O |

## Features

| Category | Feature | Status |
|----------|---------|--------|
| **Core** | Lock-free ring buffers | ✅ |
| | SPSC, MPSC, SPMC, MPMC | ✅ |
| | Batch operations | ✅ |
| **IPC** | Shared memory (mmap) | ✅ |
| | Zero-copy reads | ✅ |
| **Network** | Reliable UDP | ✅ |
| | Congestion control (AIMD) | ✅ |
| **Linux** | sendmmsg/recvmmsg | ✅ |
| | io_uring | ✅ |
| | AF_XDP kernel bypass | ✅ |
| **Observability** | Metrics | ✅ |

## Performance

**Run on your hardware** — numbers vary by CPU, load, and thermals.

Reference: Apple M1, macOS 14, 100M trace events, SPSC, verified.

### Ring Buffer Comparison

| Library | Throughput | Language |
|---------|------------|----------|
| **Kaos** | **400 M/s** | Rust |
| disruptor-rs | 437 M/s | Rust |
| LMAX Disruptor | 140 M/s | Java |

### Kaos Components

| Component | Throughput | Latency |
|-----------|------------|---------|
| Ring buffer (batch) | 2.5 Gelem/s | — |
| IPC (mmap) | 412 M/s | 2.4 ns |
| Reliable UDP | 3 M/s | — |

### Full Stack Comparison (IPC via Media Driver)

| Library | IPC Throughput | Language |
|---------|----------------|----------|
| **Kaos** | **254 M/s** | Rust |
| Aeron | 18.7 M/s | Java |

*Both test app → media driver → shared memory → consumer.*

```bash
# Run benchmarks
cargo bench -p kaos --bench bench_trace -- "100M"
cd ext-benches/disruptor-rs-bench && cargo bench --bench bench_trace_events
cd ext-benches/disruptor-java-bench && mvn compile -q && \
  java -cp "target/classes:$(mvn dependency:build-classpath -q -Dmdep.outputFile=/dev/stdout)" \
  com.kaos.TraceEventsBenchmark
```

## Quick Start

```rust
use kaos::disruptor::{RingBuffer, Slot8};

// Create ring buffer
let ring = RingBuffer::<Slot8>::new(1024)?;

// Producer: claim, write, publish
if let Some((seq, slots)) = ring.try_claim_slots(10, cursor) {
    for (i, slot) in slots.iter_mut().enumerate() {
        slot.value = i as u64;
    }
    ring.publish(seq + slots.len() as u64 - 1);
}

// Consumer: read, advance
let slots = ring.get_read_batch(0, 10);
ring.update_consumer(9);
```

## Architecture

```
┌─────────────────────────────────────────────────────────────┐
│                        APPLICATION                          │
│            Producer ───► Ring Buffer ───► Consumer          │
└─────────────────────────────┬───────────────────────────────┘
                              │ Shared Memory (2.4 ns)
┌─────────────────────────────▼───────────────────────────────┐
│                       MEDIA DRIVER                          │
│       sendmmsg  │  io_uring  │  AF_XDP  │  Reliable UDP     │
└─────────────────────────────┬───────────────────────────────┘
                              │
┌─────────────────────────────▼───────────────────────────────┐
│                         NETWORK                             │
└─────────────────────────────────────────────────────────────┘
```

## Testing

```bash
# Unit tests
cargo test --workspace

# Loom concurrency verification (exhaustive state exploration)
RUSTFLAGS="--cfg loom" cargo test -p kaos --test loom_ring_buffer --release

# Memory analysis (macOS)
leaks --atExit -- ./target/release/examples/spsc_basic

# Memory analysis (Linux)
cargo valgrind run --example spsc_basic -p kaos --release
```

**Loom** tests verify lock-free correctness by exploring all possible thread interleavings. See [kaos/tests/loom_ring_buffer.rs](./kaos/tests/loom_ring_buffer.rs).

**Profiling** guide with flamegraphs, valgrind, leaks and Instruments: [kaos/docs/PROFILING.md](./kaos/docs/PROFILING.md)

## Platform Support

| Platform | Status |
|----------|--------|
| macOS ARM64 | ✅ Tested |
| Linux x86_64 | ✅ Tested |
| Windows | Not supported |

## Design Principles

- **Lock-free** — Atomic sequences, no mutex contention
- **Zero-copy reads** — Consumers get direct slice access (writes copy to buffer)  
- **Cache-aligned** — 128-byte padding prevents false sharing
- **Batch operations** — Amortize synchronization overhead

## License

MIT OR Apache-2.0

---

<p align="center">
  Inspired by <a href="https://lmax-exchange.github.io/disruptor/">LMAX Disruptor</a> and <a href="https://github.com/real-logic/aeron">Aeron</a>
</p>

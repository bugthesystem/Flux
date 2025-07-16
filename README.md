# Flux - High-Performance Message Transport

[![Rust](https://img.shields.io/badge/rust-1.70%2B-brightgreen.svg)](https://www.rust-lang.org/)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](https://opensource.org/licenses/MIT)

Flux is a high-performance message transport library for Rust, implementing LMAX Disruptor patterns with zero-copy memory management and lock-free operations.

## Architecture

```
┌─────────────────────────────────────────────────────────────────┐
│                        Flux Architecture                       │
├─────────────────────────────────────────────────────────────────┤
│                                                               │
│  ┌─────────────┐    ┌─────────────┐    ┌─────────────┐      │
│  │  Producer   │    │  Producer   │    │  Producer   │      │
│  │   (P1)      │    │   (P2)      │    │   (P3)      │      │
│  └─────┬───────┘    └─────┬───────┘    └─────┬───────┘      │
│        │                  │                  │               │
│        └──────────────────┼──────────────────┘               │
│                           │                                  │
│                    ┌──────▼──────┐                           │
│                    │ Ring Buffer │  ← Lock-free, zero-copy   │
│                    │ (1M slots)  │     cache-line aligned    │
│                    └──────┬──────┘                           │
│                           │                                  │
│        ┌──────────────────┼──────────────────┐               │
│        │                  │                  │               │
│  ┌─────▼──────┐    ┌─────▼──────┐    ┌─────▼──────┐        │
│  │  Consumer  │    │  Consumer  │    │  Consumer  │        │
│  │   (C1)     │    │   (C2)     │    │   (C3)     │        │
│  └────────────┘    └────────────┘    └────────────┘        │
│                                                               │
│  ┌─────────────────────────────────────────────────────────┐   │
│  │              Performance Optimizations                 │   │
│  │  • SIMD (NEON/AVX2) data copy                        │   │
│  │  • Hardware CRC32 (ARM/x86)                           │   │
│  │  • NUMA-aware allocation (Linux)                      │   │
│  │  • Huge pages + io_uring (Linux)                      │   │
│  │  • Cache-line padding + prefetching                   │   │
│  │  • Batch processing (8K-128K slots)                   │   │
│  └─────────────────────────────────────────────────────────┘   │
│                                                               │
│  Throughput: 38M msg/sec (Apple Silicon)                    │
│  Latency: <1μs (cache-local operations)                     │
│                                                               │
└─────────────────────────────────────────────────────────────────┘
```

## Getting Started (Developer Friendly)

Flux is easy to use for basic message passing. Here’s a minimal example:

```rust
use flux::disruptor::{RingBuffer, RingBufferConfig, WaitStrategyType};

// Create a ring buffer with 1M slots and 1 consumer
let config = RingBufferConfig::new(1024 * 1024)
    .unwrap()
    .with_consumers(1)
    .unwrap()
    .with_wait_strategy(WaitStrategyType::BusySpin);

let buffer = RingBuffer::new(config).unwrap();

// Producer: claim and publish a batch
if let Some((seq, slots)) = buffer.try_claim_slots(10) {
    for (i, slot) in slots.iter_mut().enumerate() {
        slot.set_sequence(seq + i as u64);
        slot.set_data(b"Hello, Flux!");
    }
    buffer.publish_batch(seq, slots.len());
}

// Consumer: read a batch
let messages = buffer.try_consume_batch(0, 10);
for message in messages {
    println!("Received: {:?}", message.data());
}
```

## Low-Level, High-Performance Usage

For maximum throughput, use the memory-mapped or Linux-optimized ring buffer and large batch sizes:

```rust
use flux::disruptor::{RingBufferConfig, ring_buffer::MappedRingBuffer};

let config = RingBufferConfig::new(1024 * 1024)
    .unwrap()
    .with_consumers(4)
    .unwrap()
    .with_optimal_batch_size(8192);

let buffer = MappedRingBuffer::new_mapped(config).unwrap();

// Producer: claim a large batch for high throughput
if let Some((seq, slots)) = buffer.try_claim_slots(8192) {
    for (i, slot) in slots.iter_mut().enumerate() {
        slot.set_sequence(seq + i as u64);
        // Use SIMD-optimized data copy for best performance
        slot.set_data(b"Ultra-fast message");
    }
    buffer.publish_batch(seq, slots.len());
}
```

On Linux, enable all optimizations:

```bash
cargo build --release --features linux_optimized
```

And use the `LinuxRingBuffer` for NUMA, huge pages, and affinity.

## Performance

> **Recent Performance Milestone (June 2024):**
> Flux achieved **11.33 million messages/second** on Apple Silicon (8 consumers, 64-byte messages, 64K batch, 10s run) using aggressive batching, zero-copy slices, and thread affinity. [See full details.](docs/performance.md)

Flux is designed for extreme throughput and low latency. Here are real numbers from our benchmarks:

| Test Scenario                | Platform         | Throughput         | Notes                        |
|------------------------------|------------------|--------------------|------------------------------|
| Minimal (no validation)      | Apple Silicon    | 173M msg/sec       | Raw memory bandwidth limit   |
| Realistic (64B messages)     | Apple Silicon    | 38M msg/sec        | SIMD, batching, zero-copy    |
| Realistic (Linux, NUMA, HP)  | x86_64 Linux     | 50-100M msg/sec    | NUMA, huge pages, affinity   |
| UDP Transport (in-memory)    | Apple Silicon    | 10-20M msg/sec     | Reliable UDP, batching       |

> **We have a play here:** Flux matches or exceeds Aeron/Disruptor in-memory throughput on modern hardware, and is ready for production use in high-throughput, low-latency systems.

## Features

- Lock-free ring buffer with single-writer, multiple-reader semantics
- Zero-copy memory management with cache-line aligned data structures
- Batch processing for amortized atomic operations
- Platform-specific optimizations for Linux and macOS
- Reliable UDP transport with NAK-based retransmission
- Comprehensive error handling and monitoring

## Installation

Add to your `Cargo.toml`:

```toml
[dependencies]
flux = "0.1.0"
```

## Platform Support

### Linux
- Full feature set including NUMA awareness and huge pages
- Kernel bypass I/O with io_uring support
- Thread affinity and real-time scheduling

### macOS
- Core functionality with Apple Silicon optimizations
- NEON SIMD acceleration
- P-core CPU pinning

## Examples

```bash
# Basic usage
cargo run --example basic_usage

# UDP transport
cargo run --example udp_transport

# Linux optimizations
cargo run --example linux_ultra_bench --features linux_optimized

# macOS optimizations
cargo run --example realistic_high_throughput
```

## Benchmarks

```bash
# Performance benchmarks
cargo run --release --bin extreme_bench
cargo run --release --bin macos_ultra_bench
```

## Documentation

- [Architecture](./docs/architecture.md)
- [Getting Started](./docs/getting-started.md)
- [Performance](./docs/performance.md)
- [Platform Setup](./docs/platform-setup.md)
- [Linux Optimizations](./docs/linux_optimizations.md)

## Contributing

Contributions are welcome. Please read our contributing guidelines and ensure all tests pass before submitting a pull request.

## License

This project is licensed under the MIT License - see the [LICENSE](./LICENSE) file for details. 
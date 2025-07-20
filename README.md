# Flux - High-Performance Message Transport

[![Rust](https://img.shields.io/badge/rust-1.70%2B-brightgreen.svg)](https://www.rust-lang.org/)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](https://opensource.org/licenses/MIT)

Flux is a high-performance message transport library (IPC, UDP, Reliable UDP) for Rust, implementing LMAX Disruptor patterns with zero-copy memory management and lock-free operations.

> Optimizations still in progress, the readme will be updated as progress is made
> 
>  Known issues atm:
> - /docs/architecture.md needs to be updated to reflect the current state (code blocks are outdated)
> - better alignment (64, 128, depending on CPUs) to fully prevent false-sharing
> - cover resilience concerns 
> - review safety concerns (`unsafe` in use there and there)

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
└─────────────────────────────────────────────────────────────────┘
```

## Getting Started

Flux is easy to use for basic message passing. Here's a minimal example:

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

// Consumer: read a batch,
let messages = buffer.try_consume_batch(0, 10);
for message in messages {
    println!("Received: {:?}", message.data());
}
```

## High-Performance Usage

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

## UDP & Reliable UDP Transport

Flux provides high-performance UDP transport with zero-copy operations and reliable delivery:

### Basic UDP Transport

```rust
use flux::{BasicUdpTransport, BasicUdpConfig};

// Configure basic UDP transport
let config = BasicUdpConfig {
    local_addr: "0.0.0.0:8080".to_string(),
    buffer_size: 4096,
    batch_size: 64,
    non_blocking: true,
    socket_timeout_ms: 100,
};

let mut transport = BasicUdpTransport::new(config)?;
transport.start()?;

// Send messages
transport.send(b"Hello, Flux!", addr)?;

// Receive messages
if let Some((data, _addr)) = transport.receive()? {
    println!("Received: {:?}", data);
}
```

### Zero-Copy UDP Transport

```rust
use flux::transport::zero_copy_udp::{ZeroCopyUdpTransport, ZeroCopyConfig};

// Configure for high-performance UDP
let config = ZeroCopyConfig {
    batch_size: 1000,
    buffer_size: 1024 * 1024,
    socket_buffer_mb: 64,
    busy_poll: true,
    ..Default::default()
};

let transport = ZeroCopyUdpTransport::new(config)?;
transport.start()?;

// Send high-throughput batches
let messages = vec![b"Hello"; 1000];
transport.send_batch(&messages.iter().map(|m| m.as_slice()).collect::<Vec<_>>(), addr)?;
```

### Reliable UDP with NAK-based Retransmission

```rust
use flux::transport::{ReliableUdpTransport, TransportConfig};

let config = TransportConfig {
    batch_size: 64,
    buffer_size: 1024 * 1024,
    retransmit_timeout_ms: 100,
    max_retransmits: 3,
    enable_fec: true,  // Forward Error Correction
    ..Default::default()
};

let mut transport = ReliableUdpTransport::new(config)?;
transport.start()?;

// Send with automatic retransmission and FEC
transport.send(b"Reliable message", addr)?;

// Receive with guaranteed delivery
if let Some((data, _addr)) = transport.receive()? {
    println!("Received: {:?}", data);
}
```

**Performance:** 5M+ messages/sec for zero-copy UDP, 1M+ messages/sec for reliable UDP with full error correction.

## Performance

Flux is designed for high throughput and low latency. Here are example numbers from our benchmarks:

| Test Scenario                | Platform         | Throughput         | Notes                        |
|------------------------------|------------------|--------------------|------------------------------|
| Minimal (no validation)      | Apple Silicon    | 173M msg/sec       | Raw memory bandwidth limit   |
| Realistic (64B messages)     | Apple Silicon    | 38M msg/sec        | SIMD, batching, zero-copy    |
| Realistic (Linux, NUMA, HP)  | x86_64 Linux     | 50-100M msg/sec    | NUMA, huge pages, affinity   |
| UDP Transport (in-memory)    | Apple Silicon    | 10-20M msg/sec     | Reliable UDP, batching       |

**Note:** Performance varies significantly based on:
- **Configuration**: Batch size, buffer size, number of consumers
- **Validation**: Whether message validation/checksums are enabled
- **Hardware**: CPU cores, memory bandwidth, cache size
- **Optimizations**: SIMD, memory mapping, CPU affinity

Flux matches or exceeds Aeron/Disruptor in-memory throughput on modern hardware, and is ready for production use in high-throughput, low-latency systems.

## Safety

Flux uses unsafe code for performance-critical operations like SIMD operations and memory mapping. All unsafe code is:

- **Carefully documented** with safety explanations
- **Bounded by safety checks** to prevent undefined behavior  
- **Thoroughly tested** for edge cases and race conditions
- **Performance-justified** with measurable benefits

See [SAFETY.md](./SAFETY.md) for comprehensive documentation of all unsafe code patterns.

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
cargo run --example example_basic_usage

# UDP transport with ring buffers
cargo run --example example_udp_transport

# Zero-copy UDP transport
cargo run --example example_udp_transport --features io-uring

# Linux optimizations
cargo run --example example_linux_optimized --features linux_optimized

# Realistic high throughput
cargo run --example example_realistic_validated
```

## Benchmarks

```bash
# Performance benchmarks
cargo run --release --bin bench_extreme
cargo run --release --bin bench_macos_ultra_optimized
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

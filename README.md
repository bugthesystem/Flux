# Flux - High-Performance Message Transport
> **Research Preview** 🧪  
> Pre-Production - Breaking Changes Expected

[![Rust](https://img.shields.io/badge/rust-1.70%2B-brightgreen.svg)](https://www.rust-lang.org/)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](https://opensource.org/licenses/MIT)

Flux is a high-performance message transport library for Rust that implements patterns inspired by LMAX Disruptor and Aeron. It provides lock-free inter-process communication (IPC), UDP transport, and reliable UDP with optimized memory management for applications with low-latency requirements.

> **Development Status**: This library is under active development with ongoing optimizations. Performance characteristics and APIs are subject to change as the implementation matures.

## Key Features

**Core Messaging**
- Lock-free ring buffer with single-writer, multiple-reader semantics
- Batch processing for amortized atomic operations
- Cache-line aligned data structures for optimal memory access patterns
- Support for 1M+ slot ring buffers with microsecond latencies

**Transport Layer**
- Unified UDP transport with ring buffer integration for high throughput
- Reliable UDP with NAK-based retransmission and optional forward error correction (XOR-based)
- 🚧 **(In Progress)** Kernel bypass zero-copy with io_uring on Linux

**Platform Optimizations**
- **Linux**: NUMA awareness, huge pages, real-time scheduling support
- **macOS**: Apple Silicon optimizations with thread priority tuning
- Cross-platform memory mapping and compiler auto-vectorization

## Architecture

```
┌─────────────────────────────────────────────────────────────────┐
│                        Flux Architecture                        │
├─────────────────────────────────────────────────────────────────┤
│  ┌─────────────┐    ┌─────────────┐    ┌─────────────┐          │
│  │  Producer   │    │  Producer   │    │  Producer   │          │
│  │     P1      │    │     P2      │    │     P3      │          │
│  └─────┬───────┘    └─────┬───────┘    └─────┬───────┘          │
│        │                  │                  │                  │
│        └──────────────────┼──────────────────┘                  │
│                           │                                     │
│                    ┌──────▼──────┐                              │
│                    │ Ring Buffer │  ← Lock-free, cache-aligned  │
│                    │ (1M slots)  │                              │
│                    └──────┬──────┘                              │
│                           │                                     │
│        ┌──────────────────┼──────────────────┐                  │
│        │                  │                  │                  │
│  ┌─────▼──────┐    ┌─────▼──────┐    ┌─────▼──────┐             │
│  │  Consumer  │    │  Consumer  │    │  Consumer  │             │
│  │     C1     │    │     C2     │    │     C3     │             │
│  └────────────┘    └────────────┘    └────────────┘             │
│                                                                 │
│  ┌─────────────────────────────────────────────────────────┐    │
│  │              Performance Optimizations                  │    │
│  │  • Memory-mapped ring buffers                           │    │
│  │  • Hardware CRC32 (ARM/x86)                             │    │
│  │  • NUMA-aware allocation (Linux)                        │    │
│  │  • Cache-line padding and prefetching                   │    │
│  │  • Batch processing (configurable sizes)                │    │
│  └─────────────────────────────────────────────────────────┘    │
└─────────────────────────────────────────────────────────────────┘
```

## Quick Start

### Basic Ring Buffer Usage

```rust
use flux::disruptor::{RingBuffer, RingBufferConfig, WaitStrategyType};

// Create a ring buffer with 1M slots
let config = RingBufferConfig::new(1024 * 1024)?
    .with_consumers(1)?
    .with_wait_strategy(WaitStrategyType::BusySpin);

let buffer = RingBuffer::new(config)?;

// Producer: claim and publish messages
if let Some((seq, slots)) = buffer.try_claim_slots(10) {
    for (i, slot) in slots.iter_mut().enumerate() {
        slot.set_sequence(seq + i as u64);
        slot.set_data(b"Hello, Flux!");
    }
    buffer.publish_batch(seq, slots.len());
}

// Consumer: read messages
let messages = buffer.try_consume_batch(0, 10);
for message in messages {
    println!("Received: {:?}", message.data());
}
```

### Basic UDP Transport Usage

```rust
use flux::{UdpRingBufferTransport, UdpTransportConfig};

let config = UdpTransportConfig {
    local_addr: "0.0.0.0:8080".to_string(),
    buffer_size: 4096, // Use a power of two for best performance
    batch_size: 64,
    non_blocking: true,
    socket_timeout_ms: 100,
};

let mut transport = UdpRingBufferTransport::new(config)?;
transport.start()?;

// Send and receive messages
transport.send(b"Message", addr)?;
if let Some((data, _addr)) = transport.receive()? {
    println!("Received: {:?}", data);
}
```

## Platform-Specific Ring Buffer Implementations

- **RingBuffer**: Default, cross-platform, in-memory, lock-free. Use for most scenarios.
- **MappedRingBuffer**: Memory-mapped, for zero-copy or IPC. Unix-like systems (Linux/macOS/BSD).
- **LinuxRingBuffer**: Linux-only, NUMA/hugepage/affinity optimized. Use for maximum performance on Linux.

## Performance Characteristics

> **Note**: These benchmarks represent preliminary results from development hardware (Apple Silicon M1). Production performance will vary based on hardware, configuration, and workload characteristics.

## IPC Ring Buffer Performance

| Configuration | Platform | Throughput | Notes |
|---------------|----------|------------|-------|
| Multi-producer peak | Apple M1 | 30.8M msgs/sec | Optimized batch processing |
| Single producer | Apple M1 | 25.2M msgs/sec | Sustained high throughput |
| Realistic workload | Apple M1 | 15.8M msgs/sec | Production-like conditions |
| With validation | Apple M1 | 2.08M msgs/sec | Full integrity checking |

## Network Transport Performance

| Transport                        | Throughput (M msgs/sec) | Success Rate | Notes                        |
|----------------------------------|-------------------------|--------------|------------------------------|
| Basic UDP                        | 0.23                    | 100%         | Fastest, no reliability      |
| UDP Ring Buffer Transport        | 0.22                    | 100%         | High-perf, no reliability    |
| Reliable UDP (NAK, RingBuffer)   | 0.21                    | 100%         | Fastest reliable, hybrid window [1] |
| Reliable UDP (NAK, BTreeMap)     | 0.19                    | 100%         | Benchmark-only, sparse-friendly |

 **Notes:**
- [1] The hybrid window (ring buffer + btreemap) achieves the best of both worlds: fast in-order delivery and robust out-of-order handling.
- See the `HybridWindow` implementation for details.
> **⚠️ HEADS UP!** The `BTreeMap-based NAK transport` exists only for benchmark comparison and is not part of the main library API.

**Performance Factors**:
- Batch size and buffer configuration significantly impact throughput
- Memory mapping and cache-line alignment provide 10-20% improvements
- Platform-specific optimizations can yield additional performance gains

## Installation

Add to your `Cargo.toml`:

```toml
[dependencies]
flux = "0.1.0"

# Enable platform-specific optimizations
[target.'cfg(target_os = "linux")'.dependencies]
flux = { version = "0.1.0", features = ["linux_optimized"] }
```

## Platform Support

### Linux (Primary Target)
- ✅ Full ring buffer functionality
- ✅ UDP transport implementations (ring buffer-based)
- ✅ NUMA awareness and thread affinity
- 🚧 **(In Progress)** Huge pages support (development active)
- 🚧 **(In Progress)** io_uring zero-copy integration

### macOS
- ✅ Ring buffer with Apple Silicon optimizations
- ✅ UDP transport with compiler auto-vectorization
- ✅ Thread priority optimization
- ⚠️ Limited thread affinity (Apple Silicon restrictions)

### Windows
- 🚧 Basic functionality planned (not yet implemented)

## Safety and Reliability

Flux uses `unsafe` code in performance-critical paths for:
- Memory-mapped buffer operations
- Cache-line aligned memory access
- Low-level memory copying optimizations

**Safety Measures**:
- `unsafe` code is documented with safety justifications
- Comprehensive bounds checking prevents buffer overruns
- Memory barriers ensure proper ordering in concurrent scenarios
- Extensive test coverage for edge cases and race conditions

See [SAFETY.md](./SAFETY.md) for detailed documentation of all unsafe code usage.

## Examples and Benchmarks

Below is a table of available examples and benchmarks. For best performance, always use `--release`.

### Examples
| Name                              | Description                                 | Command to Run (Release)                                 |
|------------------------------------|---------------------------------------------|----------------------------------------------------------|
| example_basic_usage                | Ring buffer, batch, producer-consumer demo  | `cargo run --release --example example_basic_usage`      |
| example_udp_transport              | Basic UDP message transport                 | `cargo run --release --example example_udp_transport`    |
| example_udp_average                | UDP, client sends numbers, server averages  | `cargo run --release --example example_udp_average`      |
| example_reliable_udp_transport     | Reliable UDP echo test                      | `cargo run --release --example example_reliable_udp_transport` |
| example_reliable_udp_batch_average | Reliable UDP, batch send/receive, average   | `cargo run --release --example example_reliable_udp_batch_average` |
| example_minimal_raw                | Minimal raw ring buffer usage               | `cargo run --release --example example_minimal_raw`      |
| example_linux_optimized            | Linux-optimized ring buffer usage           | `cargo run --release --example example_linux_optimized`  |
| fec_library_test                   | FEC (Forward Error Correction) test         | `cargo run --release --example fec_library_test`         |
| example_spsc_average               | SPSC average calculator                     | `cargo run --release --example example_spsc_average`     |
| example_mpmc_average               | MPMC average calculator                     | `cargo run --release --example example_mpmc_average`     |
| example_reliable_udp_average       | Reliable UDP average calculator             | `cargo run --release --example example_reliable_udp_average` |

### Benches
| Name                              | Description                                 | Command to Run (Release)                                 |
|------------------------------------|---------------------------------------------|----------------------------------------------------------|
| bench_flux                        | General Flux benchmarks                     | `cargo bench --bench bench_flux`                         |
| bench_simple                      | Simple ring buffer benchmark                | `cargo bench --bench bench_simple`                       |
| bench_extreme                     | Extreme throughput benchmark                | `cargo run --release --bin bench_extreme`                |
| bench_macos_optimized             | macOS-optimized ring buffer benchmark       | `cargo run --release --bin bench_macos_optimized`        |
| bench_profile_analysis            | Profile analysis benchmark                  | `cargo run --release --bin bench_profile_analysis`        |
| bench_extreme_batching            | Extreme batching benchmark                  | `cargo run --release --bin bench_extreme_batching`       |
| bench_transport_comparison         | Network transport performance               | `cargo run --release --bin bench_transport_comparison`   |
| bench_ringbuffer_comparison        | Ring buffer comparison benchmark            | `cargo run --release --bin bench_ringbuffer_comparison`  |
| bench_ringbuffer_multithreaded     | Multithreaded ring buffer benchmark         | `cargo run --release --bin bench_ringbuffer_multithreaded`|
| bench_realistic_measurement        | Realistic measurement benchmark             | `cargo run --release --bin bench_realistic_measurement`  |
| bench_verified_multithread         | Verified multithreaded benchmark            | `cargo run --release --bin bench_verified_multithread`   |

## Roadmap

### Current Development (Q3 2025)
- [x] [Linux] **(In Progress)** io_uring zero-copy integration
- [x] **(In Progress)** zero or optimized copy improvements
- [ ] Stabilize reliable UDP implementation
- [ ] Windows platform support
- [ ] Comprehensive error handling and monitoring

### Future Enhancements

- [ ] Dynamic buffer sizing
- [ ] Multi-path redundancy for reliable transport
- [ ] Distributed consensus protocols

## Contributing

We welcome contributions! Please:

2. Check existing issues before creating new ones
3. Ensure all tests pass: `cargo test --all-features`
4. Run benchmarks to verify performance: `cargo run --release --bin bench_all`

## Comparison with Existing Solutions

**vs. LMAX Disruptor (Java)**: Flux implements similar lock-free ring buffer patterns with Rust's zero-cost abstractions and memory safety.

**vs. Aeron**: While Aeron focuses on network transport with 6M+ msgs/sec, Flux provides both IPC and network transports in a unified library.

**vs. Traditional Message Queues**: Eliminates broker overhead and provides predictable microsecond latencies through direct memory access.

## License

This project is licensed under the MIT License - see the [LICENSE](./LICENSE) file for details.

---

**Disclaimer**: This is experimental software under active development. While we strive for accuracy in performance claims and technical specifications, results may vary across different hardware and software configurations. Always benchmark in your specific environment before using.

## For advanced users: Optimized APIs and Cache Prefetching

Flux provides special `*_ultra` APIs (e.g., `try_claim_slots_ultra`, `publish_batch_ultra`) for expert users and high-performance scenarios:

- These APIs minimize synchronization and provide direct memory access for maximum throughput.
- They use platform-specific cache prefetch instructions (via `asm!`) to aggressively load data into CPU caches, reducing memory latency.
- Intended for benchmarks or specialized production code where you control all producer/consumer logic.
- Most users should use the standard APIs for safety and ergonomics, but `*_ultra` is available for squeezing out every last bit of performance.
- See the source in `src/disruptor/ring_buffer.rs` for details.

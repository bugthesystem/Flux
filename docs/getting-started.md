# Getting Started with Flux

Welcome to **Flux**, a high-performance message transport library built for demanding applications that require ultra-low latency and maximum throughput.

## 🎯 What is Flux?

Flux is a zero-copy, lock-free message transport library that combines:

- **LMAX Disruptor pattern** for ultra-fast producer-consumer scenarios
- **Mechanical sympathy** with CPU cache optimization
- **Reliable UDP transport** with forward error correction
- **Cross-platform support** for Linux and macOS

## 📋 Prerequisites

### System Requirements

- **Rust**: 1.70.0 or higher
- **RAM**: 8GB minimum, 16GB+ recommended for high-performance workloads
- **CPU**: Modern multi-core processor with good cache hierarchy
- **OS**: Linux (preferred) or macOS

### Development Tools

```bash
# Install Rust if not already installed
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh

# Install required tools
rustup component add clippy rustfmt
cargo install cargo-flamegraph  # For profiling (optional)
```

## 🚀 Installation

### 1. Add Flux to Your Project

```toml
[dependencies]
flux = "0.1.0"

# Optional: For async support
tokio = { version = "1.0", features = ["full"] }
```

### 2. Basic Dependencies

```toml
[dependencies]
flux = "0.1.0"
crossbeam = "0.8"
parking_lot = "0.12"
```

## 🔧 Your First Flux Application

### Basic Ring Buffer Usage

```rust
use flux::{RingBuffer, WaitStrategy, MessageSlot};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Create a ring buffer with 1024 slots
    let buffer = RingBuffer::new(1024, WaitStrategy::BusySpin);
    
    // Create producer and consumer
    let mut producer = buffer.create_producer();
    let mut consumer = buffer.create_consumer();
    
    // Producer: Write messages
    for i in 0..10 {
        let message = format!("Message {}", i);
        if let Ok(slot) = producer.try_claim_slot() {
            slot.write_message(message.as_bytes());
            producer.publish_single();
        }
    }
    
    // Consumer: Read messages
    while let Some(message) = consumer.try_consume_single() {
        println!("Received: {:?}", 
            String::from_utf8_lossy(message.data()));
    }
    
    Ok(())
}
```

### High-Performance Batch Processing

```rust
use flux::{RingBuffer, WaitStrategy};

fn high_performance_example() -> Result<(), Box<dyn std::error::Error>> {
    // Larger buffer for high throughput
    let buffer = RingBuffer::new(1024 * 1024, WaitStrategy::BusySpin);
    let mut producer = buffer.create_producer();
    let mut consumer = buffer.create_consumer();
    
    // Batch size for optimal performance
    const BATCH_SIZE: usize = 1000;
    
    // Producer: Write in batches
    loop {
        if let Ok(slots) = producer.try_claim_slots(BATCH_SIZE) {
            for (i, slot) in slots.iter().enumerate() {
                let message = format!("Batch message {}", i);
                slot.write_message(message.as_bytes());
            }
            producer.publish_batch(BATCH_SIZE);
        }
    }
    
    // Consumer: Read in batches (separate thread)
    std::thread::spawn(move || {
        loop {
            if let Some(batch) = consumer.try_consume_batch(BATCH_SIZE) {
                for message in batch {
                    // Process message
                    let _data = message.data();
                }
            }
        }
    });
    
    Ok(())
}
```

## 🎚️ Wait Strategies

Choose the right wait strategy for your use case:

```rust
use flux::WaitStrategy;

// Maximum performance, highest CPU usage
let buffer = RingBuffer::new(1024, WaitStrategy::BusySpin);

// Balanced performance and CPU usage
let buffer = RingBuffer::new(1024, WaitStrategy::Yielding);

// Lower CPU usage, higher latency
let buffer = RingBuffer::new(1024, WaitStrategy::Sleeping);

// Blocking with notifications
let buffer = RingBuffer::new(1024, WaitStrategy::Blocking);

// Timeout-based waiting
let buffer = RingBuffer::new(1024, WaitStrategy::Timeout { 
    timeout: Duration::from_micros(100) 
});
```

## ⚡ Performance Tips

### 1. Buffer Sizing

```rust
// For high-throughput: Use power-of-2 sizes
let buffer = RingBuffer::new(1024 * 1024, WaitStrategy::BusySpin);

// For low-latency: Smaller buffers
let buffer = RingBuffer::new(4096, WaitStrategy::BusySpin);
```

### 2. Batch Processing

```rust
// Always prefer batches over single operations
const OPTIMAL_BATCH_SIZE: usize = 1000;

// Producer batching
let slots = producer.try_claim_slots(OPTIMAL_BATCH_SIZE)?;
// ... fill slots ...
producer.publish_batch(OPTIMAL_BATCH_SIZE);

// Consumer batching
if let Some(batch) = consumer.try_consume_batch(OPTIMAL_BATCH_SIZE) {
    // Process entire batch
}
```

### 3. Thread Affinity

```rust
use flux::utils::set_thread_affinity;

// Pin threads to specific CPU cores
set_thread_affinity(0)?; // Producer on core 0
set_thread_affinity(1)?; // Consumer on core 1
```

## 🐛 Common Issues and Solutions

### Issue 1: Low Performance

**Symptoms**: Throughput below 1M messages/second

**Solutions**:
- Use `WaitStrategy::BusySpin` for maximum performance
- Increase batch sizes (try 1000-2000)
- Pin threads to dedicated CPU cores
- Use release builds: `cargo run --release`

### Issue 2: High CPU Usage

**Symptoms**: 100% CPU usage even when idle

**Solutions**:
- Switch to `WaitStrategy::Yielding` or `WaitStrategy::Sleeping`
- Implement backpressure in your application
- Use appropriate batch sizes

### Issue 3: Memory Usage

**Symptoms**: High memory consumption

**Solutions**:
- Reduce ring buffer size if not needed
- Use zero-copy operations consistently
- Avoid unnecessary allocations in hot paths

## 📊 Basic Benchmarking

```rust
use std::time::Instant;

fn benchmark_throughput() {
    let buffer = RingBuffer::new(1024 * 1024, WaitStrategy::BusySpin);
    let mut producer = buffer.create_producer();
    
    let start = Instant::now();
    let message_count = 1_000_000;
    
    for i in 0..message_count {
        if let Ok(slot) = producer.try_claim_slot() {
            slot.write_message(format!("Message {}", i).as_bytes());
            producer.publish_single();
        }
    }
    
    let duration = start.elapsed();
    let throughput = message_count as f64 / duration.as_secs_f64();
    
    println!("Throughput: {:.2} messages/second", throughput);
}
```

## 🎓 Next Steps

1. **Read the [Architecture Guide](./architecture.md)** to understand Flux's design
2. **Follow the [Performance Tuning](./performance.md)** guide for optimization
3. **Check platform-specific setup** in [Platform Setup](./platform-setup.md)
4. **Explore [Examples](../examples/)** for real-world usage patterns

## 🆘 Getting Help

- **Documentation**: Check other guides in the `docs/` folder
- **Examples**: Look at the `examples/` directory
- **Issues**: Report bugs on GitHub
- **Performance**: Run benchmarks and profiling tools

Happy coding with Flux! 🚀 
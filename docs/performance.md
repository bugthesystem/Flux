# Performance Tuning Guide

This guide provides detailed strategies and techniques for optimizing Flux performance across different scenarios and hardware configurations.

## Performance Goals

### Target Metrics

| Metric | Target | Elite |
|--------|---------|-------|
| **Throughput** | 10M+ msgs/sec | 30M+ msgs/sec |
| **Latency (P99)** | < 5µs | < 1µs |
| **CPU Usage** | < 80% | < 60% |
| **Memory Usage** | < 1GB | < 500MB |
| **Cache Miss Rate** | < 5% | < 2% |

### Performance Tiers

```
Performance Tiers:
┌─────────────────────────────────────────────────────────────┐
│                                                             │
│  🚀 LEGENDARY (30M+ msgs/sec)                               │
│  🔥 ELITE (20M+ msgs/sec)                                   │
│  ⚡ HIGH (10M+ msgs/sec)                                     │
│  ✅ GOOD (5M+ msgs/sec)                                      │
│  🟡 MODERATE (1M+ msgs/sec)                                 │
│  🔴 NEEDS WORK (< 1M msgs/sec)                              │
│                                                             │
└─────────────────────────────────────────────────────────────┘
```

## Core Optimization Strategies

### 1. Ring Buffer Optimization

#### Buffer Sizing

```rust
// Power-of-2 sizes for optimal performance
const OPTIMAL_SIZES: &[usize] = &[
    4096,      // Low latency
    16384,     // Balanced
    65536,     // High throughput
    262144,    // Very high throughput
    1048576,   // Maximum throughput
    4194304,   // Extreme performance
];

// Choose based on your use case
let buffer_size = if latency_critical {
    4096
} else if throughput_critical {
    1048576
} else {
    65536  // Balanced default
};

let buffer = RingBuffer::new(buffer_size, WaitStrategy::BusySpin);
```

#### Memory Layout Optimization

```rust
// Ensure proper alignment for cache efficiency
#[repr(C, align(64))]
struct OptimizedMessageSlot {
    // Hot data first (accessed frequently)
    sequence: u32,
    length: u32,
    
    // Message data (cache-aligned)
    data: [u8; 56],
}

// Pre-allocate buffers to avoid allocation overhead
const PREALLOC_BUFFER_SIZE: usize = 1024 * 1024;
static mut PREALLOC_BUFFER: [u8; PREALLOC_BUFFER_SIZE] = [0; PREALLOC_BUFFER_SIZE];
```

### 2. Batch Processing Optimization

#### Optimal Batch Sizes

```rust
// Batch sizes for different scenarios
const BATCH_SIZES: &[(usize, &str)] = &[
    (1, "Ultra-low latency"),
    (10, "Low latency"),
    (100, "Balanced"),
    (1000, "High throughput"),
    (2000, "Maximum throughput"),
];

fn choose_batch_size(use_case: UseCase) -> usize {
    match use_case {
        UseCase::UltraLowLatency => 1,
        UseCase::LowLatency => 10,
        UseCase::Balanced => 100,
        UseCase::HighThroughput => 1000,
        UseCase::MaxThroughput => 2000,
    }
}
```

#### Batch Processing Implementation

```rust
// Optimized batch producer
fn optimized_batch_producer(
    producer: &mut Producer,
    messages: &[Message],
    batch_size: usize,
) -> Result<(), FluxError> {
    let mut processed = 0;
    
    while processed < messages.len() {
        let remaining = messages.len() - processed;
        let current_batch_size = batch_size.min(remaining);
        
        // Claim slots in batch
        if let Ok(slots) = producer.try_claim_slots(current_batch_size) {
            // Write messages to slots
            for (i, slot) in slots.iter().enumerate() {
                if let Some(message) = messages.get(processed + i) {
                    slot.write_message(message.data());
                }
            }
            
            // Publish entire batch atomically
            producer.publish_batch(current_batch_size);
            processed += current_batch_size;
        } else {
            // Backoff strategy
            thread::yield_now();
        }
    }
    
    Ok(())
}
```

### 3. CPU Affinity and Thread Management

#### CPU Pinning Strategy

```rust
use flux::utils::set_thread_affinity;

fn optimize_thread_affinity() -> Result<(), FluxError> {
    // Get CPU information
    let num_cores = num_cpus::get();
    let num_physical_cores = num_cores / 2; // Assuming hyperthreading
    
    // Pin producer to dedicated core
    set_thread_affinity(0)?;
    
    // Pin consumer to adjacent core (same cache)
    set_thread_affinity(1)?;
    
    // Pin network threads to separate cores
    set_thread_affinity(2)?;
    
    println!("✅ Optimized thread affinity for {} cores", num_cores);
    Ok(())
}
```

#### Thread Pool Configuration

```rust
// Optimized thread pool for different workloads
struct OptimizedThreadPool {
    producer_threads: usize,
    consumer_threads: usize,
    network_threads: usize,
}

impl OptimizedThreadPool {
    fn new(num_cores: usize) -> Self {
        if num_cores >= 8 {
            // High-end system
            OptimizedThreadPool {
                producer_threads: 2,
                consumer_threads: 2,
                network_threads: 2,
            }
        } else if num_cores >= 4 {
            // Mid-range system
            OptimizedThreadPool {
                producer_threads: 1,
                consumer_threads: 1,
                network_threads: 1,
            }
        } else {
            // Low-end system
            OptimizedThreadPool {
                producer_threads: 1,
                consumer_threads: 1,
                network_threads: 0, // Use main thread
            }
        }
    }
}
```

### 4. Memory Management

#### Zero-Copy Operations

```rust
// Avoid allocations in hot paths
fn zero_copy_message_processing(
    consumer: &mut Consumer,
    processor: &mut MessageProcessor,
) -> Result<(), FluxError> {
    // Use pre-allocated buffers
    const BATCH_SIZE: usize = 1000;
    
    if let Some(batch) = consumer.try_consume_batch(BATCH_SIZE) {
        // Process messages without copying
        for message in batch {
            // Direct access to message data
            let data_slice = message.data();
            processor.process_in_place(data_slice)?;
        }
    }
    
    Ok(())
}
```

#### Memory Pool Management

```rust
// Memory pool for high-frequency allocations
struct MemoryPool {
    pool: Vec<Vec<u8>>,
    size: usize,
}

impl MemoryPool {
    fn new(pool_size: usize, buffer_size: usize) -> Self {
        let mut pool = Vec::with_capacity(pool_size);
        for _ in 0..pool_size {
            pool.push(vec![0u8; buffer_size]);
        }
        
        MemoryPool {
            pool,
            size: pool_size,
        }
    }
    
    fn get_buffer(&mut self) -> Option<Vec<u8>> {
        self.pool.pop()
    }
    
    fn return_buffer(&mut self, buffer: Vec<u8>) {
        if self.pool.len() < self.size {
            self.pool.push(buffer);
        }
    }
}
```

## Platform-Specific Optimizations

### Linux Optimizations

#### NUMA Awareness

```rust
use flux::utils::numa;

fn optimize_numa_linux() -> Result<(), FluxError> {
    // Get NUMA topology
    let numa_nodes = numa::get_numa_nodes()?;
    
    // Pin ring buffer to specific NUMA node
    let buffer = RingBuffer::new_with_numa(
        1024 * 1024,
        WaitStrategy::BusySpin,
        Some(0), // NUMA node 0
    )?;
    
    // Pin threads to NUMA-local cores
    for (i, node) in numa_nodes.iter().enumerate() {
        if let Some(core) = node.local_cores.first() {
            set_thread_affinity(*core)?;
        }
    }
    
    Ok(())
}
```

#### Huge Pages

```rust
// Use huge pages for large buffers
fn create_huge_page_buffer() -> Result<RingBuffer, FluxError> {
    let config = RingBufferConfig {
        size: 1024 * 1024,
        use_huge_pages: true,
        numa_node: Some(0),
        ..Default::default()
    };
    
    RingBuffer::new(config)
}
```

### macOS Optimizations

#### Apple Silicon Optimizations

```rust
use flux::utils::macos;

fn optimize_apple_silicon() -> Result<(), FluxError> {
    // Use P-cores for performance-critical threads
    let p_cores = macos::get_p_cores()?;
    
    // Pin producer to P-core
    if let Some(p_core) = p_cores.first() {
        set_thread_affinity(*p_core)?;
    }
    
    // Use NEON SIMD for data copying
    let config = RingBufferConfig {
        enable_simd: true,
        ..Default::default()
    };
    
    Ok(())
}
```

## Wait Strategy Selection

### Strategy Comparison

| Strategy | Latency | CPU Usage | Use Case |
|----------|---------|-----------|----------|
| BusySpin | Lowest | Highest | Ultra-low latency |
| Yielding | Low | High | Balanced performance |
| Sleeping | Medium | Medium | General purpose |
| Blocking | High | Low | Resource constrained |
| Timeout | Variable | Variable | Network applications |

### Implementation Guidelines

```rust
fn select_wait_strategy(requirements: &PerformanceRequirements) -> WaitStrategy {
    match (requirements.latency_critical, requirements.cpu_constrained) {
        (true, false) => WaitStrategy::BusySpin,
        (true, true) => WaitStrategy::Yielding,
        (false, true) => WaitStrategy::Sleeping,
        (false, false) => WaitStrategy::Blocking,
    }
}
```

## Performance Monitoring

### Metrics Collection

```rust
use std::sync::atomic::{AtomicU64, Ordering};

struct PerformanceMetrics {
    messages_sent: AtomicU64,
    messages_received: AtomicU64,
    latency_samples: Vec<u64>,
    cpu_usage: f64,
    memory_usage: usize,
}

impl PerformanceMetrics {
    fn record_latency(&mut self, latency_nanos: u64) {
        self.latency_samples.push(latency_nanos);
    }
    
    fn calculate_percentiles(&self) -> (u64, u64, u64) {
        let mut sorted = self.latency_samples.clone();
        sorted.sort_unstable();
        
        let p50 = percentile(&sorted, 50.0);
        let p95 = percentile(&sorted, 95.0);
        let p99 = percentile(&sorted, 99.0);
        
        (p50, p95, p99)
    }
}
```

### Benchmarking Framework

```rust
struct BenchmarkRunner {
    config: BenchmarkConfig,
    metrics: PerformanceMetrics,
}

impl BenchmarkRunner {
    fn run_benchmark(&mut self) -> Result<BenchmarkResults, FluxError> {
        // Warmup phase
        self.warmup()?;
        
        // Measurement phase
        let start_time = Instant::now();
        self.run_measurement()?;
        let duration = start_time.elapsed();
        
        // Calculate results
        let throughput = self.metrics.messages_sent.load(Ordering::Relaxed) as f64 
            / duration.as_secs_f64();
        
        let (p50, p95, p99) = self.metrics.calculate_percentiles();
        
        Ok(BenchmarkResults {
            throughput_messages_per_second: throughput,
            latency_p50_nanos: p50,
            latency_p95_nanos: p95,
            latency_p99_nanos: p99,
            test_duration_seconds: duration.as_secs_f64(),
        })
    }
}
```

## Common Performance Issues

### Issue 1: Low Throughput

**Symptoms**: Throughput below 1M messages/second

**Diagnosis**:
- Check batch sizes (should be 1000+ for high throughput)
- Verify wait strategy (BusySpin for maximum performance)
- Monitor CPU usage and cache misses

**Solutions**:
- Increase batch sizes
- Use BusySpin wait strategy
- Pin threads to dedicated cores
- Enable SIMD optimizations

### Issue 2: High Latency

**Symptoms**: P99 latency above 10µs

**Diagnosis**:
- Check for cache misses
- Monitor memory access patterns
- Verify thread affinity

**Solutions**:
- Reduce batch sizes for lower latency
- Use smaller ring buffers
- Optimize memory layout
- Pin producer/consumer to adjacent cores

### Issue 3: High CPU Usage

**Symptoms**: 100% CPU usage even when idle

**Diagnosis**:
- Check wait strategy (BusySpin uses 100% CPU)
- Monitor for busy-waiting loops
- Verify backpressure handling

**Solutions**:
- Switch to Yielding or Sleeping wait strategy
- Implement proper backpressure
- Use appropriate batch sizes
- Add yield points in hot loops

## Performance Testing

### Benchmark Configuration

```rust
#[derive(Debug)]
struct BenchmarkConfig {
    pub num_producers: usize,
    pub num_consumers: usize,
    pub ring_buffer_size: usize,
    pub message_size: usize,
    pub batch_size: usize,
    pub test_duration: Duration,
    pub warmup_duration: Duration,
}

impl Default for BenchmarkConfig {
    fn default() -> Self {
        Self {
            num_producers: 1,
            num_consumers: 1,
            ring_buffer_size: 1024 * 1024,
            message_size: 64,
            batch_size: 1000,
            test_duration: Duration::from_secs(10),
            warmup_duration: Duration::from_secs(1),
        }
    }
}
```

### Automated Testing

```rust
fn run_performance_suite() -> Result<(), FluxError> {
    let configs = vec![
        BenchmarkConfig {
            num_producers: 1,
            num_consumers: 1,
            ..Default::default()
        },
        BenchmarkConfig {
            num_producers: 4,
            num_consumers: 4,
            ..Default::default()
        },
    ];
    
    for config in configs {
        let mut runner = BenchmarkRunner::new(config);
        let results = runner.run_benchmark()?;
        results.print_summary();
    }
    
    Ok(())
}
```

This performance tuning guide provides the foundation for optimizing Flux applications. The key is to understand your specific requirements and choose the appropriate optimizations for your use case. 
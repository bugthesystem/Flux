# Performance Tuning Guide

This guide provides detailed strategies and techniques for optimizing Flux performance across different scenarios and hardware configurations.

## 🎯 Performance Goals

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

## 🔧 Core Optimization Strategies

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
    
    fn return_buffer(&mut self, mut buffer: Vec<u8>) {
        if self.pool.len() < self.size {
            buffer.clear();
            self.pool.push(buffer);
        }
    }
}
```

## 📊 Performance Monitoring and Profiling

### 1. Real-Time Metrics

```rust
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

struct PerformanceMetrics {
    messages_processed: AtomicU64,
    bytes_processed: AtomicU64,
    start_time: Instant,
}

impl PerformanceMetrics {
    fn new() -> Self {
        PerformanceMetrics {
            messages_processed: AtomicU64::new(0),
            bytes_processed: AtomicU64::new(0),
            start_time: Instant::now(),
        }
    }
    
    fn record_message(&self, size: usize) {
        self.messages_processed.fetch_add(1, Ordering::Relaxed);
        self.bytes_processed.fetch_add(size as u64, Ordering::Relaxed);
    }
    
    fn get_throughput(&self) -> f64 {
        let elapsed = self.start_time.elapsed().as_secs_f64();
        let messages = self.messages_processed.load(Ordering::Relaxed);
        messages as f64 / elapsed
    }
    
    fn get_bandwidth(&self) -> f64 {
        let elapsed = self.start_time.elapsed().as_secs_f64();
        let bytes = self.bytes_processed.load(Ordering::Relaxed);
        bytes as f64 / elapsed
    }
}
```

### 2. Latency Measurement

```rust
use hdrhistogram::Histogram;

struct LatencyTracker {
    histogram: Histogram<u64>,
}

impl LatencyTracker {
    fn new() -> Self {
        LatencyTracker {
            histogram: Histogram::new(3).unwrap(),
        }
    }
    
    fn record_latency(&mut self, latency: Duration) {
        let nanos = latency.as_nanos() as u64;
        self.histogram.record(nanos).unwrap();
    }
    
    fn get_percentiles(&self) -> LatencyStats {
        LatencyStats {
            p50: Duration::from_nanos(self.histogram.value_at_percentile(50.0)),
            p95: Duration::from_nanos(self.histogram.value_at_percentile(95.0)),
            p99: Duration::from_nanos(self.histogram.value_at_percentile(99.0)),
            p999: Duration::from_nanos(self.histogram.value_at_percentile(99.9)),
        }
    }
}
```

### 3. System Resource Monitoring

```rust
use sysinfo::{System, SystemExt, ProcessExt};

fn monitor_system_resources() -> SystemStats {
    let mut sys = System::new_all();
    sys.refresh_all();
    
    let process = sys.process(sysinfo::get_current_pid().unwrap()).unwrap();
    
    SystemStats {
        cpu_usage: process.cpu_usage(),
        memory_usage: process.memory(),
        virtual_memory: process.virtual_memory(),
        disk_usage: process.disk_usage(),
    }
}
```

## 🎚️ Wait Strategy Optimization

### Strategy Selection Matrix

```rust
fn select_optimal_wait_strategy(
    use_case: UseCase,
    cpu_cores: usize,
    power_constraints: bool,
) -> WaitStrategy {
    match (use_case, cpu_cores, power_constraints) {
        (UseCase::UltraLowLatency, cores, false) if cores > 4 => {
            WaitStrategy::BusySpin
        }
        (UseCase::LowLatency, cores, false) if cores > 2 => {
            WaitStrategy::Yielding
        }
        (UseCase::Balanced, _, _) => {
            WaitStrategy::Timeout { timeout: Duration::from_micros(100) }
        }
        (UseCase::PowerEfficient, _, _) => {
            WaitStrategy::Sleeping
        }
        (UseCase::Blocking, _, _) => {
            WaitStrategy::Blocking
        }
        _ => WaitStrategy::Yielding, // Safe default
    }
}
```

### Adaptive Wait Strategies

```rust
struct AdaptiveWaitStrategy {
    current_strategy: WaitStrategy,
    performance_history: Vec<f64>,
    adaptation_threshold: f64,
}

impl AdaptiveWaitStrategy {
    fn adapt(&mut self, current_throughput: f64) {
        self.performance_history.push(current_throughput);
        
        if self.performance_history.len() > 10 {
            self.performance_history.remove(0);
        }
        
        let avg_throughput = self.performance_history.iter().sum::<f64>() 
            / self.performance_history.len() as f64;
        
        if current_throughput < avg_throughput * self.adaptation_threshold {
            self.current_strategy = match self.current_strategy {
                WaitStrategy::Sleeping => WaitStrategy::Yielding,
                WaitStrategy::Yielding => WaitStrategy::BusySpin,
                _ => self.current_strategy,
            };
        }
    }
}
```

## 🔧 Platform-Specific Optimizations

### Linux Optimizations

```rust
#[cfg(target_os = "linux")]
mod linux_optimizations {
    use std::os::unix::io::AsRawFd;
    
    pub fn optimize_socket(socket: &UdpSocket) -> Result<(), std::io::Error> {
        use nix::sys::socket::*;
        
        let fd = socket.as_raw_fd();
        
        // Set socket buffer sizes
        setsockopt(fd, sockopt::RcvBuf, &(16 * 1024 * 1024))?;
        setsockopt(fd, sockopt::SndBuf, &(16 * 1024 * 1024))?;
        
        // Enable timestamp options
        setsockopt(fd, sockopt::Timestamp, &true)?;
        
        // Set CPU affinity for socket
        setsockopt(fd, sockopt::CpuAffinity, &0x01)?;
        
        Ok(())
    }
    
    pub fn setup_huge_pages() -> Result<(), std::io::Error> {
        // Enable transparent huge pages
        std::fs::write("/proc/sys/vm/nr_hugepages", "1024")?;
        Ok(())
    }
}
```

### macOS Optimizations

```rust
#[cfg(target_os = "macos")]
mod macos_optimizations {
    use std::os::unix::io::AsRawFd;
    
    pub fn optimize_socket(socket: &UdpSocket) -> Result<(), std::io::Error> {
        use nix::sys::socket::*;
        
        let fd = socket.as_raw_fd();
        
        // Set socket buffer sizes (smaller than Linux due to limits)
        setsockopt(fd, sockopt::RcvBuf, &(8 * 1024 * 1024))?;
        setsockopt(fd, sockopt::SndBuf, &(8 * 1024 * 1024))?;
        
        // Enable low-latency mode
        setsockopt(fd, sockopt::NoDelay, &true)?;
        
        Ok(())
    }
}
```

## 🚀 Advanced Optimization Techniques

### 1. NUMA-Aware Memory Allocation

```rust
#[cfg(target_os = "linux")]
fn allocate_numa_aware_buffer(size: usize, node: usize) -> Result<Vec<u8>, std::io::Error> {
    use libnuma::*;
    
    // Set memory policy for current thread
    set_membind(&[node])?;
    
    // Allocate buffer
    let buffer = vec![0u8; size];
    
    // Verify allocation is on correct NUMA node
    let actual_node = get_mempolicy_node(&buffer[0])?;
    assert_eq!(actual_node, node);
    
    Ok(buffer)
}
```

### 2. Cache-Optimized Data Structures

```rust
// Cache-friendly message queue
#[repr(C, align(64))]
struct CacheOptimizedQueue {
    // Producer data (own cache line)
    producer_head: AtomicUsize,
    producer_pad: [u8; 64 - 8],
    
    // Consumer data (own cache line)
    consumer_tail: AtomicUsize,
    consumer_pad: [u8; 64 - 8],
    
    // Buffer metadata
    capacity: usize,
    mask: usize,
    
    // Message slots
    slots: Vec<MessageSlot>,
}
```

### 3. Instruction-Level Optimizations

```rust
// Use SIMD instructions for batch processing
#[cfg(target_arch = "x86_64")]
fn simd_batch_copy(src: &[u8], dst: &mut [u8], count: usize) {
    use std::arch::x86_64::*;
    
    unsafe {
        let mut i = 0;
        while i + 32 <= count {
            let data = _mm256_loadu_si256(src.as_ptr().add(i) as *const __m256i);
            _mm256_storeu_si256(dst.as_mut_ptr().add(i) as *mut __m256i, data);
            i += 32;
        }
        
        // Handle remaining bytes
        std::ptr::copy_nonoverlapping(src.as_ptr().add(i), dst.as_mut_ptr().add(i), count - i);
    }
}
```

## 📈 Performance Testing and Validation

### 1. Benchmark Suite

```rust
use criterion::{black_box, criterion_group, criterion_main, Criterion};

fn benchmark_throughput(c: &mut Criterion) {
    let mut group = c.benchmark_group("throughput");
    
    for batch_size in [1, 10, 100, 1000].iter() {
        group.bench_with_input(
            BenchmarkId::new("batch_size", batch_size),
            batch_size,
            |b, &batch_size| {
                let buffer = RingBuffer::new(1024 * 1024, WaitStrategy::BusySpin);
                let mut producer = buffer.create_producer();
                let mut consumer = buffer.create_consumer();
                
                b.iter(|| {
                    benchmark_batch_processing(
                        black_box(&mut producer),
                        black_box(&mut consumer),
                        black_box(batch_size),
                    )
                });
            },
        );
    }
    
    group.finish();
}

criterion_group!(benches, benchmark_throughput);
criterion_main!(benches);
```

### 2. Stress Testing

```rust
fn stress_test_high_load() -> Result<(), FluxError> {
    let buffer = RingBuffer::new(4 * 1024 * 1024, WaitStrategy::BusySpin);
    let num_producers = 4;
    let num_consumers = 2;
    let messages_per_producer = 1_000_000;
    
    let mut handles = Vec::new();
    
    // Start producers
    for i in 0..num_producers {
        let producer = buffer.create_producer();
        let handle = thread::spawn(move || {
            stress_test_producer(producer, messages_per_producer, i)
        });
        handles.push(handle);
    }
    
    // Start consumers
    for i in 0..num_consumers {
        let consumer = buffer.create_consumer();
        let handle = thread::spawn(move || {
            stress_test_consumer(consumer, messages_per_producer * num_producers / num_consumers, i)
        });
        handles.push(handle);
    }
    
    // Wait for completion
    for handle in handles {
        handle.join().unwrap()?;
    }
    
    Ok(())
}
```

## 🎯 Performance Tuning Checklist

### ✅ Basic Optimizations

- [ ] Use power-of-2 buffer sizes
- [ ] Choose appropriate wait strategy
- [ ] Enable release mode compilation
- [ ] Use batch processing where possible
- [ ] Pin threads to CPU cores
- [ ] Avoid allocations in hot paths

### ✅ Intermediate Optimizations

- [ ] Optimize batch sizes for workload
- [ ] Use zero-copy operations
- [ ] Configure system parameters
- [ ] Monitor and profile performance
- [ ] Implement proper error handling
- [ ] Use pre-allocated buffers

### ✅ Advanced Optimizations

- [ ] NUMA-aware memory allocation
- [ ] Cache-optimized data structures
- [ ] Platform-specific optimizations
- [ ] SIMD instruction usage
- [ ] Kernel bypass techniques
- [ ] Custom memory allocators

## 📊 Performance Troubleshooting

### Common Issues and Solutions

| Issue | Symptoms | Solution |
|-------|----------|----------|
| **Low Throughput** | < 1M msgs/sec | Increase batch size, check CPU affinity |
| **High Latency** | > 10µs P99 | Use BusySpin wait strategy, reduce batch size |
| **High CPU Usage** | > 90% CPU | Use Yielding/Sleeping wait strategy |
| **Memory Leaks** | Growing memory usage | Check for allocation in hot paths |
| **Cache Misses** | Poor performance | Optimize data layout, use prefetching |

Ready to achieve maximum performance with Flux! 🚀 

# Ultra Performance Benchmark: How We Achieved 11M+ Messages/sec

## Overview

In June 2024, we pushed Flux to new heights with an ultra performance benchmark, achieving **11.33 million messages/second** on Apple Silicon with 8 consumers, 64-byte messages, and aggressive batching. This section documents the architectural changes, benchmark setup, and key learnings.

---

## Benchmark Setup

- **Benchmark:** `benches/ultra_performance_bench.rs`
- **Configuration:**
  - 2M slot ring buffer
  - 8 consumers (threads)
  - 64-byte messages (cache-line friendly)
  - 64K batch size for producer/consumer
  - Busy-spin wait strategy
  - CPU affinity for all threads
- **Platform:** Apple Silicon (macOS, 10-core)
- **Duration:** 10 seconds

### Command
```bash
cargo run --release --bin ultra_performance_bench
```

---

## Key Architectural Changes

- **Zero-Copy Batch Claim:**
  - `try_claim_slots_ultra(&mut self, count)` returns a mutable slice for direct, zero-copy access.
  - Producer thread gets exclusive mutable access (no Arc overhead).
- **Aggressive Batching:**
  - Producer claims and publishes 64K messages at a time.
  - Consumers process in large batches, minimizing atomic ops.
- **Thread Affinity:**
  - Each thread is pinned to a dedicated core for cache locality.
- **Lock-Free for Consumers:**
  - Consumers use shared read access; only the producer needs mutable access.
- **Mutex for Producer Safety:**
  - Used Arc<Mutex<RingBuffer>> for safe mutable access in a multi-threaded benchmark.

---

## Results

```
🏆 ULTRA PERFORMANCE RESULTS
============================
📊 Total Messages: 113,377,280
📊 Duration: 10.01 seconds
📊 Throughput: 11.33 M messages/second
📊 Messages/Core: 1.42 M/sec
```

---

## Key Learnings

- **Batching is King:** Large batch sizes (64K) dramatically reduce atomic and synchronization overhead.
- **Thread Affinity Matters:** Pinning threads to isolated cores improves cache locality and reduces context switching.
- **Zero-Copy Slices:** Returning mutable slices for batch operations is the most efficient way to move data in Rust.
- **Mutex Overhead:** The main bottleneck at this scale is the mutex for safe mutable access. A true SPSC/MPMC lock-free design would push performance even higher.
- **System Tuning:** Further gains are possible with CPU isolation, huge pages, and real-time scheduling.

---

## Next Steps

- Remove the mutex for single-producer scenarios (true lock-free SPSC/MPMC).
- Add NUMA-aware allocation and huge page support for Linux.
- Explore kernel bypass (io_uring, DPDK) for networked use cases.
- Continue tuning batch sizes and cache alignment for each platform.

---

**Flux is now among the fastest open-source message transports in Rust.**

For more details, see `benches/ultra_performance_bench.rs` and the main README. 
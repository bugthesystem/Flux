# Linux Optimizations for Flux

This document describes the Linux-specific optimizations available in Flux and how to use them for maximum performance.

## Available Features

### 1. **NUMA-Aware Allocation** (`linux_numa`)
- Allocates memory on specific NUMA nodes
- Reduces cross-node memory access latency
- Automatically detects NUMA topology

### 2. **Huge Pages** (`linux_hugepages`)
- Uses 2MB huge pages instead of 4KB pages
- Reduces TLB misses and page table overhead
- Improves memory bandwidth utilization

### 3. **Thread Affinity** (`linux_affinity`)
- Pins threads to specific CPU cores
- Uses `sched_setaffinity` for precise control
- Reduces cache misses and improves locality

### 4. **Kernel Bypass I/O** (`linux_io_uring`)
- Uses `io_uring` for zero-copy async I/O
- Bypasses kernel overhead for network operations
- Enables true zero-copy end-to-end

## Building with Linux Optimizations

### Enable All Linux Optimizations
```bash
cargo build --release --features linux_optimized
```

### Enable Specific Features
```bash
# NUMA-aware allocation
cargo build --release --features linux_numa

# Huge pages
cargo build --release --features linux_hugepages

# Thread affinity
cargo build --release --features linux_affinity

# Kernel bypass I/O
cargo build --release --features linux_io_uring

# Multiple features
cargo build --release --features linux_numa,linux_hugepages,linux_affinity
```

## Usage Examples

### Basic Linux-Optimized Ring Buffer
```rust
use flux::disruptor::{RingBufferConfig, LinuxRingBuffer};

let config = RingBufferConfig::new(1024 * 1024)
    .unwrap()
    .with_consumers(4)
    .unwrap()
    .with_huge_pages(true)
    .with_numa_node(Some(0));

let buffer = LinuxRingBuffer::new(config)?;
```

### Thread Affinity and Priority
```rust
use flux::utils::{linux_pin_to_cpu, linux_set_max_priority};

// Pin thread to CPU core 0
linux_pin_to_cpu(0)?;

// Set maximum thread priority
linux_set_max_priority()?;
```

### NUMA-Aware Allocation
```rust
use flux::utils::LinuxNumaOptimizer;

if let Some(numa) = LinuxNumaOptimizer::new() {
    println!("NUMA nodes: {}", numa.num_nodes());
    
    // Allocate on specific NUMA node
    let ptr = numa.allocate_on_node(1024 * 1024, 0);
}
```

## Performance Benchmarks

### Linux Ultra Benchmark
```bash
cargo run --release --example linux_ultra_bench --features linux_optimized
```

This benchmark uses all Linux optimizations:
- 8 producers, 8 consumers
- 128K batch size
- NUMA-aware allocation
- Huge pages
- Thread affinity
- Maximum priority

### Expected Performance
- **Single-threaded**: Target 50-100M messages/second (requires implementation)
- **Multi-threaded**: 100-500M messages/second
- **With kernel bypass**: 1B+ messages/second

## System Requirements

### Hardware
- Multi-socket system for NUMA benefits
- High-speed memory (DDR4/DDR5)
- Fast storage for persistence

### Software
- Linux kernel 4.18+ for `io_uring`
- `libnuma` development headers
- Root access for huge pages and priority

### Kernel Configuration
```bash
# Enable huge pages
echo 1024 > /proc/sys/vm/nr_hugepages

# Set CPU governor to performance
echo performance | sudo tee /sys/devices/system/cpu/cpu*/cpufreq/scaling_governor

# Disable CPU frequency scaling
echo 1 | sudo tee /sys/devices/system/cpu/intel_pstate/no_turbo
```

## Troubleshooting

### NUMA Not Available
```bash
# Install libnuma development headers
sudo apt-get install libnuma-dev  # Ubuntu/Debian
sudo yum install numactl-devel    # RHEL/CentOS
```

### Huge Pages Not Working
```bash
# Check huge page availability
cat /proc/meminfo | grep Huge

# Mount huge pages
sudo mkdir -p /mnt/huge
sudo mount -t hugetlbfs nodev /mnt/huge
```

### Permission Denied for Affinity
```bash
# Run with appropriate capabilities
sudo setcap 'cap_sys_nice=eip' target/release/your_binary
```

## Advanced Configuration

### NUMA Topology
```bash
# View NUMA topology
numactl --hardware

# Run on specific NUMA node
numactl --cpunodebind=0 --membind=0 ./your_binary
```

### CPU Isolation
```bash
# Isolate CPU cores for real-time use
# Add to kernel command line: isolcpus=1-7
```

### Memory Locking
```bash
# Increase memory lock limit
echo "* soft memlock unlimited" >> /etc/security/limits.conf
echo "* hard memlock unlimited" >> /etc/security/limits.conf
```

## Performance Tuning

### 1. **CPU Governor**
```bash
# Set to performance mode
echo performance | sudo tee /sys/devices/system/cpu/cpu*/cpufreq/scaling_governor
```

### 2. **CPU Affinity**
```bash
# Pin to specific cores
taskset -c 0-7 ./your_binary
```

### 3. **NUMA Balancing**
```bash
# Disable NUMA balancing for predictable performance
echo 0 > /proc/sys/kernel/numa_balancing
```

### 4. **Transparent Huge Pages**
```bash
# Disable for predictable performance
echo never > /sys/kernel/mm/transparent_hugepage/enabled
```

## Monitoring

### Performance Counters
```bash
# Monitor CPU usage
perf stat -e cpu-cycles,instructions,cache-misses ./your_binary

# Monitor NUMA activity
perf stat -e numa:numa_hit,numa:numa_miss ./your_binary
```

### System Monitoring
```bash
# Monitor CPU frequency
watch -n 1 'cat /proc/cpuinfo | grep "cpu MHz"'

# Monitor NUMA stats
cat /proc/zoneinfo | grep -A 5 "Node"
```

## Best Practices

1. **Use NUMA-aware allocation** for multi-socket systems
2. **Enable huge pages** for large buffers
3. **Pin threads to cores** for predictable performance
4. **Use kernel bypass** for network I/O
5. **Monitor and tune** based on your specific workload
6. **Test with realistic data** sizes and patterns
7. **Profile regularly** to identify bottlenecks

## Future Enhancements

- **DPDK support** for raw packet I/O
- **XDP (eXpress Data Path)** for kernel bypass networking
- **Custom memory allocators** for specific workloads
- **Advanced NUMA strategies** for complex topologies
- **Real-time scheduling** with SCHED_FIFO/SCHED_RR 
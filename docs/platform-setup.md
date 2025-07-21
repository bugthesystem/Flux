# Platform Setup Guide

This guide provides platform-specific setup instructions and optimizations for Flux on macOS and Linux.

## 🐧 Linux Setup (Recommended)

Linux provides the best performance for Flux due to superior kernel support and optimization features.

### 🔧 System Requirements

```bash
# Minimum requirements
CPU: Multi-core x86_64 processor
RAM: 8GB minimum, 16GB+ recommended
Kernel: Linux 5.4+ (5.15+ recommended)
Distribution: Ubuntu 20.04+, CentOS 8+, or equivalent

# Recommended for maximum performance
CPU: Intel i7/i9 or AMD Ryzen 7/9 (8+ cores)
RAM: 32GB+ DDR4-3200 or faster
Storage: NVMe SSD
Network: 10GbE+ for network-intensive workloads
```

### 📦 Installation

```bash
# Install Rust
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
source ~/.cargo/env

# Install system dependencies
sudo apt update
sudo apt install -y \
    build-essential \
    pkg-config \
    libssl-dev \
    libnuma-dev \
    linux-tools-$(uname -r) \
    perf-tools-unstable

# For CentOS/RHEL
sudo yum groupinstall -y "Development Tools"
sudo yum install -y openssl-devel numactl-devel perf

# Install Flux
cargo install flux
```

### ⚡ Performance Optimizations

#### 1. Kernel Parameters

```bash
# Create performance tuning script
sudo nano /etc/sysctl.d/99-flux-performance.conf
```

Add the following optimizations:

```bash
# Network optimizations
net.core.rmem_max = 268435456
net.core.wmem_max = 268435456
net.core.rmem_default = 67108864
net.core.wmem_default = 67108864
net.ipv4.tcp_rmem = 4096 87380 268435456
net.ipv4.tcp_wmem = 4096 65536 268435456
net.core.netdev_max_backlog = 30000

# Memory optimizations
vm.swappiness = 1
vm.dirty_ratio = 15
vm.dirty_background_ratio = 5

# CPU optimizations
kernel.sched_rt_runtime_us = 1000000
kernel.sched_rt_period_us = 1000000

# Apply changes
sudo sysctl -p /etc/sysctl.d/99-flux-performance.conf
```

#### 2. Huge Pages Setup

```bash
# Configure huge pages
echo 1024 | sudo tee /proc/sys/vm/nr_hugepages

# Make permanent
echo 'vm.nr_hugepages = 1024' | sudo tee -a /etc/sysctl.conf

# Verify
grep HugePages /proc/meminfo
```

#### 3. CPU Isolation

```bash
# Edit GRUB configuration
sudo nano /etc/default/grub

# Add CPU isolation (adjust for your system)
GRUB_CMDLINE_LINUX="isolcpus=2-7 nohz_full=2-7 rcu_nocbs=2-7"

# Update GRUB
sudo update-grub
sudo reboot

# Verify isolation
cat /proc/cmdline
```

#### 4. IRQ Affinity

```bash
#!/bin/bash
# irq-affinity.sh - Pin network IRQs to specific CPUs

# Find network interface
INTERFACE=$(ip route | grep default | awk '{print $5}' | head -1)

# Get IRQ numbers
IRQS=$(grep $INTERFACE /proc/interrupts | awk '{print $1}' | sed 's/://g')

# Pin IRQs to CPUs 0-1 (leave 2-7 for application)
CPU_MASK="03"  # Binary: 00000011 (CPU 0,1)

for IRQ in $IRQS; do
    echo $CPU_MASK | sudo tee /proc/irq/$IRQ/smp_affinity
    echo "IRQ $IRQ pinned to CPUs 0-1"
done
```

### 🧮 NUMA Configuration

```bash
# Check NUMA topology
numactl --hardware

# Run Flux with NUMA awareness
numactl --cpunodebind=0 --membind=0 cargo run --release --bin bench_extreme

# For multi-node setups
numactl --interleave=all cargo run --release --bin bench_extreme
```

### 🔧 Real-Time Scheduling

```bash
# Enable real-time scheduling
sudo nano /etc/security/limits.conf

# Add these lines (replace 'username' with your username)
username soft rtprio 99
username hard rtprio 99
username soft memlock unlimited
username hard memlock unlimited

# Set process priority
sudo chrt -f 50 cargo run --release --bin bench_extreme
```

### 📊 Performance Monitoring

```bash
# Install monitoring tools
sudo apt install -y htop iotop nethogs

# Monitor CPU usage
htop

# Monitor network performance
nethogs

# Profile with perf
perf record -g cargo run --release --bin bench_extreme
perf report

# Monitor cache misses
perf stat -e cache-misses,cache-references cargo run --release
```

## 🍎 macOS Setup

> **📖 For comprehensive macOS optimization guidance, see [macOS Optimizations](macos_optimizations.md)**

### Quick Installation

```bash
# Install Xcode Command Line Tools
xcode-select --install

# Install Rust
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
source ~/.cargo/env

# Install Flux
cargo install flux
```

### Performance Characteristics

| Feature | Linux | macOS |
|---------|-------|-------|
| **Peak Performance** | 30M+ msgs/sec | 25M+ msgs/sec |
| **Thread Affinity** | Full support | Apple Silicon: ❌, Intel: Limited |
| **NUMA Support** | Full support | N/A (unified memory) |
| **Real-time Scheduling** | Full support | QoS classes only |

**📋 Use Cases:**
- ✅ **Development and testing**
- ✅ **Single-node deployments** 
- ✅ **Apple Silicon optimization**
- ❌ **Production high-frequency trading** (use Linux)

## 🎯 Platform-Specific Optimizations

### Linux Advanced Tuning

```bash
# CPU Governor
echo performance | sudo tee /sys/devices/system/cpu/cpu*/cpufreq/scaling_governor

# Disable C-states
sudo cpupower idle-set -D 0

# Set CPU affinity for maximum performance
taskset -c 2-7 cargo run --release --bin bench_extreme
```

### macOS Advanced Tuning

```bash
# Disable App Nap
defaults write NSGlobalDomain NSAppSleepDisabled -bool YES

# Increase networking buffers
sudo sysctl -w net.inet.udp.maxdgram=65536
sudo sysctl -w net.inet.raw.maxdgram=65536
```

## 🏃‍♂️ Performance Verification

### Benchmark Commands

```bash
# Linux - Maximum performance
sudo numactl --cpunodebind=0 --membind=0 \
  taskset -c 2-7 \
  chrt -f 50 \
  cargo run --release --bin bench_extreme

# macOS - Optimized run
cargo run --release --bin bench_extreme
```

### Expected Performance

| Platform | Single Producer | Multi-Producer | Latency (P99) |
|----------|----------------|----------------|---------------|
| **Linux (optimized)** | 9M+ msgs/sec | 30M+ msgs/sec | < 1µs |
| **macOS (M1/M2)** | 7M+ msgs/sec | 25M+ msgs/sec | < 2µs |
| **macOS (Intel)** | 6M+ msgs/sec | 20M+ msgs/sec | < 3µs |

Ready to achieve maximum performance on your platform! 🚀 
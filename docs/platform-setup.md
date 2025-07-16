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
numactl --cpunodebind=0 --membind=0 cargo run --release --bin extreme_bench

# For multi-node setups
numactl --interleave=all cargo run --release --bin extreme_bench
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
sudo chrt -f 50 cargo run --release --bin extreme_bench
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
perf record -g cargo run --release --bin extreme_bench
perf report

# Monitor cache misses
perf stat -e cache-misses,cache-references cargo run --release
```

## 🍎 macOS Setup

macOS provides good performance with some limitations compared to Linux.

### 🔧 System Requirements

```bash
# Minimum requirements
macOS: 10.15 (Catalina) or later
CPU: Intel i5/i7 or Apple Silicon M1/M2
RAM: 8GB minimum, 16GB+ recommended
Xcode: Command Line Tools installed

# Recommended
macOS: 12.0+ (Monterey)
CPU: Apple M1 Pro/Max or Intel i7/i9
RAM: 32GB+ unified memory (Apple Silicon) or DDR4
Storage: SSD with 1GB+ free space
```

### 📦 Installation

```bash
# Install Xcode Command Line Tools
xcode-select --install

# Install Homebrew
/bin/bash -c "$(curl -fsSL https://raw.githubusercontent.com/Homebrew/install/HEAD/install.sh)"

# Install Rust
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
source ~/.cargo/env

# Install Flux
cargo install flux
```

### ⚡ Performance Optimizations

#### 1. System Settings

```bash
# Disable System Integrity Protection (for advanced users only)
# This is required for some performance optimizations
# Boot into Recovery Mode and run:
# csrutil disable

# Increase file descriptor limits
sudo nano /etc/sysctl.conf
```

Add:
```bash
kern.maxfiles=65536
kern.maxfilesperproc=65536
```

#### 2. Memory and CPU Optimizations

```bash
# Create launch daemon for performance
sudo nano /Library/LaunchDaemons/com.flux.performance.plist
```

```xml
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" 
    "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>Label</key>
    <string>com.flux.performance</string>
    <key>ProgramArguments</key>
    <array>
        <string>/usr/sbin/sysctl</string>
        <string>-w</string>
        <string>kern.tcsm_available=1</string>
    </array>
    <key>RunAtLoad</key>
    <true/>
</dict>
</plist>
```

Load the daemon:
```bash
sudo launchctl load /Library/LaunchDaemons/com.flux.performance.plist
```

#### 3. Thread Affinity (Limited on macOS)

```bash
# macOS has limited thread affinity support
# Use thread_policy_set() in your application instead
```

### 🧮 Apple Silicon Optimizations

```bash
# For Apple Silicon (M1/M2) processors
export CARGO_TARGET_AARCH64_APPLE_DARWIN_RUSTFLAGS="-C target-cpu=apple-m1"

# Build with optimizations
cargo build --release --target aarch64-apple-darwin

# Use Rosetta for x86_64 compatibility if needed
arch -x86_64 cargo run --release
```

### 📊 Performance Monitoring

```bash
# Install monitoring tools
brew install htop
brew install activity-monitor

# Monitor system performance
sudo powermetrics --samplers smc,cpu_power,gpu_power -n 1 -i 1000

# Use Activity Monitor for detailed analysis
open -a "Activity Monitor"

# Profile with Instruments
instruments -t "Time Profiler" cargo run --release --bin extreme_bench
```

## 🏆 Platform Comparison

### Performance Characteristics

| Feature | Linux | macOS |
|---------|-------|-------|
| **Peak Performance** | 30M+ msgs/sec | 25M+ msgs/sec |
| **Latency** | Sub-microsecond | 1-2 microseconds |
| **CPU Isolation** | Full support | Limited |
| **NUMA Support** | Full support | N/A |
| **Real-time Scheduling** | Full support | Limited |
| **Huge Pages** | Supported | Not available |
| **IRQ Affinity** | Supported | Not available |

### Recommended Use Cases

#### Linux
- **Production deployments**
- **High-frequency trading**
- **Maximum performance requirements**
- **Multi-socket systems**
- **Network-intensive applications**

#### macOS
- **Development and testing**
- **Proof-of-concept applications**
- **Single-node deployments**
- **Apple Silicon optimization**

## 🚀 Quick Start Scripts

### Linux Quick Setup

```bash
#!/bin/bash
# linux-flux-setup.sh

set -e

echo "🐧 Setting up Flux for Linux..."

# Install dependencies
sudo apt update
sudo apt install -y build-essential pkg-config libssl-dev libnuma-dev

# Install Rust
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
source ~/.cargo/env

# Clone and build Flux
git clone https://github.com/your-org/flux.git
cd flux
cargo build --release

# Configure system
sudo sysctl -w vm.nr_hugepages=1024
sudo sysctl -w net.core.rmem_max=268435456
sudo sysctl -w net.core.wmem_max=268435456

# Run benchmark
cargo run --release --bin extreme_bench

echo "✅ Linux setup complete!"
```

### macOS Quick Setup

```bash
#!/bin/bash
# macos-flux-setup.sh

set -e

echo "🍎 Setting up Flux for macOS..."

# Install Xcode CLI tools
xcode-select --install 2>/dev/null || true

# Install Homebrew if not present
if ! command -v brew &> /dev/null; then
    /bin/bash -c "$(curl -fsSL https://raw.githubusercontent.com/Homebrew/install/HEAD/install.sh)"
fi

# Install Rust
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
source ~/.cargo/env

# Clone and build Flux
git clone https://github.com/your-org/flux.git
cd flux
cargo build --release

# Run benchmark
cargo run --release --bin extreme_bench

echo "✅ macOS setup complete!"
```

## 🔧 Troubleshooting

### Common Linux Issues

#### Permission Denied for Huge Pages
```bash
# Solution: Add user to appropriate groups
sudo usermod -a -G adm,dialout,cdrom,plugdev,lpadmin,admin,sambashare $USER

# Or run with sudo
sudo cargo run --release --bin extreme_bench
```

#### Network Interface Issues
```bash
# Check available interfaces
ip link show

# Verify IRQ assignments
cat /proc/interrupts | grep eth0
```

### Common macOS Issues

#### Xcode Command Line Tools
```bash
# Reinstall if needed
sudo xcode-select --reset
xcode-select --install
```

#### Apple Silicon Compatibility
```bash
# Use Rosetta for x86_64 binaries
arch -x86_64 cargo run --release

# Or build natively
cargo build --release --target aarch64-apple-darwin
```

## 🎯 Platform-Specific Optimizations

### Linux Advanced Tuning

```bash
# CPU Governor
echo performance | sudo tee /sys/devices/system/cpu/cpu*/cpufreq/scaling_governor

# Disable C-states
sudo cpupower idle-set -D 0

# Set CPU affinity for maximum performance
taskset -c 2-7 cargo run --release --bin extreme_bench
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
  cargo run --release --bin extreme_bench

# macOS - Optimized run
cargo run --release --bin extreme_bench
```

### Expected Performance

| Platform | Single Producer | Multi-Producer | Latency (P99) |
|----------|----------------|----------------|---------------|
| **Linux (optimized)** | 9M+ msgs/sec | 30M+ msgs/sec | < 1µs |
| **macOS (M1/M2)** | 7M+ msgs/sec | 25M+ msgs/sec | < 2µs |
| **macOS (Intel)** | 6M+ msgs/sec | 20M+ msgs/sec | < 3µs |

Ready to achieve maximum performance on your platform! 🚀 
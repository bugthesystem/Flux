# AF_XDP Implementation Plan

## Overview

AF_XDP provides kernel bypass networking on Linux, enabling sub-microsecond latency.

```
┌─────────────────────────────────────────────────────────────┐
│  NETWORK I/O PROGRESSION                                    │
├─────────────────────────────────────────────────────────────┤
│  Level 1: sendmmsg/recvmmsg  →  Batch syscalls (~5 µs)      │
│  Level 2: io_uring           →  Async syscalls (~2 µs)      │
│  Level 3: AF_XDP             →  Kernel bypass (< 1 µs)      │
└─────────────────────────────────────────────────────────────┘
```

## Architecture

```
┌──────────────────────────────────────────────────────────────┐
│                        APPLICATION                           │
│  ┌─────────────┐      ┌─────────────┐      ┌─────────────┐   │
│  │  Producer   │ ──→  │  UMEM       │ ──→  │  Consumer   │   │
│  │  (TX ring)  │      │  (shared)   │      │  (RX ring)  │   │
│  └─────────────┘      └─────────────┘      └─────────────┘   │
└──────────────────────────────────────────────────────────────┘
         │                    │                    ▲
         │              Zero-copy                  │
         ▼                    │                    │
┌──────────────────────────────────────────────────────────────┐
│                     KERNEL (eBPF)                            │
│  ┌─────────────────────────────────────────────────────────┐ │
│  │  XDP Program: Redirect packets to AF_XDP socket         │ │
│  └─────────────────────────────────────────────────────────┘ │
└──────────────────────────────────────────────────────────────┘
         │                                         ▲
         ▼                                         │
┌──────────────────────────────────────────────────────────────┐
│                          NIC                                 │
└──────────────────────────────────────────────────────────────┘
```

## Dependencies

```toml
# kaos-driver/Cargo.toml
[target.'cfg(target_os = "linux")'.dependencies]
xsk-rs = { version = "0.3", optional = true }      # AF_XDP socket
libbpf-rs = { version = "0.22", optional = true }  # eBPF loader

[features]
xdp = ["xsk-rs", "libbpf-rs"]
```

## Implementation Steps

### 1. XDP Module Structure

```
kaos-driver/src/
├── main.rs
├── uring.rs       # existing io_uring
└── xdp.rs         # NEW: AF_XDP
    ├── XdpConfig
    ├── XdpSocket
    ├── UmemArea
    └── run_xdp()
```

### 2. Core API

```rust
// kaos-driver/src/xdp.rs
pub struct XdpConfig {
    pub interface: String,      // e.g., "eth0"
    pub queue_id: u32,          // NIC queue
    pub frame_size: u32,        // 4096 typical
    pub frame_count: u32,       // Number of frames
    pub batch_size: u32,        // TX/RX batch
}

pub struct XdpSocket {
    umem: Umem,
    fill_queue: FillQueue,
    comp_queue: CompQueue,
    tx_queue: TxQueue,
    rx_queue: RxQueue,
}

impl XdpSocket {
    pub fn new(config: XdpConfig) -> io::Result<Self>;
    pub fn send(&mut self, data: &[u8]) -> io::Result<()>;
    pub fn recv(&mut self) -> Option<&[u8]>;
    pub fn poll(&mut self) -> io::Result<usize>;
}
```

### 3. Docker Setup

```dockerfile
# Dockerfile.xdp
FROM rust:1.76-bookworm AS builder
WORKDIR /app
RUN apt-get update && apt-get install -y \
    clang llvm libelf-dev linux-headers-generic
COPY . .
RUN cargo build --release -p kaos-driver --features xdp

FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y libelf1
COPY --from=builder /app/target/release/kaos-driver /usr/local/bin/
CMD ["kaos-driver", "--xdp", "eth0"]
```

```yaml
# docker-compose-xdp.yml
version: '3.8'
services:
  xdp-driver:
    build:
      context: ..
      dockerfile: kaos-driver/Dockerfile.xdp
    privileged: true              # Required for XDP
    network_mode: host            # Direct NIC access
    cap_add:
      - NET_ADMIN
      - SYS_ADMIN
      - BPF
    volumes:
      - /sys/fs/bpf:/sys/fs/bpf   # BPF filesystem
      - /sys/kernel/debug:/sys/kernel/debug
```

## Benchmark Plan

```rust
// examples/bench_xdp.rs
fn main() {
    println!("=== Network I/O Comparison ===\n");
    
    // 1. sendmmsg baseline
    let sendmmsg_throughput = bench_sendmmsg(N);
    
    // 2. io_uring 
    #[cfg(feature = "uring")]
    let uring_throughput = bench_uring(N);
    
    // 3. AF_XDP
    #[cfg(feature = "xdp")]
    let xdp_throughput = bench_xdp(N);
    
    // Results table
    println!("┌────────────────┬───────────────┬────────────┐");
    println!("│ Method         │ Throughput    │ Latency    │");
    println!("├────────────────┼───────────────┼────────────┤");
    println!("│ sendmmsg       │ {:>10} M/s    │ {:>6} µs   │", ...);
    println!("│ io_uring       │ {:>10} M/s    │ {:>6} µs   │", ...);
    println!("│ AF_XDP         │ {:>10} M/s    │ {:>6} µs   │", ...);
    println!("└────────────────┴───────────────┴────────────┘");
}
```

## Limitations

1. **Linux only** - No macOS/Windows support
2. **Root required** - Needs CAP_NET_ADMIN, CAP_BPF
3. **NIC support** - Not all NICs support XDP
4. **Complexity** - eBPF program management

## Testing Strategy

1. **Local (veth pair)**
   ```bash
   # Create virtual interface pair for testing
   ip link add veth0 type veth peer name veth1
   ip link set veth0 up
   ip link set veth1 up
   ```

2. **Docker** - Use privileged mode with host network

3. **Real hardware** - Test on supported NICs (Intel, Mellanox)

## Current Status

| Task | Status | Notes |
|------|--------|-------|
| Dependencies (xsk-rs 0.6) | ✅ | Requires Rust nightly (edition 2024) |
| xdp.rs module | ✅ | Feature-gated, Linux only |
| XdpSocket + UMEM | ✅ | Zero-copy send/recv |
| Dockerfile.xdp | ✅ | Nightly Rust, privileged mode |
| docker-compose-xdp.yml | ✅ | veth pair setup |
| test-xdp.sh | ✅ | Local Linux testing script |
| Benchmarks | ⬜ | Pending Linux testing |

## Requirements

- **Rust nightly** - xsk-rs uses edition 2024
- **Linux kernel 5.4+** - AF_XDP support
- **Root privileges** - CAP_NET_ADMIN, CAP_BPF
- **XDP-capable NIC** - or veth pair for testing

## Quick Start (Linux)

```bash
# Install nightly
rustup install nightly

# Run test script
sudo ./kaos-driver/scripts/test-xdp.sh
```

## Docker (Linux host only)

```bash
# Build
docker build -f kaos-driver/Dockerfile.xdp -t kaos-xdp .

# Run with privileges
docker run --privileged --network=host \
  -v /sys/fs/bpf:/sys/fs/bpf \
  kaos-xdp 10.0.0.1:9000 10.0.0.2:9000
```

**Note:** Docker build requires Linux host (QEMU emulation too slow).


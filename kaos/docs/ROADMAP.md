# Kaos Roadmap

## Current Features

| Feature | Status | Crate |
|---------|--------|-------|
| Lock-free ring buffers | ✅ | kaos |
| SPSC, MPSC, SPMC, MPMC patterns | ✅ | kaos |
| Shared memory IPC | ✅ | kaos-ipc |
| Reliable UDP (NAK/ACK) | ✅ | kaos-rudp |
| Media driver | ✅ | kaos-driver |
| sendmmsg/recvmmsg (Linux) | ✅ | kaos-rudp |
| io_uring (Linux) | ✅ | kaos-driver |
| Metrics/counters | ✅ | kaos |
| Congestion control (AIMD) | ✅ | kaos-rudp |
| AF_XDP (Linux) | 🔧 | kaos-driver |

## Planned

| Feature | Priority | Description |
|---------|----------|-------------|
| UDP multicast | Medium | One-to-many messaging |
| Message archive | Low | Persistent stream storage |
| Cluster support | Low | Fault-tolerant replication |

## AF_XDP Status

Kernel bypass networking via `xsk-rs`. Requires:
- Rust nightly (edition 2024)
- Linux 5.4+ with CAP_NET_ADMIN, CAP_BPF
- `--features xdp` flag

See `kaos-driver/XDP_PLAN.md` for details.

## Performance Targets

| Metric | Current | Target |
|--------|---------|--------|
| Ring buffer throughput | 2.5 Gelem/s | 3+ Gelem/s |
| IPC latency | 2.3 ns | < 2 ns |
| RUDP throughput | 15 M/s | 20+ M/s |

## Non-Goals

- Full protocol compatibility with other systems
- Language bindings (Rust-only for now)
- GUI tooling


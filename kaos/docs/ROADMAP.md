# Kaos Roadmap

## Feature Comparison

| Feature | Kaos | Status |
|---------|------|--------|
| Lock-free ring buffers | ✅ | 2.5+ Gelem/s |
| SPSC, MPSC, SPMC, MPMC | ✅ | All patterns |
| Shared memory IPC | ✅ | 412 M/s, 2.4 ns |
| Media driver architecture | ✅ | Zero-syscall apps |
| Reliable UDP (NAK/ACK) | ✅ | In-order delivery |
| sendmmsg/recvmmsg | ✅ | Linux batched I/O |
| io_uring | ✅ | Linux async I/O |
| Metrics/counters | ✅ | Lightweight |
| Congestion control | ✅ | AIMD algorithm |
| AF_XDP kernel bypass | ✅ | Compiles, needs test |
| UDP multicast | ⬜ | Planned |
| Message archive | ⬜ | Planned |
| Cluster consensus | ⬜ | Planned |

## Performance (Verified)

| Metric | Kaos | Notes |
|--------|------|-------|
| Ring buffer | 2.5 Gelem/s | SPSC hot path |
| IPC throughput | 412 M/s | Docker emulation |
| IPC latency | 2.4 ns | Same process |
| Memory | 0 leaks | All patterns verified |

## What's Left to Compete

### High Priority
1. **UDP Multicast** - One-to-many pub/sub (key for market data)
2. **AF_XDP Testing** - Verify on real Linux + NIC
3. **Archive Module** - Persistent message storage

### Medium Priority
4. **Flow Control** - Back-pressure signaling
5. **Loss Recovery** - Improve NAK timing
6. **Multi-session** - Session multiplexing

### Low Priority
7. **Cluster Consensus** - Raft/Paxos for HA
8. **C/C++ Bindings** - FFI for other languages
9. **Admin Tools** - CLI for monitoring

## Architecture

```
┌─────────────────────────────────────────────────────────────┐
│                      APPLICATION                            │
│  ┌─────────┐  ┌─────────┐  ┌─────────┐  ┌─────────┐        │
│  │Producer │  │Consumer │  │Producer │  │Consumer │        │
│  └────┬────┘  └────┬────┘  └────┬────┘  └────┬────┘        │
│       │            │            │            │              │
│       ▼            ▼            ▼            ▼              │
│  ┌─────────────────────────────────────────────────────┐   │
│  │              SHARED MEMORY (mmap)                    │   │
│  │       Ring Buffers • Zero-Copy Reads • 2.4 ns       │   │
│  └─────────────────────────────────────────────────────┘   │
└─────────────────────────────────────────────────────────────┘
                              │
                              ▼
┌─────────────────────────────────────────────────────────────┐
│                    MEDIA DRIVER                             │
│  ┌──────────┐  ┌──────────┐  ┌──────────┐  ┌──────────┐    │
│  │sendmmsg  │  │io_uring  │  │ AF_XDP   │  │  RUDP    │    │
│  │(batch)   │  │(async)   │  │(bypass)  │  │(reliable)│    │
│  └──────────┘  └──────────┘  └──────────┘  └──────────┘    │
└─────────────────────────────────────────────────────────────┘
                              │
                              ▼
┌─────────────────────────────────────────────────────────────┐
│                      NETWORK                                │
└─────────────────────────────────────────────────────────────┘
```

## Crate Structure

| Crate | Purpose | Lines |
|-------|---------|-------|
| kaos | Core ring buffers | ~2000 |
| kaos-ipc | Shared memory IPC | ~300 |
| kaos-rudp | Reliable UDP | ~900 |
| kaos-driver | Media driver | ~400 |

## Testing

```bash
# Unit tests
cargo test --workspace

# Benchmarks
cargo bench -p kaos --bench bench_core
cargo bench -p kaos --bench bench_trace -- "100M"

# Memory check (macOS)
leaks --atExit -- ./target/release/examples/spsc_basic

# Docker (AF_XDP)
docker build -f kaos-driver/Dockerfile.xdp -t kaos-xdp .
docker run --rm --platform linux/amd64 --entrypoint kaos-bench kaos-xdp
```

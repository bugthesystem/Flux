# Kaos Roadmap

## Status

| Feature | Status |
|---------|--------|
| Lock-free ring buffers (SPSC/MPSC/SPMC/MPMC) | ✅ |
| Shared memory IPC (mmap) | ✅ |
| Media driver architecture | ✅ |
| Reliable UDP (NAK/ACK) | ✅ |
| Congestion control (AIMD) | ✅ |
| sendmmsg/recvmmsg (Linux) | ✅ |
| io_uring (Linux) | ✅ |
| AF_XDP kernel bypass | ⚠️ Compiles, needs testing |
| Tracing / Tracy profiler | ✅ |
| UDP multicast | ✅ |
| Message archive | ⬜ Planned |

## Architecture

```
┌─────────────────────────────────────────────────────────────┐
│                      APPLICATION                            │
│       Producer ───► Ring Buffer (2.1 G/s) ───► Consumer    │
└─────────────────────────────────────────────────────────────┘
                              │
                    Shared Memory (mmap)
                              │
┌─────────────────────────────────────────────────────────────┐
│                    MEDIA DRIVER                             │
│    sendmmsg  │  io_uring  │  AF_XDP  │  Reliable UDP       │
└─────────────────────────────────────────────────────────────┘
                              │
                           Network
```

## Constants Reference

| Constant | Value | Purpose |
|----------|-------|---------|
| `SEND_BUFFER_SIZE` | 64KB | Max UDP send buffer |
| `RECV_PACKET_SIZE` | 2KB | Per-packet receive buffer (> MTU 1500) |
| `RECV_BATCH_SIZE` | 64 | Packets per recvmmsg syscall |
| `SOCKET_BUFFER_SIZE` | 8MB | OS socket buffer for throughput |
| `MAX_MESSAGE_DATA_SIZE` | 1KB | Max payload in MessageSlot |

## Testing

```bash
cargo test --workspace           # Unit tests
cargo bench -p kaos              # Benchmarks
cargo clippy --workspace         # Lint
RUSTFLAGS="--cfg loom" cargo test -p kaos --test loom_ring_buffer --release  # Loom
```

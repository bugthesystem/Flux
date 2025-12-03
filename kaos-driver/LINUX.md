# Kaos Media Driver

## Benchmarks (macOS M1, 127.0.0.1)

| Test | Throughput | Latency | Speedup |
|------|------------|---------|---------|
| Syscall baseline | 0.4 M/s | 2,355 ns | 1x |
| Same-process IPC | 447 M/s | 2.2 ns | **1000x** |
| Two-process IPC | 159 M/s | 6.3 ns | **374x** |
| RUDP (reliable) | 10-15 M/s | ~100 ns | **25x** |

## Architecture

```
App ──mmap──► kaos-driver ──UDP──► Network
                 │
                 ├─ sendmmsg (Linux)
                 ├─ io_uring (--features uring)
                 └─ kaos-rudp (--features reliable)
```

## Features

| Feature | Status |
|---------|--------|
| Shared memory IPC | ✅ |
| sendmmsg/recvmmsg | ✅ Linux |
| io_uring | ✅ `--features uring` |
| Reliable UDP | ✅ `--features reliable` |
| Congestion control | 🔲 |
| AF_XDP | 🔲 |

## Usage

```bash
# IPC benchmark
cargo run -p kaos-driver --release --example bench -- ipc

# RUDP benchmark (two terminals)
cargo run -p kaos-driver --release --features reliable --example bench_rudp -- recv
cargo run -p kaos-driver --release --features reliable --example bench_rudp -- send

# Run driver
cargo run -p kaos-driver --release -- 127.0.0.1:9000 127.0.0.1:9001
cargo run -p kaos-driver --release --features reliable -- ...  # with RUDP
```

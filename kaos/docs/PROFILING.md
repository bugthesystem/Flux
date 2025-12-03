# Profiling

## Quick Start

```bash
cargo bench -p kaos                           # All benchmarks
cargo bench -p kaos --bench bench_core        # Core patterns
cargo bench -p kaos --bench bench_trace       # Real-world trace events
```

## Tools

### cargo-asm
```bash
cargo install cargo-asm
cargo asm --lib "kaos::disruptor::single::RingBuffer"
```

### Flamegraph
```bash
cargo install flamegraph
cargo flamegraph --bench bench_core -o flame.svg
```

### perf (Linux)
```bash
perf stat cargo bench -p kaos --bench bench_core
perf record -g cargo bench && perf report
```

### Instruments (macOS)
```bash
xcrun xctrace record --template 'Time Profiler' --launch -- target/release/deps/bench_*
```

## Key Metrics

| Slot | Throughput | Bandwidth |
|------|------------|-----------|
| 8B | 2.19 B/s | 17.5 GB/s |
| 32B | 1.08 B/s | 34.6 GB/s |
| 64B | 516 M/s | 33.0 GB/s |

## Optimizations Applied

- ✅ 128-byte cache-line padding
- ✅ Relaxed atomics on fast path
- ✅ Release-only publish
- ✅ Bitwise AND for modulo
- ✅ Batch operations

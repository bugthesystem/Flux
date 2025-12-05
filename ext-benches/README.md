# External Benchmarks

Comparison benchmarks against other libraries.

## disruptor-rs-bench

Compares Kaos ring buffer against [disruptor-rs](https://crates.io/crates/disruptor).

```bash
cd disruptor-rs-bench && cargo bench
```

## aeron-java-bench

Compares Kaos against [Aeron](https://aeron.io/) (Java).

**Requirements:** [jbang](https://www.jbang.dev/)

### IPC Benchmark
```bash
cd aeron-java-bench && jbang AeronBench.java
```

### UDP Benchmark (two terminals)
```bash
# Terminal 1
jbang AeronBench.java recv

# Terminal 2
jbang AeronBench.java send
```

### Multicast Benchmark (two terminals)
```bash
# Aeron
jbang AeronMulticast.java recv
jbang AeronMulticast.java send

# Kaos (from project root)
cargo run -p kaos-rudp --release --example multicast_bench -- recv
cargo run -p kaos-rudp --release --example multicast_bench -- send
```

## Results (Apple M3)

| Benchmark | Kaos | Aeron | Notes |
|-----------|------|-------|-------|
| Ring buffer (SPSC 8B) | 2.1 G/s | 1.7 G/s | Batch API |
| IPC (8B) | 380 M/s | 285 M/s | Shared memory |
| UDP (8B) | 3.1 M/s | 2.5 M/s | localhost |
| **Multicast (coalesced)** | **5.2 M/s** | 2.6 M/s | 175 msgs/packet |

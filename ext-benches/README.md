# External Benchmarks

Comparison benchmarks against other libraries. These are separate from the main workspace to avoid adding dependencies to Kaos.

## disruptor-rs-bench

Compares Kaos against [disruptor-rs](https://crates.io/crates/disruptor).

```bash
cd disruptor-rs-bench
cargo bench --bench bench_trace_events
```

## Running

Each benchmark is a standalone Cargo project. Navigate to the directory and run `cargo bench`.


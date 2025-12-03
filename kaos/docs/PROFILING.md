# Profiling & Memory Analysis

## Quick Start

```bash
cargo bench -p kaos --bench bench_core        # Core patterns
cargo bench -p kaos --bench bench_trace       # Trace events
```

## Memory Analysis

### macOS - leaks (Built-in)

```bash
# Build release
cargo build --release --example spsc_basic -p kaos

# Check for leaks at exit
leaks --atExit -- ./target/release/examples/spsc_basic

# Verbose output with allocation list
leaks --atExit --list -- ./target/release/examples/spsc_basic

# Export to Instruments for visualization
leaks --atExit --outputGraph=leak.memgraph -- ./target/release/examples/spsc_basic
open leak.memgraph
```

### macOS - valgrind (via Homebrew)

[LouisBrunner/valgrind-macos](https://github.com/LouisBrunner/valgrind-macos) provides macOS ARM support.

```bash
# Install (requires Xcode CLI tools)
brew tap LouisBrunner/valgrind
brew install --HEAD LouisBrunner/valgrind/valgrind

# Memory check
valgrind --leak-check=full ./target/release/examples/spsc_basic

# Generate suppressions for false positives
valgrind --gen-suppressions=all ./target/release/examples/spsc_basic
```

**Note:** macOS valgrind can be unstable. Use `leaks` for quick checks.

### Linux - valgrind

```bash
# Install
sudo apt install valgrind
cargo install cargo-valgrind

# Easy mode
cargo valgrind run --example spsc_basic -p kaos --release

# Direct usage
valgrind --leak-check=full --show-leak-kinds=all \
  ./target/release/examples/spsc_basic

# Cache analysis
valgrind --tool=cachegrind ./target/release/examples/spsc_basic
cg_annotate cachegrind.out.*

# Heap profiling
valgrind --tool=massif ./target/release/examples/spsc_basic
ms_print massif.out.*

# Thread errors
valgrind --tool=helgrind ./target/release/examples/spsc_basic
```

## CPU Profiling

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
xcrun xctrace record --template 'Time Profiler' \
  --launch -- ./target/release/examples/spsc_basic
```

### cargo-asm
```bash
cargo install cargo-asm
cargo asm --lib "kaos::disruptor::single::RingBuffer"
```

## Verified Results

| Component | Throughput | Memory |
|-----------|------------|--------|
| SPSC | 3.5 Gelem/s | 0 leaks ✅ |
| MPSC | 2.5 Gelem/s | 0 leaks ✅ |
| SPMC | 2.0 Gelem/s | 0 leaks ✅ |
| MPMC | 1.5 Gelem/s | 0 leaks ✅ |
| IPC | 450 M/s | 0 leaks ✅ |

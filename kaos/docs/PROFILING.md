# Kaos Profiling Guide

Performance analysis tools and techniques for optimizing kaos ring buffers.

## Quick Start

```bash
# Run all benchmarks
cargo bench -p kaos

# Fast benchmark (10M events, ~5ms)
cargo bench -p kaos --bench bench_fast

# All slot sizes comparison
cargo bench -p kaos --bench bench_all_slots
```

---

## Tools

### 1. cargo-asm (Assembly Inspection)

Inspect generated assembly for hot functions.

```bash
# Install
cargo install cargo-asm

# Usage (run from kaos/ directory)
cd kaos

# List available functions
cargo asm --lib | grep -E "claim|publish|consume"

# View assembly for specific function
cargo asm --lib "kaos::disruptor::spsc::message_ring_buffer::MessageRingBuffer::try_claim_slots_relaxed"
cargo asm --lib "kaos::disruptor::spsc::message_ring_buffer::MessageRingBuffer::publish_batch_relaxed"
cargo asm --lib "kaos::disruptor::spsc::message_ring_buffer::MessageRingBuffer::try_consume_batch_relaxed"
```

#### Key Functions to Analyze

| Function | Purpose | Target |
|----------|---------|--------|
| `try_claim_slots_relaxed` | Producer claims slots | < 50 instructions |
| `publish_batch_relaxed` | Make slots visible | 3 instructions ✅ |
| `try_consume_batch_relaxed` | Consumer reads batch | < 100 instructions |

### 2. cargo-flamegraph (CPU Profiling)

Generate flame graphs to identify CPU hotspots.

```bash
# Install
cargo install flamegraph

# macOS: Requires Xcode (not just CommandLineTools)
xcode-select --install
# If still fails, install full Xcode from App Store

# Linux: Works out of the box with perf
sudo apt install linux-tools-common linux-tools-generic

# Generate flamegraph
cargo flamegraph --bench bench_fast -o flamegraph.svg

# With specific benchmark
cargo flamegraph --bench bench_all_slots -o flamegraph_slots.svg

# Open in browser
open flamegraph.svg
```

#### Reading Flame Graphs

- **Width** = Time spent (wider = slower)
- **Height** = Call stack depth
- Look for wide bars in `kaos::disruptor` namespace
- Atomic operations (`core::sync::atomic`) should be minimal

### 3. cargo-valgrind (Memory Analysis)

Detect memory issues and cache behavior.

```bash
# Install (Linux only - valgrind doesn't work on macOS ARM)
cargo install cargo-valgrind
sudo apt install valgrind

# Memory check
cargo valgrind run --example spsc_message

# Cache analysis (cachegrind)
valgrind --tool=cachegrind target/release/examples/spsc_message

# Analyze output
cg_annotate cachegrind.out.*
```

### 4. perf (Linux Performance Counters)

```bash
# Install
sudo apt install linux-tools-common

# Record
perf record -g cargo bench -p kaos --bench bench_fast

# Report
perf report

# Stat (quick overview)
perf stat cargo bench -p kaos --bench bench_fast
```

### 5. Instruments (macOS)

```bash
# Profile with Time Profiler
xcrun xctrace record --template 'Time Profiler' --launch -- \
  target/release/deps/bench_fast-*

# Open trace
open *.trace
```

---

## Assembly Analysis Results

### publish_batch_relaxed (OPTIMAL ✅)

```asm
kaos::disruptor::spsc::message_ring_buffer::MessageRingBuffer::publish_batch_relaxed:
 sub     x8, x8, #1          ; Calculate end sequence
 stlr    x8, [x0]            ; Store-Release (single atomic write!)
 ret                         ; Return
```

**Analysis**: 3 instructions! This is perfect - just one atomic store with release semantics.

### try_claim_slots_relaxed

```asm
kaos::disruptor::spsc::message_ring_buffer::MessageRingBuffer::try_claim_slots_relaxed:
 stp     x29, x30, [sp, #-16]!   ; Stack setup
 mov     x29, sp
 ldr     x9, [x0]                ; Load producer sequence (Relaxed)
 add     x10, x9, x1             ; Calculate next sequence
 ldr     x11, [x0, #128]         ; Load gating sequence (cache-aligned!)
 cmn     x11, #1                 ; Check if initialized
 b.eq    LBB22_6                 ; Branch if first claim
 ...
 dmb     ishld                   ; Memory barrier (only when needed)
 ...
```

**Analysis**: 
- Uses relaxed atomics for fast path
- Memory barrier (`dmb ishld`) only on contention
- Gating sequence at offset 128 (cache-line aligned)

### try_consume_batch_relaxed

```asm
 ldr     x8, [x0, #320]      ; Load consumer sequences array
 add     x11, x8, x9, lsl, #7 ; Index into array (x128 for cache alignment)
 ldr     x8, [x11]           ; Load consumer position
 ...
 and     x8, x8, x11         ; Mask for ring buffer index
 ...
```

**Analysis**:
- Consumer sequences are 128-byte aligned (`lsl #7` = multiply by 128)
- Uses bitwise AND for modulo (fast!)
- No unnecessary memory barriers on hot path

---

## Optimization Checklist

### ✅ Already Optimized

- [x] Cache-line padding (128 bytes between producer/consumer)
- [x] Relaxed atomics on fast path
- [x] Release-only publish (no acquire needed)
- [x] Bitwise AND for modulo (`& mask` instead of `% size`)
- [x] Batch operations to amortize overhead
- [x] Zero-allocation consume path

### 🔍 Potential Improvements

1. **Prefetching**: Add `__builtin_prefetch` hints for next batch
2. **SIMD**: Use NEON/AVX for bulk slot initialization
3. **Huge Pages**: Use 2MB pages for large buffers
4. **CPU Pinning**: Bind producer/consumer to specific cores

---

## Benchmark Results Summary

### Slot Size vs Throughput (10M events)

| Slot | Messages/sec | Bandwidth |
|------|-------------|-----------|
| 8B   | 2.19 B/s    | 17.5 GB/s |
| 16B  | 1.88 B/s    | 30.1 GB/s |
| 32B  | 1.08 B/s    | **34.6 GB/s** ⭐ |
| 64B  | 516 M/s     | 33.0 GB/s |
| 128B | 120 M/s     | 15.4 GB/s |

**Recommendation**: Use `Slot32` for maximum raw throughput, `Slot8` for maximum messages/sec.

---

## Running Custom Profiles

### Profile Specific Code Path

```bash
# Build with debug symbols
RUSTFLAGS="-C debuginfo=2" cargo build --release -p kaos

# Run with profiler
perf record -g ./target/release/examples/spsc_message
perf report --hierarchy
```

### Generate Assembly for Benchmarks

```bash
# Compile bench with assembly output
RUSTFLAGS="--emit=asm" cargo bench -p kaos --bench bench_fast --no-run

# Find assembly file
find target -name "*.s" | xargs grep -l "bench_fast"
```

### Memory Bandwidth Test

```bash
# Measure memory bandwidth during benchmark
perf stat -e LLC-loads,LLC-load-misses,LLC-stores,LLC-store-misses \
  cargo bench -p kaos --bench bench_fast
```

---

## Troubleshooting

### cargo-flamegraph fails on macOS

```
xcode-select: error: tool 'xctrace' requires Xcode
```

**Fix**: Install full Xcode from App Store, not just CommandLineTools.

### cargo-asm can't find function

```
[ERROR]: could not find function at path "..."
```

**Fix**: 
1. Use exact function path (check with `cargo asm --lib`)
2. Generic functions need monomorphization - use concrete types
3. Try `--clean` build

### valgrind not available on macOS ARM

**Alternative**: Use Instruments.app or `leaks` command:
```bash
leaks --atExit -- ./target/release/examples/spsc_message
```


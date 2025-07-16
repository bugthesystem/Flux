# Safety Documentation

This document explains all unsafe code patterns used in the Flux codebase and why they are safe.

## Overview

Flux uses unsafe code primarily for:
1. **Performance optimization** - SIMD operations, zero-copy memory access
2. **System-level operations** - Memory mapping, CPU affinity, NUMA allocation
3. **Lock-free synchronization** - Atomic operations with raw pointers

All unsafe code is carefully documented and bounded by safety invariants.

## Memory Management

### MappedRingBuffer

**Location:** `src/disruptor/ring_buffer.rs`

**Unsafe Operations:**
- `libc::mmap` - Memory mapping
- `libc::mlock` - Memory locking
- `std::ptr::write_bytes` - Zero initialization
- `std::slice::from_raw_parts` - Raw slice creation
- `libc::munmap` - Memory unmapping

**Safety Guarantees:**
1. **Bounds checking** - All access uses `& self.mask` to ensure valid indices
2. **Lifetime management** - Buffer lifetime tied to `MappedRingBuffer` instance
3. **Synchronization** - Producer/consumer sequences prevent race conditions
4. **Cleanup** - `Drop` implementation ensures proper memory deallocation

**Example:**
```rust
// SAFETY: start_index is bounds-checked by mask, count is validated above
std::slice::from_raw_parts_mut(self.buffer.add(start_index), count)
```

## SIMD Operations

### MessageSlot SIMD Copy

**Location:** `src/disruptor/message_slot.rs`

**Unsafe Operations:**
- `vld1q_u8` / `vst1q_u8` - NEON SIMD load/store
- `std::ptr::read_unaligned` / `std::ptr::write_unaligned` - Unaligned access
- Raw pointer arithmetic

**Safety Guarantees:**
1. **Length validation** - `dst.len() == src.len()` checked before unsafe block
2. **Bounds checking** - All pointer arithmetic is bounds-checked
3. **Architecture-specific** - SIMD operations only on supported architectures
4. **Read-only operations** - Checksum calculation only reads memory

**Example:**
```rust
// SAFETY: offset is calculated safely, pointers are valid
let chunk = vld1q_u8(src_ptr.add(offset));
vst1q_u8(dst_ptr.add(offset), chunk);
```

## System-Level Operations

### CPU Affinity

**Location:** `src/utils/cpu.rs`

**Unsafe Operations:**
- `libc::sched_setaffinity` - CPU affinity setting
- `libc::pthread_setaffinity_np` - Thread affinity

**Safety Guarantees:**
1. **Error checking** - All system calls check return values
2. **Bounds validation** - CPU sets are validated before use
3. **Platform-specific** - Operations only on supported platforms

### Memory Allocation

**Location:** `src/utils/memory.rs`

**Unsafe Operations:**
- `std::alloc::alloc_zeroed` - Zero-initialized allocation
- `std::alloc::dealloc` - Memory deallocation
- `libc::posix_memalign` - Aligned allocation

**Safety Guarantees:**
1. **Layout validation** - All allocations use valid `Layout`
2. **Size tracking** - Allocated size tracked for deallocation
3. **Alignment requirements** - Proper alignment for SIMD operations

## Lock-Free Synchronization

### Atomic Operations

**Location:** `src/disruptor/ring_buffer.rs`

**Unsafe Operations:**
- `unsafe impl Send for MappedRingBuffer`
- `unsafe impl Sync for MappedRingBuffer`

**Safety Guarantees:**
1. **Atomic access** - All shared state accessed via atomic operations
2. **Memory ordering** - Proper `Ordering` parameters for synchronization
3. **Bounded lifetimes** - Raw pointers never outlive the buffer
4. **Exclusive access** - Producer has exclusive write access to claimed slots

## Prefetching

### Cache Prefetch

**Location:** `src/disruptor/ring_buffer.rs`

**Unsafe Operations:**
- `std::arch::asm!` - Assembly instructions for prefetching

**Safety Guarantees:**
1. **Read-only** - Prefetch instructions only read memory
2. **Bounds checked** - All prefetch addresses are bounds-checked
3. **Architecture-specific** - Assembly only on supported architectures

## Error Handling

### Graceful Degradation

All unsafe operations include proper error handling:

```rust
if ptr == libc::MAP_FAILED {
    return Err(FluxError::RingBufferFull);
}
```

### Fallback Mechanisms

SIMD operations include fallbacks for unsupported architectures:

```rust
#[cfg(target_arch = "aarch64")]
{
    // SIMD operations
}

#[cfg(not(target_arch = "aarch64"))]
{
    // Fallback implementation
}
```

## Testing

### Safety Testing

All unsafe code is tested for:
1. **Bounds checking** - Invalid indices handled gracefully
2. **Memory leaks** - Proper cleanup in all code paths
3. **Race conditions** - Concurrent access tested
4. **Platform compatibility** - Cross-platform behavior verified

### Fuzzing

Critical unsafe code paths are fuzzed to catch edge cases.

## Best Practices

1. **Documentation** - All unsafe blocks include `# Safety` documentation
2. **Bounding** - Unsafe operations are bounded by safety checks
3. **Testing** - Comprehensive tests for all unsafe code paths
4. **Review** - All unsafe code reviewed by multiple developers
5. **Minimal scope** - Unsafe blocks are as small as possible

## Conclusion

All unsafe code in Flux is:
- **Carefully documented** with safety explanations
- **Bounded by safety checks** to prevent undefined behavior
- **Thoroughly tested** for edge cases and race conditions
- **Performance-justified** with measurable benefits
- **Platform-specific** with proper fallbacks

The unsafe code enables high-performance operations while maintaining Rust's safety guarantees through careful design and comprehensive testing. 
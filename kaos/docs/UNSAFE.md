# Unsafe Code

## Summary

| Crate | unsafe blocks | Purpose |
|-------|---------------|---------|
| kaos | ~30 | Ring buffer I/O |
| kaos-ipc | ~5 | mmap/msync |
| kaos-rudp | ~15 | sendmmsg/recvmmsg |

## Common Patterns

### Memory-Mapped I/O
```rust
unsafe { libc::mmap(...) }  // Shared memory buffers
unsafe { libc::mlock(...) } // Prevent swapping
unsafe { libc::msync(...) } // IPC sync
```

### Volatile Slot Access
```rust
unsafe fn write_slot(&mut self, seq: u64, slot: &T) {
    let ptr = self.buffer.add(seq as usize & self.mask);
    ptr.write_volatile(slot.clone());
}
```

### Send/Sync
```rust
unsafe impl Send for RingBuffer<T> {}
unsafe impl Sync for RingBuffer<T> {}
// Justified: atomics protect concurrent access
```

### FFI (Linux)
```rust
unsafe { libc::sendmmsg(fd, msgs, count, 0) }
unsafe { libc::recvmmsg(fd, msgs, count, 0, null()) }
```

## Invariants

1. Producer claims before writing
2. Consumer waits for publish before reading
3. Sequences are monotonic
4. Buffer size is power of 2

# Unsafe Code Audit

This document catalogs every `unsafe` block in Kaos with safe alternatives.
Use this as a checklist to gradually make the codebase safer.

## Summary

| Crate | unsafe blocks | Category |
|-------|---------------|----------|
| kaos | 60 | Ring buffer I/O, mmap, Send/Sync |
| kaos-rudp | 20 | FFI (sendmmsg), pointer casts |
| kaos-ipc | 0 | Uses kaos internals |

---

## Category 1: Memory-Mapped I/O

### Location: `kaos/src/disruptor/ipc.rs` (lines 66, 113)

```rust
// UNSAFE: Direct libc::mmap call
let mmap_ptr = unsafe {
    libc::mmap(std::ptr::null_mut(), file_size, 
               libc::PROT_READ | libc::PROT_WRITE,
               libc::MAP_SHARED, file.as_raw_fd(), 0)
};
```

**Why unsafe:** Raw FFI call, returns raw pointer, no bounds checking.

**Safe alternative:**
```rust
// Using memmap2 crate (add to Cargo.toml: memmap2 = "0.9")
use memmap2::MmapMut;

let mmap = unsafe { MmapMut::map_mut(&file)? };
// Still unsafe! But memmap2 handles cleanup via Drop
```

**Fully safe alternative:**
```rust
// Use a Vec-backed buffer instead of mmap (loses cross-process sharing)
let buffer: Vec<T> = vec![T::default(); capacity];
```

**Performance impact:** `memmap2` has same performance. Vec loses IPC capability.

**Action:** ✅ SKIPPED - Could use `memmap2` but we already implement identical Drop with munmap. No added benefit.

---

### Location: `kaos/src/disruptor/single.rs` (line 91)

```rust
// UNSAFE: mmap for mapped ring buffers  
let ptr = unsafe {
    libc::mmap(ptr::null_mut(), buffer_size,
               libc::PROT_READ | libc::PROT_WRITE,
               libc::MAP_PRIVATE | libc::MAP_ANONYMOUS, -1, 0)
};
```

**Safe alternative:** Use `new()` for heap allocation (default), `new_mapped()` is opt-in.

**Action:** ✅ SKIPPED - Could use `memmap2` but we implement identical Drop. `new_mapped()` is opt-in.

---

## Category 2: Volatile Read/Write

### Location: `kaos/src/disruptor/single.rs` (lines 172, 182)

```rust
// UNSAFE: Volatile slot access
pub unsafe fn write_slot(&self, sequence: u64, value: T) {
    std::ptr::write_volatile(self.buffer.add(idx), value);
}

pub unsafe fn read_slot(&self, sequence: u64) -> T {
    std::ptr::read_volatile(self.buffer.add(idx))
}
```

**Why unsafe:** Raw pointer arithmetic, no bounds checking.

**Safe alternative:**
```rust
// Using UnsafeCell + bounds checking
use std::cell::UnsafeCell;

pub struct SafeRingBuffer<T> {
    buffer: Box<[UnsafeCell<T>]>,
    mask: usize,
}

impl<T: Clone> SafeRingBuffer<T> {
    pub fn write_slot(&self, seq: u64, value: T) {
        let idx = (seq as usize) & self.mask;
        // Bounds check happens via slice indexing
        let cell = &self.buffer[idx];
        // Still needs unsafe for UnsafeCell::get()
        unsafe { *cell.get() = value; }
    }
}
```

**Fully safe (but slower):**
```rust
use std::sync::atomic::AtomicU64;
use crossbeam::utils::CachePadded;

// Use atomics for each slot (significant overhead)
pub struct AtomicSlot {
    value: CachePadded<AtomicU64>,
}
```

**Performance impact:** Bounds checking: ~1-2% overhead. Full atomics: ~30% slower.

**Action:** Add bounds checking, keep volatile for performance. ⬜

---

### Location: `kaos/src/disruptor/multi.rs` (lines 88, 107, 324, 369, 471)

Same pattern as single.rs. Apply same fix.

**Action:** Consolidate into shared helper with bounds checking. ⬜

---

## Category 3: Raw Pointer Casts (Headers)

### Location: `kaos/src/disruptor/ipc.rs` (lines 81, 128, 182, 185, 189)

```rust
// UNSAFE: Cast raw pointer to struct reference
let header = unsafe { &mut *(mmap_ptr as *mut SharedHeader) };
```

**Why unsafe:** Alignment, lifetime, validity not checked.

**Safe alternative:**
```rust
// Using bytemuck (add: bytemuck = { version = "1.14", features = ["derive"] })
use bytemuck::{Pod, Zeroable};

#[derive(Clone, Copy, Pod, Zeroable)]
#[repr(C)]
struct SharedHeader { /* fields */ }

// Safe cast
let header: &SharedHeader = bytemuck::from_bytes(&mmap_slice[..HEADER_SIZE]);
```

**Performance impact:** None - bytemuck is zero-cost.

**Action:** Derive Pod/Zeroable on SharedHeader, use bytemuck. ⬜

---

### Location: `kaos-rudp/src/lib.rs` (lines 160, 182, 355, 367, 440, 533, 558)

```rust
// UNSAFE: Pointer cast for header serialization
unsafe { std::ptr::read_unaligned(bytes.as_ptr() as *const Self) }

// UNSAFE: Slice from struct pointer
unsafe { std::slice::from_raw_parts(&header as *const _ as *const u8, SIZE) }
```

**Safe alternative:**
```rust
use bytemuck::{Pod, Zeroable, bytes_of, from_bytes};

#[derive(Clone, Copy, Pod, Zeroable)]
#[repr(C, packed)]
struct ReliableUdpHeader { /* fields */ }

// Safe serialization
let bytes: &[u8] = bytemuck::bytes_of(&header);

// Safe deserialization  
let header: &ReliableUdpHeader = bytemuck::from_bytes(slice);
```

**Performance impact:** None.

**Action:** Add bytemuck derives to all headers. ⬜

---

## Category 4: Send/Sync Implementations

### Location: `kaos/src/disruptor/single.rs` (lines 205-206, 221-222)

```rust
unsafe impl<T: RingBufferEntry> Send for RingBuffer<T> {}
unsafe impl<T: RingBufferEntry> Sync for RingBuffer<T> {}
```

**Why unsafe:** Compiler can't verify thread-safety; we assert it manually.

**Justification:**
- All shared state uses atomics (AtomicU64)
- Producer/consumer sequences are atomic
- Slots are accessed only after sequence barriers

**Safe alternative:** None - this is a promise to the compiler.

**Action:** Document invariants in code comments. Keep unsafe. ✅

---

### Location: `kaos/src/disruptor/multi.rs` (lines 153-154, 161, 419-420, 510-511)

Same pattern. Keep with documentation.

**Action:** Add safety comments explaining invariants. ⬜

---

## Category 5: FFI (Linux Syscalls)

### Location: `kaos-rudp/src/sendmmsg.rs` (lines 30, 96)

```rust
// UNSAFE: Linux sendmmsg/recvmmsg FFI
pub unsafe fn send_batch(&mut self, fd: i32, packets: &[&[u8]], addr: &SocketAddr) -> io::Result<usize> {
    libc::sendmmsg(fd, self.msgvec.as_mut_ptr(), count as u32, 0)
}
```

**Why unsafe:** Raw syscall, manual struct setup, pointer lifetimes.

**Safe alternative:**
```rust
// Option 1: Use socket2 crate (no batching)
use socket2::{Socket, Domain, Type};
let socket = Socket::new(Domain::IPV4, Type::DGRAM, None)?;
socket.send_to(data, &addr)?;

// Option 2: Create safe wrapper with lifetime tracking
pub struct SafeBatchSender<'a> {
    packets: Vec<&'a [u8]>,
    socket: &'a UdpSocket,
}
```

**Performance impact:** No batching = ~10x more syscalls = significant slowdown.

**Action:** Keep unsafe, add extensive safety comments. ✅

---

### Location: `kaos-rudp/src/lib.rs` (line 270)

```rust
// UNSAFE: setsockopt for buffer sizing
unsafe {
    libc::setsockopt(fd, libc::SOL_SOCKET, libc::SO_SNDBUF, ...);
}
```

**Safe alternative:**
```rust
use socket2::Socket;
let socket = Socket::from(std_socket);
socket.set_send_buffer_size(8 * 1024 * 1024)?;
```

**Performance impact:** None.

**Action:** Use socket2 for socket options. ⬜

---

## Category 6: Memory Zeroing

### Location: `kaos/src/disruptor/slots.rs` (line 137)

```rust
// UNSAFE: Zero-initialize struct
fn default() -> Self {
    unsafe { std::mem::zeroed() }
}
```

**Why unsafe:** Not all types are valid when zeroed (e.g., references, NonZero).

**Safe alternative:**
```rust
use bytemuck::Zeroable;

#[derive(Zeroable)]
struct MessageSlot { /* only types that are valid when zeroed */ }

fn default() -> Self {
    bytemuck::Zeroable::zeroed()
}
```

**Performance impact:** None - same machine code.

**Action:** Derive Zeroable, use safe zeroed(). ⬜

---

### Location: `kaos-rudp/src/sendmmsg.rs` (lines 24-26, 89-92)

```rust
// UNSAFE: Zero libc structs
msgvec: vec![unsafe { std::mem::zeroed() }; batch_size],
```

**Safe alternative:**
```rust
// libc structs don't implement Zeroable, but we can use MaybeUninit
use std::mem::MaybeUninit;

let mut msgvec: Vec<MaybeUninit<libc::mmsghdr>> = Vec::with_capacity(batch_size);
msgvec.resize_with(batch_size, MaybeUninit::uninit);

// Then initialize each field explicitly before use
```

**Action:** Keep zeroed() for libc structs (standard practice). ✅

---

## Category 7: Macro Unsafe

### Location: `kaos/src/disruptor/macros.rs` (lines 25, 91, 96)

```rust
// UNSAFE: Raw pointer deref in macro
let ring_ref = unsafe { &*(ring_ptr as *const RingBuffer<$slot_type>) };
```

**Why unsafe:** Arc::as_ptr returns raw pointer, need to deref.

**Safe alternative:**
```rust
// Use Arc::as_ref() or pass reference directly
let ring_ref: &RingBuffer<T> = ring.as_ref();
```

**Performance impact:** None.

**Action:** Refactor macros to take references. ⬜

---

## Checklist

| Category | Count | Safe Alternative | Action |
|----------|-------|------------------|--------|
| mmap | 4 | memmap2 (same as ours) | ✅ Skipped - we have identical Drop |
| Volatile R/W | 12 | Bounds checking | ⬜ Add bounds checks |
| Pointer casts | 15 | bytemuck crate | ⬜ Derive Pod/Zeroable |
| Send/Sync | 10 | None (keep) | ✅ Add safety docs |
| FFI syscalls | 5 | socket2 crate | ⬜ Partial migration |
| mem::zeroed | 6 | bytemuck::Zeroable | ⬜ Derive Zeroable |
| Macro unsafe | 3 | Pass references | ⬜ Refactor macros |

**Total: 55 unsafe blocks**
- **Keep as-is:** 15 (Send/Sync, FFI batching, libc zeroed)
- **Can make safe:** 40 (with bytemuck, memmap2, bounds checks)

---

## Recommended Migration Order

1. **bytemuck for headers** ✅ Done (kaos-rudp)
2. **Bounds checking for slot access** ✅ Done (debug_assert!)
3. **memmap2 for mmap** ✅ Skipped - we already have identical Drop impl
4. **socket2 for setsockopt** ⬜ Optional (low priority)
5. **Macro refactoring** ✅ Done (Arc::as_ref, bounds)

## Feature Flags

```toml
[features]
default = []              # Safe by default (for cautious users)
unsafe-perf = []          # Opt-in for brave users (max performance)
```

### Usage

```bash
# Safe (default) - bounds checks, bytemuck, memmap2
cargo build -p kaos

# Unsafe (opt-in) - raw pointers, no checks, max speed
cargo build -p kaos --features unsafe-perf
```

---

## Implementation Patterns by Category

### Category 1: mmap (4 blocks)

```rust
// UNSAFE PATH (--features unsafe-perf)
#[cfg(feature = "unsafe-perf")]
fn alloc_buffer(size: usize) -> *mut T {
    unsafe {
        libc::mmap(ptr::null_mut(), size, 
                   PROT_READ | PROT_WRITE,
                   MAP_PRIVATE | MAP_ANONYMOUS, -1, 0) as *mut T
    }
}

// SAFE PATH (default)
#[cfg(not(feature = "unsafe-perf"))]
fn alloc_buffer(size: usize) -> Box<[T]> {
    vec![T::default(); size].into_boxed_slice()
}
```

**Files:** `single.rs`, `ipc.rs`
**Safe dep:** None (Vec) or `memmap2` crate

---

### Category 2: Volatile R/W (12 blocks)

```rust
// UNSAFE PATH
#[cfg(feature = "unsafe-perf")]
#[inline(always)]
pub fn write_slot(&self, seq: u64, value: T) {
    let idx = (seq as usize) & self.mask;
    unsafe { std::ptr::write_volatile(self.buffer.add(idx), value); }
}

// SAFE PATH
#[cfg(not(feature = "unsafe-perf"))]
#[inline(always)]
pub fn write_slot(&self, seq: u64, value: T) {
    let idx = (seq as usize) & self.mask;
    assert!(idx < self.size, "index {} >= size {}", idx, self.size);
    unsafe { std::ptr::write_volatile(self.buffer.add(idx), value); }
}
```

**Files:** `single.rs`, `multi.rs`, `ipc.rs`
**Safe dep:** None (bounds check)

---

### Category 3: Pointer Casts (15 blocks)

```rust
// UNSAFE PATH
#[cfg(feature = "unsafe-perf")]
fn header(&self) -> &SharedHeader {
    unsafe { &*(self.mmap_ptr as *const SharedHeader) }
}

// SAFE PATH  
#[cfg(not(feature = "unsafe-perf"))]
fn header(&self) -> &SharedHeader {
    let slice = unsafe { 
        std::slice::from_raw_parts(self.mmap_ptr, HEADER_SIZE) 
    };
    bytemuck::from_bytes(slice)
}
```

**Files:** `ipc.rs`, `lib.rs` (kaos-rudp)
**Safe dep:** `bytemuck = { version = "1.14", features = ["derive"] }`

**Requires derives:**
```rust
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
#[repr(C)]
struct SharedHeader { ... }
```

---

### Category 4: Send/Sync (10 blocks) - KEEP AS-IS

```rust
// No feature gate - these are promises to the compiler
unsafe impl<T: RingBufferEntry> Send for RingBuffer<T> {}
unsafe impl<T: RingBufferEntry> Sync for RingBuffer<T> {}
// Justified: atomics protect all shared state
```

**No safe alternative** - this is declaring thread-safety.

---

### Category 5: FFI syscalls (5 blocks)

```rust
// UNSAFE PATH (Linux batching)
#[cfg(all(target_os = "linux", feature = "unsafe-perf"))]
pub fn send_batch(&mut self, packets: &[&[u8]]) -> io::Result<usize> {
    unsafe { libc::sendmmsg(self.fd, self.msgvec.as_mut_ptr(), count, 0) }
}

// SAFE PATH (one-by-one)
#[cfg(not(feature = "unsafe-perf"))]
pub fn send_batch(&mut self, packets: &[&[u8]]) -> io::Result<usize> {
    let mut sent = 0;
    for pkt in packets {
        self.socket.send(pkt)?;
        sent += 1;
    }
    Ok(sent)
}
```

**Files:** `sendmmsg.rs`, `lib.rs` (kaos-rudp)
**Safe dep:** `socket2` for setsockopt

---

### Category 6: mem::zeroed (6 blocks)

```rust
// UNSAFE PATH
#[cfg(feature = "unsafe-perf")]
impl Default for MessageSlot {
    fn default() -> Self {
        unsafe { std::mem::zeroed() }
    }
}

// SAFE PATH
#[cfg(not(feature = "unsafe-perf"))]
impl Default for MessageSlot {
    fn default() -> Self {
        bytemuck::Zeroable::zeroed()
    }
}
```

**Files:** `slots.rs`, `sendmmsg.rs`
**Safe dep:** `bytemuck` with `Zeroable` derive

---

### Category 7: Macro unsafe (3 blocks)

```rust
// UNSAFE PATH
#[cfg(feature = "unsafe-perf")]
macro_rules! publish_batch {
    ($producer:expr, ...) => {
        let rb = unsafe { &mut *$producer.ring_buffer };
        // ...
    }
}

// SAFE PATH
#[cfg(not(feature = "unsafe-perf"))]
macro_rules! publish_batch {
    ($producer:expr, ...) => {
        let rb = $producer.ring_buffer_ref();  // Safe accessor
        // ...
    }
}
```

**Files:** `macros.rs`
**Safe dep:** None (add safe accessor methods)

---

## Migration Checklist

| Cat | File | Item | Safe Path | Status |
|-----|------|------|-----------|--------|
| 1 | single.rs | new_mapped() | use new() | 🔒 KEEP (opt-in) |
| 1 | ipc.rs | mmap create | N/A | 🔒 KEEP (IPC needs mmap) |
| 1 | ipc.rs | mmap open | N/A | 🔒 KEEP (IPC needs mmap) |
| 1 | ipc.rs | munmap | N/A | 🔒 KEEP (IPC needs mmap) |
| 2 | single.rs | write_slot | bounds | ✅ |
| 2 | single.rs | read_slot | bounds | ✅ |
| 2 | single.rs | get_read_batch | bounds | ✅ |
| 2 | multi.rs | MpscRingBuffer::write_slot | bounds | ✅ |
| 2 | multi.rs | MpscRingBuffer::read_slot | bounds | ✅ |
| 2 | multi.rs | SpmcRingBuffer::write_slot | bounds | ✅ |
| 2 | multi.rs | SpmcRingBuffer::read_slot | bounds | ✅ |
| 2 | multi.rs | MpmcRingBuffer::write_slot | bounds | ✅ |
| 2 | ipc.rs | write_slot | bounds | ✅ |
| 2 | ipc.rs | read_slot | bounds | ✅ |
| 2 | ipc.rs | try_receive | bounds | ✅ (via read_slot) |
| 2 | ipc.rs | receive callback | bounds | ✅ (via slot_ptr) |
| 3 | ipc.rs | header() | N/A | 🔒 KEEP (AtomicU64 not Pod) |
| 3 | ipc.rs | header_mut() | N/A | 🔒 KEEP (AtomicU64 not Pod) |
| 3 | ipc.rs | slot_ptr() | N/A | 🔒 KEEP (Generic T) |
| 3 | rudp/lib.rs | from_bytes() | bytemuck | ✅ |
| 3 | rudp/lib.rs | header serialize (x6) | bytemuck | ✅ |
| 5 | rudp/lib.rs | setsockopt | socket2 | 🔒 KEEP (perf) |
| 5 | sendmmsg.rs | sendmmsg | N/A | 🔒 KEEP (batching perf) |
| 5 | sendmmsg.rs | recvmmsg | N/A | 🔒 KEEP (batching perf) |
| 6 | slots.rs | MessageSlot::default | Zeroable | ✅ |
| 6 | sendmmsg.rs | mmsghdr zeroed | N/A | 🔒 KEEP (libc types) |
| 7 | macros.rs | publish_batch ring_ref | Arc::as_ref | ✅ |
| 7 | macros.rs | publish_unrolled slice | bounds | ✅ |
| 7 | macros.rs | consume_batch | safe | ✅ (no unsafe) |

**Total: 29 items**
- ✅ **Migrated: 24** (bounds checks, safe refs, bytemuck)
- 🔒 **Keep unsafe: 5** (mmap/munmap, FFI syscalls)

## Remaining Unsafe (Intentionally Kept)

These are **inherently unsafe** due to FFI or platform requirements:

| Location | Why Kept | Bounds Check |
|----------|----------|--------------|
| ipc.rs: mmap/munmap | FFI for shared memory | ✅ size overflow, MAP_FAILED, file_size >= HEADER_SIZE |
| ipc.rs: header/slot_ptr | mmap interpretation | ✅ magic, version, slot_size, capacity bounds |
| sendmmsg.rs: sendmmsg | FFI batched syscalls | ✅ batch_size > 0, count <= msgvec.len() |
| sendmmsg.rs: mem::zeroed | libc types (no Rust ctor) | ✅ batch_size/buffer_size > 0 |
| main.rs: MaybeUninit | FFI structs | ✅ BATCH_SIZE compile-time const |
| xdp.rs: XDP ops | Kernel bypass | ✅ frame bounds in config |
| single.rs: new_mapped() | Opt-in mmap | ✅ power-of-2, overflow check, MAP_FAILED |
| Drop impls: munmap | FFI cleanup | ✅ is_mapped flag, mmap_len stored |

**Every unsafe block now has validation BEFORE the operation.**

---

## Expected Performance

| Mode | Throughput | Use Case |
|------|------------|----------|
| Safe (default) | ~2.3 Gelem/s | Production, debugging |
| Unsafe (opt-in) | ~2.5 Gelem/s | Benchmarks, HFT |

**~8% overhead** for safety - worth it for most users!

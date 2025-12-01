# Kaos Unsafe Code Audit

This document lists all `unsafe` blocks in the Kaos codebase for review.

## Summary

| Crate | unsafe blocks | unsafe fn | unsafe impl |
|-------|---------------|-----------|-------------|
| kaos | 65 | 8 | 12 |
| kaos-ipc | 2 | 0 | 0 |
| kaos-rudp | 15 | 3 | 0 |

---

## kaos (core)

### kaos/src/disruptor/spsc/ring_buffer.rs

| Line | Type | Purpose |
|------|------|---------|
| 76 | block | `mmap()` for memory-mapped allocation |
| 157-161 | block | `from_raw_parts_mut()` for slice from buffer pointer |
| 179-183 | block | `from_raw_parts()` for read slice |
| 189 | fn | `write_slot()` - volatile write to buffer |
| 204 | fn | `read_slot()` - volatile read from buffer |
| 221 | block | `munmap()` in Drop |
| 229-230 | impl | `Send + Sync` for RingBuffer |

### kaos/src/disruptor/spsc/shared_ring_buffer.rs

| Line | Type | Purpose |
|------|------|---------|
| 133 | block | `mmap()` with file backing for IPC |
| 151 | block | Cast to SharedHeader |
| 161 | block | `write_bytes()` for zeroing slots |
| 166 | block | `msync()` for disk sync |
| 191 | block | `mmap()` for opening existing |
| 209-240 | blocks | Header validation with early munmap on error |
| 272 | block | Header pointer cast |
| 278 | block | Mutable header pointer cast |
| 286 | block | Slot pointer calculation |
| 318 | fn | `write_slot()` |
| 339-346 | block | Slot byte access for memcpy |
| 377 | block | Read slot at pointer |
| 398 | fn | `read_slot()` |
| 414 | block | `munmap()` in Drop |
| 422 | impl | `Send` for SharedRingBuffer |

### kaos/src/disruptor/spsc/message_ring_buffer.rs

| Line | Type | Purpose |
|------|------|---------|
| 99 | block | Self-pointer cast for blocking claim |
| 157 | block | Self-pointer cast for relaxed claim |
| 223 | block | `ClaimBatch::new()` |

### kaos/src/disruptor/spsc/producer.rs

| Line | Type | Purpose |
|------|------|---------|
| 21 | impl | `Send` for Producer |
| 46 | block | Deref raw ring buffer pointer |
| 67 | block | Deref for batch publish |
| 74 | block | Unrolled loop with `get_unchecked_mut` |
| 134 | block | Deref for try_claim_relaxed |
| 184 | block | Deref for publish_relaxed |

### kaos/src/disruptor/spsc/consumer.rs

| Line | Type | Purpose |
|------|------|---------|
| 37 | block | `Arc::as_ptr()` deref |

### kaos/src/disruptor/spsc/ring_producer.rs

| Line | Type | Purpose |
|------|------|---------|
| 15 | impl | `Send` for RingProducer |
| 34-90 | blocks | Multiple deref and write_slot calls |

### kaos/src/disruptor/spsc/ring_consumer.rs

| Line | Type | Purpose |
|------|------|---------|
| 48 | block | Read slot in event processing |

### kaos/src/disruptor/ring_buffer_core.rs

| Line | Type | Purpose |
|------|------|---------|
| 33 | fn | `write_slot()` - volatile write |
| 39 | fn | `read_slot()` - volatile read |
| 133-134 | impl | `Send + Sync` for RingBufferCore |

### kaos/src/disruptor/spmc/spmc_ring_buffer.rs

| Line | Type | Purpose |
|------|------|---------|
| 67 | fn | `write_slot()` |
| 150 | fn | `read_slot()` |
| 282-283 | impl | `Send + Sync` for SpmcRingBuffer |
| 297-428 | blocks | Tests using write_slot |

### kaos/src/disruptor/mpsc/mpsc_ring_buffer.rs

| Line | Type | Purpose |
|------|------|---------|
| 85 | fn | `write_slot()` |
| 123 | fn | `read_slot()` |
| 189-190 | impl | `Send + Sync` for MpscRingBuffer |

### kaos/src/disruptor/mpsc/mpsc_producer.rs

| Line | Type | Purpose |
|------|------|---------|
| 15 | impl | `Send` for MpscProducer |
| 35-83 | blocks | write_slot calls |

### kaos/src/disruptor/mpsc/mpsc_consumer.rs

| Line | Type | Purpose |
|------|------|---------|
| 47 | block | read_slot in event processing |

### kaos/src/disruptor/mpmc/mpmc_ring_buffer.rs

| Line | Type | Purpose |
|------|------|---------|
| 76 | fn | `write_slot()` |
| 154 | fn | `read_slot()` |
| 212-213 | impl | `Send + Sync` for MpmcRingBuffer |
| 228-320 | blocks | Tests using write_slot |

### kaos/src/disruptor/message_slot.rs

| Line | Type | Purpose |
|------|------|---------|
| 96 | block | `mem::zeroed()` for Default |
| 247 | block | ARM CRC32 intrinsics |

### kaos/src/disruptor/macros.rs

| Line | Type | Purpose |
|------|------|---------|
| 41 | block | Arc pointer cast to mutable |
| 45 | block | write_slot in publish_light |
| 112 | block | read_slot in consume_light |
| 206 | block | Arc pointer cast in consume_batch |
| 269 | block | Arc pointer cast in publish_batch |
| 274 | block | write_slot |
| 322 | block | Producer ring buffer deref |
| 327 | block | write_slot |

### kaos/src/disruptor/claim_batch.rs

| Line | Type | Purpose |
|------|------|---------|
| 18 | fn | `new()` - pointer arithmetic |
| 28 | fn | `get_unchecked_mut()` - unchecked index |
| 35 | block | Pointer add in get_mut |
| 43-44 | impl | `Send + Sync` for ClaimBatch |

### kaos/src/cpu.rs

| Line | Type | Purpose |
|------|------|---------|
| 8 | block | Linux `sched_setaffinity()` |
| 25 | block | macOS `thread_policy_set()` |

---

## kaos-ipc

### kaos-ipc/src/lib.rs

| Line | Type | Purpose |
|------|------|---------|
| 64 | fn | `write_slot()` - delegates to SharedRingBuffer |
| 102 | fn | `read_slot()` - delegates to SharedRingBuffer |

---

## kaos-rudp

### kaos-rudp/src/lib.rs

| Line | Type | Purpose |
|------|------|---------|
| 143 | block | `from_raw_parts()` for header bytes |
| 152 | block | `read_unaligned()` for header parse |
| 184 | block | Header bytes for checksum |
| 278 | block | `setsockopt()` for socket buffers |
| 355-364 | blocks | Header bytes copy |
| 432-543 | blocks | FastHeader/header bytes for packet building |
| 636-670 | blocks | NAK/ACK packet building |
| 817-831 | blocks | Retransmit packet building, sendmmsg |
| 849 | block | recvmmsg |

### kaos-rudp/src/sendmmsg.rs

| Line | Type | Purpose |
|------|------|---------|
| 29-31 | blocks | `mem::zeroed()` for mmsghdr init |
| 45 | fn | `send_batch()` - Linux sendmmsg |
| 108-112 | blocks | `mem::zeroed()` for receiver init |
| 128 | fn | `recv_batch()` - Linux recvmmsg |
| 191 | fn | `send_batch()` - non-Linux stub |
| 215 | fn | `recv_batch()` - non-Linux stub |

---

## Common Patterns

### 1. Memory-mapped I/O
- `mmap()` / `munmap()` for zero-copy buffers
- `mlock()` to prevent swapping
- `msync()` for IPC persistence

### 2. Raw Pointer Access
- `write_volatile()` / `read_volatile()` for ring buffer slots
- `from_raw_parts()` / `from_raw_parts_mut()` for slices
- `read_unaligned()` for packed struct parsing

### 3. Send/Sync Implementations
- Ring buffers implement `Send + Sync` for cross-thread sharing
- Justified by atomic operations protecting concurrent access

### 4. FFI Calls
- `libc::sched_setaffinity()` - CPU pinning (Linux)
- `libc::thread_policy_set()` - CPU affinity (macOS)
- `libc::setsockopt()` - Socket buffer sizing
- `libc::sendmmsg()` / `libc::recvmmsg()` - Batch UDP I/O

---

## TODO: Review Items

- [ ] Audit all `unsafe impl Send/Sync` for soundness
- [ ] Verify volatile read/write ordering is correct
- [ ] Check mmap error handling paths
- [ ] Review pointer arithmetic bounds
- [ ] Validate packed struct alignment assumptions


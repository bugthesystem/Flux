//! RingBuffer - High performance generic SPSC ring buffer
//!
//! Primary ring buffer implementation supporting any slot type.
//!
//! ## Allocation Strategies
//!
//! - `new()` - Standard heap allocation
//! - `new_mapped()` - Memory-mapped with mlock (faster, no page faults)
//!
//! ## APIs
//!
//! For maximum throughput, use slice-based access:
//! - `try_claim_slots()` - Claim and get mutable slice for batch writes
//! - `get_read_batch()` - Get slice for batch reads
//!
//! For simpler code, use per-slot access:
//! - `write_slot()` / `read_slot()` - Individual slot access

use std::ptr;
use std::sync::atomic::{ AtomicU64, Ordering };
use std::sync::Arc;
use crate::disruptor::RingBufferEntry;
use crate::error::{ FluxError, Result };
use crate::constants::CACHE_PREFETCH_LINES;

/// High performance generic SPSC ring buffer
pub struct RingBuffer<T: RingBufferEntry> {
    /// Direct pointer to buffer (no enum dispatch in hot path!)
    buffer: *mut T,
    /// Buffer size
    size: usize,
    /// Mask for fast index calculation
    mask: usize,
    /// Producer cursor
    producer_cursor: Arc<AtomicU64>,
    /// Consumer cursor
    consumer_cursor: Arc<AtomicU64>,
    /// Keep heap allocation alive (None for mmap)
    _heap: Option<Box<[T]>>,
    /// Is this mmap'd? (for Drop)
    is_mapped: bool,
}

impl<T: RingBufferEntry> RingBuffer<T> {
    /// Create with heap allocation
    pub fn new(size: usize) -> Result<Self> {
        if !size.is_power_of_two() {
            return Err(FluxError::config("Size must be power of 2"));
        }

        let buffer: Box<[T]> = (0..size)
            .map(|_| T::default())
            .collect::<Vec<_>>()
            .into_boxed_slice();

        let ptr = buffer.as_ptr() as *mut T;

        Ok(Self {
            buffer: ptr,
            size,
            mask: size - 1,
            producer_cursor: Arc::new(AtomicU64::new(0)),
            consumer_cursor: Arc::new(AtomicU64::new(0)),
            _heap: Some(buffer),
            is_mapped: false,
        })
    }

    /// Create with memory-mapped allocation (mmap + mlock)
    pub fn new_mapped(size: usize) -> Result<Self> {
        if !size.is_power_of_two() {
            return Err(FluxError::config("Size must be power of 2"));
        }

        let buffer_size = size * std::mem::size_of::<T>();

        let ptr = unsafe {
            let p = libc::mmap(
                ptr::null_mut(),
                buffer_size,
                libc::PROT_READ | libc::PROT_WRITE,
                libc::MAP_PRIVATE | libc::MAP_ANONYMOUS,
                -1,
                0
            );

            if p == libc::MAP_FAILED {
                return Err(FluxError::config("mmap failed"));
            }

            // Lock memory to prevent swapping
            let _ = libc::mlock(p, buffer_size);

            // Zero-initialize
            std::ptr::write_bytes(p as *mut u8, 0, buffer_size);

            p as *mut T
        };

        Ok(Self {
            buffer: ptr,
            size,
            mask: size - 1,
            producer_cursor: Arc::new(AtomicU64::new(0)),
            consumer_cursor: Arc::new(AtomicU64::new(0)),
            _heap: None,
            is_mapped: true,
        })
    }

    #[inline(always)]
    pub fn size(&self) -> usize {
        self.size
    }

    #[inline(always)]
    pub fn mask(&self) -> usize {
        self.mask
    }

    #[inline(always)]
    pub fn consumer_cursor(&self) -> Arc<AtomicU64> {
        self.consumer_cursor.clone()
    }

    #[inline(always)]
    pub fn producer_cursor(&self) -> Arc<AtomicU64> {
        self.producer_cursor.clone()
    }

    /// Try to claim slots for writing (simple API)
    #[inline(always)]
    pub fn try_claim(&mut self, count: usize, local_cursor: u64) -> Option<u64> {
        let next = local_cursor + (count as u64);
        let consumer_seq = self.consumer_cursor.load(Ordering::Relaxed);

        // Use wrapping arithmetic to avoid overflow when next > consumer_seq + size
        let pending = next.wrapping_sub(consumer_seq);
        if pending < (self.size as u64) {
            Some(next)
        } else {
            None
        }
    }

    /// Try to claim slots and get mutable slice for batch writes (fast API)
    ///
    /// Returns (start_sequence, mutable_slice) for direct memory access.
    /// This is the fastest way to write multiple slots.
    #[inline(always)]
    pub fn try_claim_slots(&self, count: usize, local_cursor: u64) -> Option<(u64, &mut [T])> {
        if count == 0 {
            return None;
        }

        let next = local_cursor + (count as u64);
        let consumer_seq = self.consumer_cursor.load(Ordering::Relaxed);

        // Use wrapping arithmetic to avoid overflow when next > consumer_seq + size
        let pending = next.wrapping_sub(consumer_seq);
        if pending < (self.size as u64) {
            // Prefetch for better cache performance
            self.prefetch_write(local_cursor as usize, count);

            let start_idx = (local_cursor as usize) & self.mask;
            let end_idx = start_idx + count;

            // Handle wrap-around: only return contiguous portion
            let slots = if end_idx <= self.size {
                unsafe { std::slice::from_raw_parts_mut(self.buffer.add(start_idx), count) }
            } else {
                let first_part = self.size - start_idx;
                unsafe { std::slice::from_raw_parts_mut(self.buffer.add(start_idx), first_part) }
            };

            Some((local_cursor, slots))
        } else {
            None
        }
    }

    /// Get read batch as slice (fast API)
    ///
    /// Returns slice of slots for reading. This is the fastest way to read multiple slots.
    #[inline(always)]
    pub fn get_read_batch(&self, cursor: u64, count: usize) -> &[T] {
        let start_idx = (cursor as usize) & self.mask;
        let end_idx = start_idx + count;

        // Prefetch for better cache performance
        self.prefetch_read(cursor as usize, count);

        // Handle wrap-around: only return contiguous portion
        if end_idx <= self.size {
            unsafe { std::slice::from_raw_parts(self.buffer.add(start_idx), count) }
        } else {
            let first_part = self.size - start_idx;
            unsafe { std::slice::from_raw_parts(self.buffer.add(start_idx), first_part) }
        }
    }

    /// Prefetch slots for writing
    #[inline(always)]
    fn prefetch_write(&self, start_index: usize, count: usize) {
        let prefetch_count = count.min(CACHE_PREFETCH_LINES * 8);
        for i in 0..prefetch_count {
            let slot_index = (start_index + i) & self.mask;
            let slot_ptr = unsafe { self.buffer.add(slot_index) };

            #[cfg(target_arch = "aarch64")]
            unsafe {
                std::arch::asm!(
                    "prfm pstl1keep, [{ptr}]",
                    ptr = in(reg) slot_ptr,
                    options(nostack)
                );
            }

            #[cfg(target_arch = "x86_64")]
            unsafe {
                std::arch::x86_64::_mm_prefetch(
                    slot_ptr as *const i8,
                    std::arch::x86_64::_MM_HINT_T0
                );
            }
        }
    }

    /// Prefetch slots for reading
    #[inline(always)]
    fn prefetch_read(&self, start_index: usize, count: usize) {
        let prefetch_count = count.min(CACHE_PREFETCH_LINES * 8);
        for i in 0..prefetch_count {
            let slot_index = (start_index + i) & self.mask;
            let slot_ptr = unsafe { self.buffer.add(slot_index) };

            #[cfg(target_arch = "aarch64")]
            unsafe {
                std::arch::asm!(
                    "prfm pldl1keep, [{ptr}]",
                    ptr = in(reg) slot_ptr,
                    options(nostack)
                );
            }

            #[cfg(target_arch = "x86_64")]
            unsafe {
                std::arch::x86_64::_mm_prefetch(
                    slot_ptr as *const i8,
                    std::arch::x86_64::_MM_HINT_T0
                );
            }
        }
    }

    /// Write a value to a slot (simple API)
    #[inline(always)]
    pub unsafe fn write_slot(&self, sequence: u64, value: T) {
        let idx = (sequence as usize) & self.mask;
        let slot_ptr = self.buffer.add(idx);
        std::ptr::write_volatile(slot_ptr, value);
    }

    /// Publish slots
    #[inline(always)]
    pub fn publish(&self, sequence: u64) {
        std::sync::atomic::fence(Ordering::Release);
        self.producer_cursor.store(sequence, Ordering::Relaxed);
    }

    /// Read a value from a slot (simple API)
    #[inline(always)]
    pub unsafe fn read_slot(&self, sequence: u64) -> T {
        let idx = (sequence as usize) & self.mask;
        let slot_ptr = self.buffer.add(idx);
        std::ptr::read_volatile(slot_ptr)
    }

    /// Update consumer cursor
    #[inline(always)]
    pub fn update_consumer(&self, sequence: u64) {
        self.consumer_cursor.store(sequence, Ordering::Relaxed);
    }
}

impl<T: RingBufferEntry> Drop for RingBuffer<T> {
    fn drop(&mut self) {
        if self.is_mapped && !self.buffer.is_null() {
            let buffer_size = self.size * std::mem::size_of::<T>();
            unsafe {
                libc::munmap(self.buffer as *mut libc::c_void, buffer_size);
            }
        }
        // Heap storage (_heap) is automatically dropped
    }
}

unsafe impl<T: RingBufferEntry> Send for RingBuffer<T> {}
unsafe impl<T: RingBufferEntry> Sync for RingBuffer<T> {}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::disruptor::SmallSlot;

    #[test]
    fn test_heap_allocation() {
        let ring = RingBuffer::<SmallSlot>::new(1024).unwrap();
        assert_eq!(ring.size(), 1024);
        assert_eq!(ring.mask(), 1023);
    }

    #[test]
    fn test_mapped_allocation() {
        let ring = RingBuffer::<SmallSlot>::new_mapped(1024).unwrap();
        assert_eq!(ring.size(), 1024);
        assert_eq!(ring.mask(), 1023);
    }

    #[test]
    fn test_write_read() {
        let mut ring = RingBuffer::<SmallSlot>::new(1024).unwrap();

        let cursor = 0u64;
        if let Some(next) = ring.try_claim(3, cursor) {
            unsafe {
                ring.write_slot(0, SmallSlot { value: 1 });
                ring.write_slot(1, SmallSlot { value: 2 });
                ring.write_slot(2, SmallSlot { value: 3 });
            }
            ring.publish(next);

            std::sync::atomic::fence(Ordering::Acquire);
            unsafe {
                assert_eq!(ring.read_slot(0).value, 1);
                assert_eq!(ring.read_slot(1).value, 2);
                assert_eq!(ring.read_slot(2).value, 3);
            }
        }
    }

    #[test]
    fn test_mapped_write_read() {
        let mut ring = RingBuffer::<SmallSlot>::new_mapped(1024).unwrap();

        let cursor = 0u64;
        if let Some(next) = ring.try_claim(2, cursor) {
            unsafe {
                ring.write_slot(0, SmallSlot { value: 42 });
                ring.write_slot(1, SmallSlot { value: 43 });
            }
            ring.publish(next);

            std::sync::atomic::fence(Ordering::Acquire);
            unsafe {
                assert_eq!(ring.read_slot(0).value, 42);
                assert_eq!(ring.read_slot(1).value, 43);
            }
        }
    }

    #[test]
    fn test_invalid_size() {
        assert!(RingBuffer::<SmallSlot>::new(1000).is_err());
        assert!(RingBuffer::<SmallSlot>::new_mapped(1000).is_err());
    }
}

//! Batch descriptor for zero-allocation claim/publish pattern
//!
//! This module implements a reusable batch descriptor that eliminates
//! slice allocation overhead on every claim operation.
//!
//! Inspired by LMAX Disruptor's flyweight pattern where `ringBuffer.get(seq)`
//! returns the same object reference without allocation.

use crate::disruptor::MessageSlot;

/// Reusable batch descriptor for claimed slots
///
/// Instead of creating a new slice view on every claim, we return this
/// lightweight descriptor that provides direct indexed access.
///
/// # Performance
///
/// - Zero allocation (unlike `&mut [MessageSlot]`)
/// - Inline index access (better than slice bounds checking)
/// - Cache-friendly (all data in one struct)
pub struct ClaimBatch {
    /// Starting sequence number for this batch
    start_seq: u64,

    /// Number of slots claimed
    count: usize,

    /// Raw pointer to FIRST slot of THIS batch (pre-calculated!)
    /// SAFETY: Valid for lifetime of RingBuffer, index < count
    /// This is buffer_ptr + (start_seq & mask) - CACHED!
    batch_base_ptr: *mut MessageSlot,

    /// Ring buffer mask for wraparound (kept for future bounds checking)
    #[allow(dead_code)]
    mask: usize,
}

impl ClaimBatch {
    /// Create a new batch descriptor
    ///
    /// # Safety
    ///
    /// - `buffer_ptr` must point to valid MessageSlot array
    /// - Array must have at least `mask + 1` elements
    /// - Caller ensures exclusive access to slots [start_seq..start_seq+count]
    #[inline(always)]
    pub unsafe fn new(
        start_seq: u64,
        count: usize,
        buffer_ptr: *mut MessageSlot,
        mask: usize
    ) -> Self {
        // Pre-calculate base pointer for this batch!
        // This makes get_unchecked_mut() as fast as slice access!
        let start_index = (start_seq as usize) & mask;
        let batch_base_ptr = buffer_ptr.add(start_index);

        Self {
            start_seq,
            count,
            batch_base_ptr,
            mask,
        }
    }

    /// Get starting sequence number
    #[inline(always)]
    pub fn start_seq(&self) -> u64 {
        self.start_seq
    }

    /// Get number of claimed slots
    #[inline(always)]
    pub fn count(&self) -> usize {
        self.count
    }

    /// Get mutable reference to slot at index
    ///
    /// # Safety
    ///
    /// Caller must ensure `index < self.count()`
    ///
    /// # Performance
    ///
    /// This is faster than slice indexing because:
    /// - No slice bounds check (we do our own debug assertion)
    /// - Direct pointer arithmetic
    /// - Better inlining
    #[inline(always)]
    pub unsafe fn get_unchecked_mut(&mut self, index: usize) -> &mut MessageSlot {
        debug_assert!(index < self.count, "index out of bounds");
        // NOW as fast as slice! Just base_ptr + index!
        &mut *self.batch_base_ptr.add(index)
    }

    /// Get mutable reference to slot at index (with bounds check)
    ///
    /// Use `get_unchecked_mut()` in hot loops for maximum performance.
    #[inline(always)]
    pub fn get_mut(&mut self, index: usize) -> Option<&mut MessageSlot> {
        if index < self.count {
            Some(unsafe { &mut *self.batch_base_ptr.add(index) })
        } else {
            None
        }
    }

    /// Prefetch cache line for future access
    /// Prefetch slot at index into L1 cache.
    #[inline(always)]
    pub fn prefetch(&self, index: usize) {
        if index < self.count {
            unsafe {
                let ptr = self.batch_base_ptr.add(index) as *const i8;

                #[cfg(target_arch = "aarch64")]
                {
                    std::arch::asm!("prfm pldl1keep, [{0}]", in(reg) ptr);
                }

                #[cfg(target_arch = "x86_64")]
                {
                    std::arch::x86_64::_mm_prefetch(ptr, std::arch::x86_64::_MM_HINT_T0);
                }
            }
        }
    }
}

// SAFETY: ClaimBatch contains a raw pointer to ring buffer slots.
// - Send: Safe because the underlying buffer is either heap-allocated with stable
//   address or mmap'd memory. The ClaimBatch has exclusive write access to its
//   claimed slots during its lifetime.
// - Sync: Safe because ClaimBatch provides mutable access via &mut self methods,
//   so Rust's borrow checker ensures no concurrent access.
unsafe impl Send for ClaimBatch {}
unsafe impl Sync for ClaimBatch {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_claim_batch_access() {
        // Create a small ring buffer
        let mut buffer = vec![MessageSlot::default(); 8];
        let buffer_ptr = buffer.as_mut_ptr();

        unsafe {
            let mut batch = ClaimBatch::new(10, 4, buffer_ptr, 7);

            assert_eq!(batch.start_seq(), 10);
            assert_eq!(batch.count(), 4);

            // Test indexed access
            let start_seq = batch.start_seq();
            let count = batch.count();
            for i in 0..count {
                let slot = batch.get_unchecked_mut(i);
                slot.sequence = start_seq + (i as u64);
            }

            // Verify writes
            for i in 0..4 {
                let ring_index = (10 + i) & 7;
                assert_eq!(buffer[ring_index].sequence, 10 + (i as u64));
            }
        }
    }
}

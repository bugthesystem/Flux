//! MPMC (Multi-Producer Multi-Consumer) Ring Buffer
//!
//! Lock-free ring buffer with multiple producers and consumers.
//! Uses CAS for producer coordination and read-then-commit for consumers.
//! - Release/Acquire fences for synchronization
//! - This is the full LMAX Disruptor pattern

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use crate::disruptor::completion_tracker::{CompletionTracker, ReadGuard, BatchReadGuard, ReadableRing};
use crate::disruptor::RingBufferEntry;
use crate::error::Result;

pub struct MpmcRingBuffer<T: RingBufferEntry> {
    buffer: Box<[T]>,
    mask: usize,
    producer_cursor: Arc<AtomicU64>,
    completion_tracker: CompletionTracker,
}

impl<T: RingBufferEntry> MpmcRingBuffer<T> {
    pub fn new(size: usize) -> Result<Self> {
        if !size.is_power_of_two() {
            return Err(crate::error::KaosError::config("Size must be power of 2"));
        }

        let buffer = (0..size)
            .map(|_| T::default())
            .collect::<Vec<_>>()
            .into_boxed_slice();

        Ok(Self {
            buffer,
            mask: size - 1,
            producer_cursor: Arc::new(AtomicU64::new(0)),
            completion_tracker: CompletionTracker::new(),
        })
    }


    // PRODUCER API (Multi-producer with CAS)


    /// Try to claim write slots - CAS for multi-producer
    /// 
    /// Returns the START sequence if successful. The producer should write to
    /// slots [sequence, sequence + count) and then call `publish()`.
    pub fn try_claim(&self, count: usize) -> Option<u64> {
        let current = self.producer_cursor.load(Ordering::Relaxed);
        let next = current + count as u64;

        // Check against completed_cursor for accurate backpressure
        let consumer_seq = self.completion_tracker.completed_cursor();

        if !self.check_space(next, consumer_seq) {
            return None;
        }

        match self.producer_cursor.compare_exchange_weak(
            current,
            next,
            Ordering::Relaxed,
            Ordering::Relaxed,
        ) {
            Ok(_) => Some(current),
            Err(_) => None, // Another producer won
        }
    }

    /// Write a value to a slot.
    /// 
    /// # Safety
    /// Caller must ensure the slot has been claimed and not yet published.

    pub unsafe fn write_slot(&self, sequence: u64, value: T) {
        let idx = (sequence as usize) & self.mask;
        let slot_ptr = self.buffer.as_ptr().add(idx) as *mut T;
        std::ptr::write_volatile(slot_ptr, value);
    }

    /// Publish written slots, making them visible to consumers.

    pub fn publish(&self, _sequence: u64) {
        std::sync::atomic::fence(Ordering::Release);
        // Note: For MPMC, the CAS in try_claim already updated producer_cursor
    }


    // CONSUMER API - SAFE (Read-then-commit with RAII guards)


    /// Try to claim and read a single slot.
    /// 
    /// Returns a `ReadGuard` that:
    /// - Provides access to the slot data via `guard.get()`
    /// - Automatically commits the slot when dropped
    /// 
    /// This is the recommended API as it prevents data loss even if the
    /// consumer exits early (e.g., on seeing a sentinel value).

    pub fn try_read(&self) -> Option<ReadGuard<'_, T, Self>> {
        let producer_seq = self.producer_cursor.load(Ordering::Relaxed);
        
        if let Some(sequence) = self.completion_tracker.try_claim(producer_seq) {
            std::sync::atomic::fence(Ordering::Acquire);
            Some(ReadGuard::new(self, sequence))
        } else {
            None
        }
    }

    /// Try to claim and read a batch of slots.
    /// 
    /// Returns a `BatchReadGuard` for efficient batch processing.
    /// All slots are committed when the guard is dropped.

    pub fn try_read_batch(&self, max_count: usize) -> Option<BatchReadGuard<'_, T, Self>> {
        let producer_seq = self.producer_cursor.load(Ordering::Relaxed);
        
        if let Some((start, count)) = self.completion_tracker.try_claim_batch(max_count, producer_seq) {
            std::sync::atomic::fence(Ordering::Acquire);
            Some(BatchReadGuard::new(self, start, count))
        } else {
            None
        }
    }


    // CONSUMER API - LOW LEVEL (For advanced use cases)


    /// Try to claim a slot for reading (low-level API).
    /// 
    /// **WARNING**: If you use this API, you MUST call `complete_read()` after
    /// processing, even if you exit early. Prefer `try_read()` instead.

    pub fn try_claim_read(&self) -> Option<u64> {
        let producer_seq = self.producer_cursor.load(Ordering::Relaxed);
        
        if let Some(sequence) = self.completion_tracker.try_claim(producer_seq) {
            std::sync::atomic::fence(Ordering::Acquire);
            Some(sequence)
        } else {
            None
        }
    }

    /// Read a slot (low-level API).
    /// 
    /// # Safety
    /// The slot must have been claimed and published.

    pub unsafe fn read_slot(&self, sequence: u64) -> T {
        let idx = (sequence as usize) & self.mask;
        let slot_ptr = self.buffer.as_ptr().add(idx);
        std::ptr::read_volatile(slot_ptr)
    }

    /// Mark a slot as completed (low-level API).
    /// 
    /// **MUST** be called after reading a slot claimed via `try_claim_read()`.

    pub fn complete_read(&self, sequence: u64) {
        self.completion_tracker.complete(sequence);
    }


    // UTILITY METHODS

    fn check_space(&self, next: u64, consumer_seq: u64) -> bool {
        let available = self.buffer.len() as u64 - (next - consumer_seq);
        available > 0
    }

    /// Get the producer cursor for external coordination.
    pub fn producer_cursor(&self) -> Arc<AtomicU64> {
        self.producer_cursor.clone()
    }

    /// Get the completed cursor (minimum fully-processed sequence).
    pub fn completed_cursor(&self) -> u64 {
        self.completion_tracker.completed_cursor()
    }
}

// Implement ReadableRing trait for RAII guards
impl<T: RingBufferEntry> ReadableRing<T> for MpmcRingBuffer<T> {

    fn read_slot_ref(&self, sequence: u64) -> &T {
        let idx = (sequence as usize) & self.mask;
        &self.buffer[idx]
    }
    fn complete_read(&self, sequence: u64) {
        self.completion_tracker.complete(sequence);
    }
    fn complete_read_batch(&self, start: u64, count: usize) {
        self.completion_tracker.complete_batch(start, count);
    }
}

unsafe impl<T: RingBufferEntry> Send for MpmcRingBuffer<T> {}
unsafe impl<T: RingBufferEntry> Sync for MpmcRingBuffer<T> {}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::disruptor::Slot8;
    use std::thread;
    use std::sync::atomic::AtomicU64;

    #[test]
    fn test_mpmc_basic_read_commit() {
        let ring = MpmcRingBuffer::<Slot8>::new(1024).unwrap();

        // Producer writes
        let seq = ring.try_claim(1).unwrap();
        unsafe { ring.write_slot(seq, Slot8 { value: 42 }); }
        ring.publish(seq);

        // Consumer reads with guard
        let guard = ring.try_read().unwrap();
        assert_eq!(guard.get().value, 42);
        assert_eq!(ring.completed_cursor(), 0); // Not committed yet
        
        drop(guard);
        assert_eq!(ring.completed_cursor(), 1); // Now committed
    }

    #[test]
    fn test_mpmc_early_exit_no_data_loss() {
        let ring = Arc::new(MpmcRingBuffer::<Slot8>::new(1024).unwrap());

        // Producer writes values 1, 2, 3
        for val in [1, 2, 3] {
            let seq = ring.try_claim(1).unwrap();
            unsafe { ring.write_slot(seq, Slot8 { value: val }); }
            ring.publish(seq);
        }

        // Consumer claims slot 0
        {
            let guard = ring.try_read().unwrap();
            assert_eq!(guard.get().value, 1);
        }

        // Consumer claims slot 1 but "exits early"
        {
            let guard = ring.try_read().unwrap();
            let val = guard.get().value;
            assert_eq!(val, 2);
            // Guard still commits on drop!
        }

        // Slot 2 should still be available
        {
            let guard = ring.try_read().unwrap();
            assert_eq!(guard.get().value, 3);
        }

        assert_eq!(ring.completed_cursor(), 3);
    }

    #[test]
    fn test_mpmc_multi_producer_multi_consumer() {
        let ring = Arc::new(MpmcRingBuffer::<Slot8>::new(1024).unwrap());
        let num_producers = 4;
        let num_consumers = 4;
        let items_per_producer = 250u64;
        let total_items = num_producers as u64 * items_per_producer;

        let total_sent = Arc::new(AtomicU64::new(0));
        let total_sum = Arc::new(AtomicU64::new(0));
        let total_count = Arc::new(AtomicU64::new(0));

        // Producer threads
        let mut producers = vec![];
        for producer_id in 0..num_producers {
            let ring_clone = ring.clone();
            let total_sent_clone = total_sent.clone();
            producers.push(thread::spawn(move || {
                let start = producer_id as u64 * items_per_producer + 1;
                let end = start + items_per_producer;
                let mut sent = 0u64;
                
                for val in start..end {
                    loop {
                        if let Some(seq) = ring_clone.try_claim(1) {
                            unsafe { ring_clone.write_slot(seq, Slot8 { value: val }); }
                            ring_clone.publish(seq);
                            sent += 1;
                            break;
                        }
                        std::hint::spin_loop();
                    }
                }
                total_sent_clone.fetch_add(sent, Ordering::Relaxed);
            }));
        }

        // Wait for producers to finish
        for p in producers {
            p.join().unwrap();
        }

        // Send sentinels
        for _ in 0..num_consumers {
            loop {
                if let Some(seq) = ring.try_claim(1) {
                    unsafe { ring.write_slot(seq, Slot8 { value: 0 }); }
                    ring.publish(seq);
                    break;
                }
                std::hint::spin_loop();
            }
        }

        // Consumer threads
        let mut consumers = vec![];
        for _ in 0..num_consumers {
            let ring_clone = ring.clone();
            let total_sum_clone = total_sum.clone();
            let total_count_clone = total_count.clone();
            consumers.push(thread::spawn(move || {
                let mut sum = 0u64;
                let mut count = 0u64;
                
                loop {
                    if let Some(guard) = ring_clone.try_read() {
                        let value = guard.get().value;
                        if value == 0 {
                            break; // Sentinel - guard commits!
                        }
                        sum += value;
                        count += 1;
                    } else {
                        std::hint::spin_loop();
                    }
                }
                
                total_sum_clone.fetch_add(sum, Ordering::Relaxed);
                total_count_clone.fetch_add(count, Ordering::Relaxed);
            }));
        }

        for c in consumers {
            c.join().unwrap();
        }

        let sum = total_sum.load(Ordering::Relaxed);
        let count = total_count.load(Ordering::Relaxed);
        let expected_sum: u64 = (total_items * (total_items + 1)) / 2;

        assert_eq!(count, total_items, "Count mismatch - data loss detected!");
        assert_eq!(sum, expected_sum, "Sum mismatch - data loss detected!");
    }
}

//! SPMC (Single Producer Multi-Consumer) Ring Buffer
//!
//! Lock-free ring buffer with one producer and multiple consumers.
//! Uses read-then-commit pattern with RAII guards for data integrity.
//! - Completion uses Release ordering to ensure visibility
//! - Acquire fence before reading data

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use crate::disruptor::completion_tracker::{CompletionTracker, ReadGuard, BatchReadGuard, ReadableRing};
use crate::disruptor::RingBufferEntry;
use crate::error::Result;

pub struct SpmcRingBuffer<T: RingBufferEntry> {
    buffer: Box<[T]>,
    mask: usize,
    producer_cursor: Arc<AtomicU64>,
    completion_tracker: CompletionTracker,
}

impl<T: RingBufferEntry> SpmcRingBuffer<T> {
    pub fn new(size: usize) -> Result<Self> {
        if !size.is_power_of_two() {
            return Err(crate::error::FluxError::config("Size must be power of 2"));
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

    // ========================================================================
    // PRODUCER API (Single producer, no CAS needed)
    // ========================================================================

    /// Try to claim slots for writing - SPSC style (no CAS)
    /// 
    /// Returns the NEXT cursor position if space is available.
    /// The producer should write to slots [current_cursor, next) and then call `publish(next)`.
    pub fn try_claim(&self, count: usize, current_cursor: u64) -> Option<u64> {
        let next = current_cursor + count as u64;

        // Check against completed_cursor for accurate backpressure
        let consumer_seq = self.completion_tracker.completed_cursor();

        if self.check_space(next, consumer_seq) {
            Some(next)
        } else {
            None
        }
    }

    /// Write a value to a slot.
    /// 
    /// # Safety
    /// Caller must ensure the slot has been claimed and not yet published.
    #[inline(always)]
    pub unsafe fn write_slot(&self, sequence: u64, value: T) {
        let idx = (sequence as usize) & self.mask;
        let slot_ptr = self.buffer.as_ptr().add(idx) as *mut T;
        std::ptr::write_volatile(slot_ptr, value);
    }

    /// Publish written slots, making them visible to consumers.
    #[inline(always)]
    pub fn publish(&self, sequence: u64) {
        std::sync::atomic::fence(Ordering::Release);
        self.producer_cursor.store(sequence, Ordering::Relaxed);
    }

    // ========================================================================
    // CONSUMER API - SAFE (Read-then-commit with RAII guards)
    // ========================================================================

    /// Try to claim and read a single slot.
    /// 
    /// Returns a `ReadGuard` that:
    /// - Provides access to the slot data via `guard.get()`
    /// - Automatically commits the slot when dropped
    /// 
    /// This is the recommended API as it prevents data loss even if the
    /// consumer exits early (e.g., on seeing a sentinel value).
    /// 
    /// # Example
    /// 
    #[inline]
    pub fn try_read(&self) -> Option<ReadGuard<'_, T, Self>> {
        let producer_seq = self.producer_cursor.load(Ordering::Relaxed);
        
        if let Some(sequence) = self.completion_tracker.try_claim(producer_seq) {
            // Acquire fence to ensure we see the producer's writes
            std::sync::atomic::fence(Ordering::Acquire);
            Some(ReadGuard::new(self, sequence))
        } else {
            None
        }
    }

    /// Try to claim and read a batch of slots.
    /// Returns a `BatchReadGuard`; all slots are committed when dropped.
    #[inline]
    pub fn try_read_batch(&self, max_count: usize) -> Option<BatchReadGuard<'_, T, Self>> {
        let producer_seq = self.producer_cursor.load(Ordering::Relaxed);
        
        if let Some((start, count)) = self.completion_tracker.try_claim_batch(max_count, producer_seq) {
            std::sync::atomic::fence(Ordering::Acquire);
            Some(BatchReadGuard::new(self, start, count))
        } else {
            None
        }
    }

    // ========================================================================
    // CONSUMER API - LOW LEVEL (For advanced use cases)
    // ========================================================================

    /// Try to claim a slot for reading (low-level API).
    /// 
    /// **WARNING**: If you use this API, you MUST call `complete_read()` after
    /// processing, even if you exit early. Prefer `try_read()` instead.
    /// 
    /// Returns the sequence number to read from if successful.
    #[inline]
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
    /// The slot must have been claimed via `try_claim_read()` and the producer
    /// must have published data to this slot.
    #[inline(always)]
    pub unsafe fn read_slot(&self, sequence: u64) -> T {
        let idx = (sequence as usize) & self.mask;
        let slot_ptr = self.buffer.as_ptr().add(idx);
        std::ptr::read_volatile(slot_ptr)
    }

    /// Mark a slot as completed (low-level API).
    /// 
    /// **MUST** be called after reading a slot claimed via `try_claim_read()`.
    /// This allows the completed_cursor to advance for producer backpressure.
    #[inline]
    pub fn complete_read(&self, sequence: u64) {
        self.completion_tracker.complete(sequence);
    }

    // ========================================================================
    // FAST BATCH API (300M+/s - works for any number of consumers!)
    // ========================================================================
    
    /// Claim a batch of slots for reading (FAST!)
    /// 
    /// Returns (start_sequence, slice) if slots are available.
    /// After processing, call `complete_batch_fast()`.
    #[inline(always)]
    pub fn claim_batch_fast(&self, max_count: usize) -> Option<(u64, &[T])> {
        let producer_seq = self.producer_cursor.load(Ordering::Acquire);
        
        if let Some((start, count)) = self.completion_tracker.try_claim_batch(max_count, producer_seq) {
            let start_idx = (start as usize) & self.mask;
            let end_idx = start_idx + count;
            
            // Handle wraparound
            let slice = if end_idx <= self.buffer.len() {
                &self.buffer[start_idx..end_idx]
            } else {
                // Don't wrap - just return what's available before wrap
                let available_to_end = self.buffer.len() - start_idx;
                &self.buffer[start_idx..start_idx + available_to_end]
            };
            
            Some((start, slice))
        } else {
            None
        }
    }

    /// Complete a batch of slots (FAST!)
    #[inline(always)]
    pub fn complete_batch_fast(&self, start: u64, count: usize) {
        self.completion_tracker.complete_batch(start, count);
    }

    // ========================================================================
    // SINGLE-CONSUMER FAST PATH (bypasses completion tracker entirely)
    // ========================================================================

    /// Get a batch of slots for reading (single consumer only!)
    #[inline(always)]
    pub fn get_read_batch_fast(&self, consumer_cursor: u64, max_count: usize) -> &[T] {
        let producer_seq = self.producer_cursor.load(Ordering::Acquire);
        let available = producer_seq.saturating_sub(consumer_cursor) as usize;
        
        if available == 0 {
            return &[];
        }
        
        let count = available.min(max_count);
        let start_idx = (consumer_cursor as usize) & self.mask;
        let end_idx = start_idx + count;
        
        if end_idx <= self.buffer.len() {
            &self.buffer[start_idx..end_idx]
        } else {
            let available_to_end = self.buffer.len() - start_idx;
            &self.buffer[start_idx..start_idx + available_to_end]
        }
    }

    /// Update consumer cursor (single consumer only!)
    #[inline(always)]
    pub fn update_consumer_fast(&self, cursor: u64) {
        self.completion_tracker.set_completed_cursor(cursor);
    }

    // ========================================================================
    // UTILITY METHODS
    // ========================================================================

    #[inline(always)]
    fn check_space(&self, next: u64, consumer_seq: u64) -> bool {
        let available = self.buffer.len() as u64 - (next - consumer_seq);
        available > 0
    }

    /// Get the producer cursor for external coordination.
    pub fn producer_cursor(&self) -> Arc<AtomicU64> {
        self.producer_cursor.clone()
    }

    /// Get the completed cursor (minimum fully-processed sequence).
    /// 
    /// This is what the producer uses for backpressure.
    pub fn completed_cursor(&self) -> u64 {
        self.completion_tracker.completed_cursor()
    }

    /// Get the claim cursor (next slot to be claimed by consumers).
    /// 
    /// Useful for debugging/metrics.
    pub fn claim_cursor(&self) -> u64 {
        self.completion_tracker.claim_cursor()
    }

    /// Get the number of in-flight read operations.
    pub fn in_flight_count(&self) -> u64 {
        self.completion_tracker.in_flight_count()
    }
}

// Implement ReadableRing trait for RAII guards
impl<T: RingBufferEntry> ReadableRing<T> for SpmcRingBuffer<T> {
    #[inline]
    fn read_slot_ref(&self, sequence: u64) -> &T {
        let idx = (sequence as usize) & self.mask;
        &self.buffer[idx]
    }

    #[inline]
    fn complete_read(&self, sequence: u64) {
        self.completion_tracker.complete(sequence);
    }

    #[inline]
    fn complete_read_batch(&self, start: u64, count: usize) {
        self.completion_tracker.complete_batch(start, count);
    }
}

unsafe impl<T: RingBufferEntry> Send for SpmcRingBuffer<T> {}
unsafe impl<T: RingBufferEntry> Sync for SpmcRingBuffer<T> {}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::disruptor::SmallSlot;
    use std::thread;

    #[test]
    fn test_spmc_basic_read_commit() {
        let ring = SpmcRingBuffer::<SmallSlot>::new(1024).unwrap();

        // Producer writes
        let next = ring.try_claim(1, 0).unwrap();
        unsafe { ring.write_slot(0, SmallSlot { value: 42 }); }
        ring.publish(next);

        // Consumer reads with guard
        let guard = ring.try_read().unwrap();
        assert_eq!(guard.get().value, 42);
        assert_eq!(ring.completed_cursor(), 0); // Not committed yet
        
        drop(guard);
        assert_eq!(ring.completed_cursor(), 1); // Now committed
    }

    #[test]
    fn test_spmc_early_exit_no_data_loss() {
        let ring = Arc::new(SpmcRingBuffer::<SmallSlot>::new(1024).unwrap());

        // Producer writes values 1, 2, 3
        let mut cursor = 0u64;
        for val in [1, 2, 3] {
            let next = ring.try_claim(1, cursor).unwrap();
            unsafe { ring.write_slot(cursor, SmallSlot { value: val }); }
            ring.publish(next);
            cursor = next;
        }

        // Consumer claims slot 0
        {
            let guard = ring.try_read().unwrap();
            assert_eq!(guard.get().value, 1);
            // Guard drops, slot 0 committed
        }

        // Consumer claims slot 1 but "exits early"
        {
            let guard = ring.try_read().unwrap();
            let val = guard.get().value;
            assert_eq!(val, 2);
            // Simulate early exit - guard still commits!
        }

        // Slot 2 should still be available
        {
            let guard = ring.try_read().unwrap();
            assert_eq!(guard.get().value, 3);
        }

        assert_eq!(ring.completed_cursor(), 3);
    }

    #[test]
    fn test_spmc_batch_read() {
        let ring = SpmcRingBuffer::<SmallSlot>::new(1024).unwrap();

        // Producer writes batch
        let mut cursor = 0u64;
        for i in 0..10 {
            let next = ring.try_claim(1, cursor).unwrap();
            unsafe { ring.write_slot(cursor, SmallSlot { value: i + 1 }); }
            ring.publish(next);
            cursor = next;
        }

        // Consumer reads batch
        let batch = ring.try_read_batch(5).unwrap();
        assert_eq!(batch.count(), 5);
        
        let values: Vec<u64> = batch.iter().map(|s| s.value).collect();
        assert_eq!(values, vec![1, 2, 3, 4, 5]);
        
        drop(batch);
        assert_eq!(ring.completed_cursor(), 5);
    }

    #[test]
    fn test_spmc_out_of_order_completion() {
        let ring = SpmcRingBuffer::<SmallSlot>::new(1024).unwrap();

        // Producer writes 3 values
        let mut cursor = 0u64;
        for i in 0..3 {
            let next = ring.try_claim(1, cursor).unwrap();
            unsafe { ring.write_slot(cursor, SmallSlot { value: i }); }
            ring.publish(next);
            cursor = next;
        }

        // Claim 3 slots
        let seq0 = ring.try_claim_read().unwrap();
        let seq1 = ring.try_claim_read().unwrap();
        let seq2 = ring.try_claim_read().unwrap();

        assert_eq!(seq0, 0);
        assert_eq!(seq1, 1);
        assert_eq!(seq2, 2);

        // Complete out of order
        ring.complete_read(seq1);  // Complete slot 1 first
        assert_eq!(ring.completed_cursor(), 0); // Can't advance yet

        ring.complete_read(seq0);  // Complete slot 0
        assert_eq!(ring.completed_cursor(), 2); // Now advances past 0 and 1

        ring.complete_read(seq2);  // Complete slot 2
        assert_eq!(ring.completed_cursor(), 3);
    }

    #[test]
    fn test_spmc_multi_threaded() {
        let ring = Arc::new(SpmcRingBuffer::<SmallSlot>::new(1024).unwrap());
        let num_items = 1000u64;
        let num_consumers = 4;

        // Producer thread
        let ring_producer = ring.clone();
        let producer = thread::spawn(move || {
            let mut cursor = 0u64;
            for i in 1..=num_items {
                loop {
                    if let Some(next) = ring_producer.try_claim(1, cursor) {
                        unsafe { ring_producer.write_slot(cursor, SmallSlot { value: i }); }
                        ring_producer.publish(next);
                        cursor = next;
                        break;
                    }
                    std::hint::spin_loop();
                }
            }
            // Send sentinels
            for _ in 0..num_consumers {
                loop {
                    if let Some(next) = ring_producer.try_claim(1, cursor) {
                        unsafe { ring_producer.write_slot(cursor, SmallSlot { value: 0 }); }
                        ring_producer.publish(next);
                        cursor = next;
                        break;
                    }
                    std::hint::spin_loop();
                }
            }
        });

        // Consumer threads
        let mut consumers = vec![];
        for _ in 0..num_consumers {
            let ring_consumer = ring.clone();
            consumers.push(thread::spawn(move || {
                let mut sum = 0u64;
                let mut count = 0u64;
                loop {
                    if let Some(guard) = ring_consumer.try_read() {
                        let value = guard.get().value;
                        if value == 0 {
                            break; // Sentinel - guard still commits!
                        }
                        sum += value;
                        count += 1;
                    } else {
                        std::hint::spin_loop();
                    }
                }
                (sum, count)
            }));
        }

        producer.join().unwrap();

        let mut total_sum = 0u64;
        let mut total_count = 0u64;
        for consumer in consumers {
            let (sum, count) = consumer.join().unwrap();
            total_sum += sum;
            total_count += count;
        }

        let expected_sum: u64 = (num_items * (num_items + 1)) / 2;
        assert_eq!(total_sum, expected_sum, "Sum mismatch - data loss detected!");
        assert_eq!(total_count, num_items, "Count mismatch - data loss detected!");
    }
}

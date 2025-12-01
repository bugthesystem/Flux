//! Completion tracking for multi-consumer ring buffers.
//!
//! Provides read-then-commit pattern for SPMC/MPMC consumers.

use std::sync::atomic::{ AtomicU64, AtomicBool, Ordering };
use std::marker::PhantomData;

use super::RingBufferEntry;

/// Maximum in-flight slots (must be power of 2)
const MAX_SLOTS: usize = 65536;
const SLOT_MASK: usize = MAX_SLOTS - 1;

/// Cache-line padded atomic for preventing false sharing
#[repr(align(128))]
struct PaddedAtomicU64(AtomicU64);

impl PaddedAtomicU64 {
    fn new(v: u64) -> Self {
        Self(AtomicU64::new(v))
    }
}

/// Fast completion tracker using per-slot completion flags.
///
/// No seqlocks, no spin-waits - just atomic operations.
pub struct CompletionTracker {
    /// Next slot to claim
    claim_cursor: PaddedAtomicU64,

    /// Minimum completed sequence (for producer backpressure)
    completed_cursor: PaddedAtomicU64,

    /// Per-slot completion flags
    /// slot_completed[seq & SLOT_MASK] = true if slot is completed
    slot_completed: Box<[AtomicBool; MAX_SLOTS]>,
}

impl CompletionTracker {
    pub fn new() -> Self {
        // Initialize slot completion array
        let slot_completed: Box<[AtomicBool; MAX_SLOTS]> = {
            let mut v = Vec::with_capacity(MAX_SLOTS);
            for _ in 0..MAX_SLOTS {
                v.push(AtomicBool::new(false));
            }
            v.into_boxed_slice().try_into().unwrap()
        };

        Self {
            claim_cursor: PaddedAtomicU64::new(0),
            completed_cursor: PaddedAtomicU64::new(0),
            slot_completed,
        }
    }

    /// Try to claim the next slot for reading.
    pub fn try_claim(&self, producer_cursor: u64) -> Option<u64> {
        loop {
            let current = self.claim_cursor.0.load(Ordering::Relaxed);
            if current >= producer_cursor {
                return None;
            }

            // CAS to claim
            if
                self.claim_cursor.0
                    .compare_exchange_weak(
                        current,
                        current + 1,
                        Ordering::Relaxed,
                        Ordering::Relaxed
                    )
                    .is_ok()
            {
                return Some(current);
            }
        }
    }

    /// Try to claim a batch of slots.
    pub fn try_claim_batch(&self, requested: usize, producer_cursor: u64) -> Option<(u64, usize)> {
        loop {
            let current = self.claim_cursor.0.load(Ordering::Relaxed);
            let available = producer_cursor.saturating_sub(current) as usize;

            if available == 0 {
                return None;
            }

            let count = requested.min(available);
            let next = current + (count as u64);

            if
                self.claim_cursor.0
                    .compare_exchange_weak(current, next, Ordering::Relaxed, Ordering::Relaxed)
                    .is_ok()
            {
                return Some((current, count));
            }
        }
    }

    /// Mark a slot as completed.
    pub fn complete(&self, sequence: u64) {
        // Mark this slot as completed
        let idx = (sequence as usize) & SLOT_MASK;
        self.slot_completed[idx].store(true, Ordering::Release);

        // Try to advance completed_cursor
        self.try_advance_completed();
    }

    /// Mark a batch of slots as completed.
    pub fn complete_batch(&self, start: u64, count: usize) {
        // Mark all slots as completed
        for i in 0..count {
            let idx = ((start + (i as u64)) as usize) & SLOT_MASK;
            self.slot_completed[idx].store(true, Ordering::Release);
        }

        // Try to advance completed_cursor
        self.try_advance_completed();
    }

    /// Try to advance completed_cursor as far as possible.
    fn try_advance_completed(&self) {
        loop {
            let completed = self.completed_cursor.0.load(Ordering::Relaxed);
            let claimed = self.claim_cursor.0.load(Ordering::Relaxed);

            // Check if next slot is completed
            if completed >= claimed {
                return;
            }

            let idx = (completed as usize) & SLOT_MASK;
            if !self.slot_completed[idx].load(Ordering::Acquire) {
                return; // Not completed yet
            }

            // Try to advance
            if
                self.completed_cursor.0
                    .compare_exchange_weak(
                        completed,
                        completed + 1,
                        Ordering::Release,
                        Ordering::Relaxed
                    )
                    .is_ok()
            {
                // Clear the flag for reuse
                self.slot_completed[idx].store(false, Ordering::Relaxed);
                // Continue trying to advance
            }
            // If CAS fails, another thread advanced - retry
        }
    }

    /// Get completed cursor for producer backpressure.
    pub fn completed_cursor(&self) -> u64 {
        self.completed_cursor.0.load(Ordering::Acquire)
    }

    /// Set completed cursor directly (for single-consumer fast path).
    pub fn set_completed_cursor(&self, cursor: u64) {
        self.completed_cursor.0.store(cursor, Ordering::Release);
        self.claim_cursor.0.store(cursor, Ordering::Relaxed);
    }
}

// RAII Guards for safe read-then-commit pattern

/// Trait for ring buffers that support the read-then-commit pattern.
pub trait ReadableRing<T: RingBufferEntry> {
    fn read_slot_ref(&self, sequence: u64) -> &T;
    fn complete_read(&self, sequence: u64);
    fn complete_read_batch(&self, start: u64, count: usize);
}

/// RAII guard for single-slot reads.
pub struct ReadGuard<'a, T: RingBufferEntry, R: ReadableRing<T>> {
    ring: &'a R,
    sequence: u64,
    _marker: PhantomData<T>,
}

impl<'a, T: RingBufferEntry, R: ReadableRing<T>> ReadGuard<'a, T, R> {
    pub fn new(ring: &'a R, sequence: u64) -> Self {
        Self {
            ring,
            sequence,
            _marker: PhantomData,
        }
    }
    pub fn get(&self) -> &T {
        self.ring.read_slot_ref(self.sequence)
    }
}

impl<'a, T: RingBufferEntry, R: ReadableRing<T>> Drop for ReadGuard<'a, T, R> {
    fn drop(&mut self) {
        self.ring.complete_read(self.sequence);
    }
}

/// RAII guard for batch reads.
pub struct BatchReadGuard<'a, T: RingBufferEntry, R: ReadableRing<T>> {
    ring: &'a R,
    start_sequence: u64,
    count: usize,
    _marker: PhantomData<T>,
}

impl<'a, T: RingBufferEntry, R: ReadableRing<T>> BatchReadGuard<'a, T, R> {
    pub fn new(ring: &'a R, start: u64, count: usize) -> Self {
        Self {
            ring,
            start_sequence: start,
            count,
            _marker: PhantomData,
        }
    }
    pub fn count(&self) -> usize {
        self.count
    }

    pub fn iter(&self) -> impl Iterator<Item = &T> {
        (0..self.count).map(move |i| { self.ring.read_slot_ref(self.start_sequence + (i as u64)) })
    }
}

impl<'a, T: RingBufferEntry, R: ReadableRing<T>> Drop for BatchReadGuard<'a, T, R> {
    fn drop(&mut self) {
        self.ring.complete_read_batch(self.start_sequence, self.count);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_completion_tracker_basic() {
        let tracker = CompletionTracker::new();

        // Claim some slots
        assert_eq!(tracker.try_claim(10), Some(0));
        assert_eq!(tracker.try_claim(10), Some(1));
        assert_eq!(tracker.try_claim(10), Some(2));

        // Complete in order
        tracker.complete(0);
        assert_eq!(tracker.completed_cursor(), 1);

        tracker.complete(1);
        assert_eq!(tracker.completed_cursor(), 2);
    }

    #[test]
    fn test_out_of_order_completion() {
        let tracker = CompletionTracker::new();

        // Claim 3 slots
        assert_eq!(tracker.try_claim(10), Some(0));
        assert_eq!(tracker.try_claim(10), Some(1));
        assert_eq!(tracker.try_claim(10), Some(2));

        // Complete out of order
        tracker.complete(2); // Complete last first
        assert_eq!(tracker.completed_cursor(), 0); // Can't advance yet

        tracker.complete(1);
        assert_eq!(tracker.completed_cursor(), 0); // Still waiting for 0

        tracker.complete(0);
        assert_eq!(tracker.completed_cursor(), 3); // All done!
    }

    #[test]
    fn test_completion_tracker_batch() {
        let tracker = CompletionTracker::new();

        // Claim batch
        let (start, count) = tracker.try_claim_batch(5, 10).unwrap();
        assert_eq!(start, 0);
        assert_eq!(count, 5);

        // Complete batch
        tracker.complete_batch(0, 5);
        assert_eq!(tracker.completed_cursor(), 5);
    }

    #[test]
    fn test_fast_path() {
        let tracker = CompletionTracker::new();

        // Use fast path
        tracker.set_completed_cursor(100);
        assert_eq!(tracker.completed_cursor(), 100);
    }
}

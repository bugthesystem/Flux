//! Completion tracking for multi-consumer ring buffers.

use super::RingBufferEntry;
use std::marker::PhantomData;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

/// Maximum slots for completion tracking (64K = 2^16, allows efficient masking)
const MAX_SLOTS: usize = 65536;

/// Bitmask for slot index (MAX_SLOTS - 1 for fast modulo via AND)
const SLOT_MASK: usize = MAX_SLOTS - 1;

/// Cache-line aligned atomic (128 bytes for Apple Silicon, 64 bytes on x86)
#[repr(align(128))]
struct PaddedAtomicU64(AtomicU64);

impl PaddedAtomicU64 {
    fn new(v: u64) -> Self {
        Self(AtomicU64::new(v))
    }
}

pub struct CompletionTracker {
    claim_cursor: PaddedAtomicU64,
    completed_cursor: PaddedAtomicU64,
    slot_completed: Box<[AtomicBool; MAX_SLOTS]>,
}

impl CompletionTracker {
    pub fn new() -> Self {
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

    pub fn try_claim(&self, producer_cursor: u64) -> Option<u64> {
        loop {
            let current = self.claim_cursor.0.load(Ordering::Relaxed);
            if current >= producer_cursor {
                return None;
            }
            if self
                .claim_cursor
                .0
                .compare_exchange_weak(current, current + 1, Ordering::Relaxed, Ordering::Relaxed)
                .is_ok()
            {
                return Some(current);
            }
        }
    }

    pub fn try_claim_batch(&self, requested: usize, producer_cursor: u64) -> Option<(u64, usize)> {
        loop {
            let current = self.claim_cursor.0.load(Ordering::Relaxed);
            let available = producer_cursor.saturating_sub(current) as usize;
            if available == 0 {
                return None;
            }
            let count = requested.min(available);
            let next = current + (count as u64);
            if self
                .claim_cursor
                .0
                .compare_exchange_weak(current, next, Ordering::Relaxed, Ordering::Relaxed)
                .is_ok()
            {
                return Some((current, count));
            }
        }
    }

    pub fn complete(&self, sequence: u64) {
        let idx = (sequence as usize) & SLOT_MASK;
        self.slot_completed[idx].store(true, Ordering::Release);
        self.try_advance_completed();
    }

    pub fn complete_batch(&self, start: u64, count: usize) {
        for i in 0..count {
            let idx = ((start + (i as u64)) as usize) & SLOT_MASK;
            self.slot_completed[idx].store(true, Ordering::Release);
        }
        self.try_advance_completed();
    }

    fn try_advance_completed(&self) {
        loop {
            let completed = self.completed_cursor.0.load(Ordering::Relaxed);
            let claimed = self.claim_cursor.0.load(Ordering::Relaxed);
            if completed >= claimed {
                return;
            }
            let idx = (completed as usize) & SLOT_MASK;
            if !self.slot_completed[idx].load(Ordering::Acquire) {
                return;
            }
            if self
                .completed_cursor
                .0
                .compare_exchange_weak(
                    completed,
                    completed + 1,
                    Ordering::Release,
                    Ordering::Relaxed,
                )
                .is_ok()
            {
                self.slot_completed[idx].store(false, Ordering::Relaxed);
            }
        }
    }

    pub fn completed_cursor(&self) -> u64 {
        self.completed_cursor.0.load(Ordering::Acquire)
    }

    pub fn set_completed_cursor(&self, cursor: u64) {
        self.completed_cursor.0.store(cursor, Ordering::Release);
        self.claim_cursor.0.store(cursor, Ordering::Relaxed);
    }
}

pub trait ReadableRing<T: RingBufferEntry> {
    fn read_slot_ref(&self, sequence: u64) -> &T;
    fn complete_read(&self, sequence: u64);
    fn complete_read_batch(&self, start: u64, count: usize);
}

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
        (0..self.count).map(move |i| self.ring.read_slot_ref(self.start_sequence + (i as u64)))
    }
}

impl<'a, T: RingBufferEntry, R: ReadableRing<T>> Drop for BatchReadGuard<'a, T, R> {
    fn drop(&mut self) {
        self.ring
            .complete_read_batch(self.start_sequence, self.count);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_completion_tracker_basic() {
        let tracker = CompletionTracker::new();
        assert_eq!(tracker.try_claim(10), Some(0));
        assert_eq!(tracker.try_claim(10), Some(1));
        assert_eq!(tracker.try_claim(10), Some(2));
        tracker.complete(0);
        assert_eq!(tracker.completed_cursor(), 1);
        tracker.complete(1);
        assert_eq!(tracker.completed_cursor(), 2);
    }

    #[test]
    fn test_out_of_order_completion() {
        let tracker = CompletionTracker::new();
        tracker.try_claim(10);
        tracker.try_claim(10);
        tracker.try_claim(10);

        tracker.complete(2);
        assert_eq!(tracker.completed_cursor(), 0);
        tracker.complete(1);
        assert_eq!(tracker.completed_cursor(), 0);
        tracker.complete(0);
        assert_eq!(tracker.completed_cursor(), 3);
    }

    #[test]
    fn test_completion_tracker_batch() {
        let tracker = CompletionTracker::new();
        let (start, count) = tracker.try_claim_batch(5, 10).unwrap();
        assert_eq!(start, 0);
        assert_eq!(count, 5);
        tracker.complete_batch(0, 5);
        assert_eq!(tracker.completed_cursor(), 5);
    }

    #[test]
    fn test_fast_path() {
        let tracker = CompletionTracker::new();
        tracker.set_completed_cursor(100);
        assert_eq!(tracker.completed_cursor(), 100);
    }
}

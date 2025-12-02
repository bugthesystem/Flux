//! MPSC (Multi-Producer Single Consumer) Ring Buffer
//!
//! This implements a lock-free ring buffer that supports multiple producers
//! and a single consumer using atomic CAS operations.
//!
//! ## Performance
//!
//!
//! ## Memory Ordering (Sequence Barrier Pattern)
//!
//! - **claim_cursor**: CAS-based claiming, tells producers where to write
//! - **published[]**: Per-slot flags indicating write completion
//! - Consumer reads published[] to know what's safe to read
//! - This ensures proper ordering even with concurrent producers

use std::sync::atomic::{ AtomicU64, Ordering };
use std::sync::Arc;

use crate::disruptor::{ RingBufferEntry, ring_buffer_core::RingBufferCore };
use crate::error::{ Result, KaosError };

pub struct MpscRingBuffer<T: RingBufferEntry> {
    core: RingBufferCore<T>,
    claim_cursor: Arc<AtomicU64>, // For claiming slots (CAS)
    /// Bitfield tracking published slots. Each AtomicU64 tracks 64 slots.
    /// Bits encode even/odd rounds via XOR flipping (LMAX Disruptor technique)
    available: Box<[AtomicU64]>,
    index_mask: usize,
    index_shift: usize,
}

impl<T: RingBufferEntry> MpscRingBuffer<T> {
    pub fn new(size: usize) -> Result<Self> {
        if size < 64 {
            return Err(KaosError::config("MPSC ring buffer must be at least 64 slots"));
        }

        let core = RingBufferCore::new(size)?;
        let u64_needed = size / 64;
        // Initialize with all 1s (nothing published yet, round 0 is even)
        let available = (0..u64_needed)
            .map(|_| AtomicU64::new(!0))
            .collect::<Vec<_>>()
            .into_boxed_slice();

        let index_mask = size - 1;
        let index_shift = Self::log2(size);

        Ok(Self {
            core,
            claim_cursor: Arc::new(AtomicU64::new(0)),
            available,
            index_mask,
            index_shift,
        })
    }

    fn log2(i: usize) -> usize {
        std::mem::size_of::<usize>() * 8 - (i.leading_zeros() as usize) - 1
    }

    /// Try to claim slots - MPSC uses CAS for coordination
    pub fn try_claim(&self, count: usize) -> Option<u64> {
        loop {
            let current = self.claim_cursor.load(Ordering::Relaxed);
            let next = current + (count as u64);

            let consumer_seq = self.core.consumer_cursor.load(Ordering::Acquire);

            if !self.core.check_space(next, consumer_seq) {
                return None;
            }

            match
                self.claim_cursor.compare_exchange_weak(
                    current,
                    next,
                    Ordering::Acquire,
                    Ordering::Relaxed
                )
            {
                Ok(_) => {
                    return Some(current);
                }
                Err(_) => std::hint::spin_loop(),
            }
        }
    }
    pub unsafe fn write_slot(&self, sequence: u64, value: T) {
        self.core.write_slot(sequence, value);
    }

    /// Calculate which AtomicU64 and bit index for a sequence

    fn calculate_indices(&self, sequence: u64) -> (usize, usize) {
        let slot_index = (sequence as usize) & self.index_mask;
        let availability_index = slot_index >> 6; // divide by 64
        let bit_index = slot_index & 63; // mod 64
        (availability_index, bit_index)
    }

    /// Calculate even (0) or odd (1) round

    fn calculate_flag(&self, sequence: u64) -> u64 {
        let round = sequence >> self.index_shift;
        round & 1
    }

    /// Publish a sequence - flips the bit to mark as published

    pub fn publish(&self, sequence: u64) {
        let (avail_idx, bit_idx) = self.calculate_indices(sequence);
        let mask = 1u64 << bit_idx;
        // XOR flips the bit - encoding even/odd round publication
        self.available[avail_idx].fetch_xor(mask, Ordering::Release);
    }

    pub unsafe fn read_slot(&self, sequence: u64) -> T {
        self.core.read_slot(sequence)
    }

    pub fn update_consumer(&self, sequence: u64) {
        // No need to clear bits - round tracking handles reuse
        self.core.consumer_cursor.store(sequence, Ordering::Release);
    }

    /// Get highest published sequence - with upper bound to avoid infinite loop
    pub fn get_published_sequence(&self) -> u64 {
        let prev = self.core.consumer_cursor.load(Ordering::Relaxed);
        let claim_seq = self.claim_cursor.load(Ordering::Acquire);

        // Don't scan beyond what's been claimed
        if prev >= claim_seq {
            return prev;
        }

        let mut availability_flag = self.calculate_flag(prev);
        let (mut availability_index, mut bit_index) = self.calculate_indices(prev);
        let mut availability = self.available[availability_index].load(Ordering::Acquire);
        // Shift bits to first relevant bit.
        availability >>= bit_index;
        let mut highest_available = prev;

        loop {
            if (availability & 1) != availability_flag {
                return highest_available.saturating_sub(1);
            }
            highest_available += 1;

            // Stop at claim cursor - can't be published beyond that
            if highest_available >= claim_seq {
                return claim_seq;
            }

            // Prepare for checking the next bit.
            if bit_index < 63 {
                // We shift max 63 places.
                bit_index += 1;
                availability >>= 1;
            } else {
                // Load next bit field.
                (availability_index, _) = self.calculate_indices(highest_available);
                availability = self.available[availability_index].load(Ordering::Acquire);
                bit_index = 0;

                if availability_index == 0 {
                    // If we wrapped then we're now looking for the flipped bit.
                    // (I.e. from odd to even or from even to odd.)
                    availability_flag ^= 1;
                }
            }
        }
    }
}

unsafe impl<T: RingBufferEntry> Send for MpscRingBuffer<T> {}
unsafe impl<T: RingBufferEntry> Sync for MpscRingBuffer<T> {}

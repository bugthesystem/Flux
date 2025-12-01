//! Shared core implementation for all ring buffer variants
//!
//! This module contains the common data structures and basic operations
//! shared by RingBuffer, MpscRingBuffer, SpmcRingBuffer, and MpmcRingBuffer.
//!
//! Each variant specializes the claim/publish logic based on their
//! concurrency pattern (SPSC, MPSC, SPMC, MPMC).

use std::sync::atomic::{ AtomicU64, Ordering };
use std::sync::Arc;

use crate::disruptor::RingBufferEntry;
use crate::error::{ FluxError, Result };

pub struct RingBufferCore<T: RingBufferEntry> {
    pub(crate) buffer: Box<[T]>,
    pub(crate) mask: usize,
    pub(crate) producer_cursor: Arc<AtomicU64>,
    pub(crate) consumer_cursor: Arc<AtomicU64>,
}

impl<T: RingBufferEntry> RingBufferCore<T> {
    pub fn new(size: usize) -> Result<Self> {
        if !size.is_power_of_two() {
            return Err(FluxError::config("Size must be power of 2"));
        }

        let buffer = (0..size)
            .map(|_| T::default())
            .collect::<Vec<_>>()
            .into_boxed_slice();

        Ok(Self {
            buffer,
            mask: size - 1,
            producer_cursor: Arc::new(AtomicU64::new(0)),
            consumer_cursor: Arc::new(AtomicU64::new(0)),
        })
    }

    #[inline(always)]
    pub fn size(&self) -> usize {
        self.buffer.len()
    }

    /// Write a slot at the given sequence.
    ///
    /// # Safety
    /// - Caller must ensure exclusive access to this slot (via claim/publish protocol)
    /// - T must be Copy/POD for volatile write to be sound
    /// - sequence must not overflow the buffer
    #[inline(always)]
    pub unsafe fn write_slot(&self, sequence: u64, value: T) {
        let idx = (sequence as usize) & self.mask;
        let slot_ptr = self.buffer.as_ptr().add(idx) as *mut T;
        std::ptr::write_volatile(slot_ptr, value);
    }

    /// Read a slot at the given sequence.
    ///
    /// # Safety
    /// - Caller must ensure the slot has been published (producer cursor >= sequence)
    /// - T must be Copy/POD for volatile read to be sound
    /// - sequence must not overflow the buffer
    #[inline(always)]
    pub unsafe fn read_slot(&self, sequence: u64) -> T {
        let idx = (sequence as usize) & self.mask;
        let slot_ptr = self.buffer.as_ptr().add(idx);
        std::ptr::read_volatile(slot_ptr)
    }

    #[inline(always)]
    pub fn check_space(&self, next: u64, consumer_seq: u64) -> bool {
        let available = (self.buffer.len() as u64) - (next - consumer_seq);
        available > 0
    }

    pub fn producer_cursor(&self) -> Arc<AtomicU64> {
        self.producer_cursor.clone()
    }

    pub fn consumer_cursor(&self) -> Arc<AtomicU64> {
        self.consumer_cursor.clone()
    }
}

pub trait ClaimStrategy<T: RingBufferEntry> {
    fn try_claim(&self, core: &RingBufferCore<T>, count: usize, current: u64) -> Option<u64>;
}

pub trait PublishStrategy {
    fn publish(&self, _sequence: u64);
}

pub struct SpscClaim;

impl<T: RingBufferEntry> ClaimStrategy<T> for SpscClaim {
    #[inline(always)]
    fn try_claim(&self, core: &RingBufferCore<T>, count: usize, current: u64) -> Option<u64> {
        let next = current + (count as u64);
        let consumer_seq = core.consumer_cursor.load(Ordering::Relaxed);

        if core.check_space(next, consumer_seq) {
            Some(next)
        } else {
            None
        }
    }
}

pub struct MpscClaim;

impl<T: RingBufferEntry> ClaimStrategy<T> for MpscClaim {
    fn try_claim(&self, core: &RingBufferCore<T>, count: usize, _current: u64) -> Option<u64> {
        loop {
            let current = core.producer_cursor.load(Ordering::Relaxed);
            let next = current + (count as u64);
            let consumer_seq = core.consumer_cursor.load(Ordering::Relaxed);

            if !core.check_space(next, consumer_seq) {
                return None;
            }

            match
                core.producer_cursor.compare_exchange_weak(
                    current,
                    next,
                    Ordering::Relaxed,
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
}

pub struct SpscPublish;

impl PublishStrategy for SpscPublish {
    #[inline(always)]
    fn publish(&self, _sequence: u64) {
        std::sync::atomic::fence(Ordering::Release);
        // SPSC: producer_cursor already updated in try_claim
    }
}

pub struct MpPublish<'a> {
    pub producer_cursor: &'a Arc<AtomicU64>,
}

impl<'a> PublishStrategy for MpPublish<'a> {
    #[inline(always)]
    fn publish(&self, _sequence: u64) {
        std::sync::atomic::fence(Ordering::Release);
        // MPSC/MPMC: already updated in CAS
    }
}

unsafe impl<T: RingBufferEntry> Send for RingBufferCore<T> {}
unsafe impl<T: RingBufferEntry> Sync for RingBufferCore<T> {}

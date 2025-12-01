//! Shared core for SPSC/MPSC/SPMC/MPMC ring buffers.

use std::sync::atomic::AtomicU64;
use std::sync::Arc;

use crate::disruptor::RingBufferEntry;
use crate::error::{KaosError, Result};

pub struct RingBufferCore<T: RingBufferEntry> {
    pub(crate) buffer: Box<[T]>,
    pub(crate) mask: usize,
    pub(crate) consumer_cursor: Arc<AtomicU64>,
}

impl<T: RingBufferEntry> RingBufferCore<T> {
    pub fn new(size: usize) -> Result<Self> {
        if !size.is_power_of_two() {
            return Err(KaosError::config("Size must be power of 2"));
        }
        let buffer = (0..size).map(|_| T::default()).collect::<Vec<_>>().into_boxed_slice();
        Ok(Self {
            buffer,
            mask: size - 1,
            consumer_cursor: Arc::new(AtomicU64::new(0)),
        })
    }

    /// # Safety: exclusive access via claim/publish, valid sequence
    pub unsafe fn write_slot(&self, sequence: u64, value: T) {
        let idx = (sequence as usize) & self.mask;
        std::ptr::write_volatile(self.buffer.as_ptr().add(idx) as *mut T, value);
    }

    /// # Safety: slot must be published (producer_cursor >= sequence)
    pub unsafe fn read_slot(&self, sequence: u64) -> T {
        let idx = (sequence as usize) & self.mask;
        std::ptr::read_volatile(self.buffer.as_ptr().add(idx))
    }

    pub fn check_space(&self, next: u64, consumer_seq: u64) -> bool {
        (self.buffer.len() as u64) > (next - consumer_seq)
    }

    pub fn consumer_cursor(&self) -> Arc<AtomicU64> {
        self.consumer_cursor.clone()
    }
}

unsafe impl<T: RingBufferEntry> Send for RingBufferCore<T> {}
unsafe impl<T: RingBufferEntry> Sync for RingBufferCore<T> {}

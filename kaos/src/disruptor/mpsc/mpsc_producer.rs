//! High-level Producer API for MpscRingBuffer (Multi-Producer Safe!)
//!
//! Provides clean API for MPSC pattern with thread-safe producers

use std::sync::Arc;
use std::marker::PhantomData;
use crate::disruptor::{RingBufferEntry, MpscRingBuffer};

/// Thread-safe producer for MPSC ring buffer
pub struct MpscProducer<T: RingBufferEntry> {
    ring_buffer: Arc<MpscRingBuffer<T>>,
    _phantom: PhantomData<T>,
}

unsafe impl<T: RingBufferEntry> Send for MpscProducer<T> {}

impl<T: RingBufferEntry> MpscProducer<T> {
    pub fn new(ring_buffer: Arc<MpscRingBuffer<T>>) -> Self {
        Self {
            ring_buffer,
            _phantom: PhantomData,
        }
    }

    /// Publish single event with closure - Thread-safe!

    pub fn publish<F>(&self, writer: F) -> Result<(), &'static str>
    where
        F: FnOnce(&mut T),
    {
        if let Some(seq) = self.ring_buffer.try_claim(1) {
            let mut value = T::default();
            writer(&mut value);
            
            unsafe {
                self.ring_buffer.write_slot(seq, value);
            }
            
            self.ring_buffer.publish(seq);
            Ok(())
        } else {
            Err("Ring buffer full")
        }
    }

    /// Publish batch

    pub fn publish_batch<F>(&self, count: usize, mut writer: F) -> Result<usize, &'static str>
    where
        F: FnMut(usize, &mut T),
    {
        if let Some(start_seq) = self.ring_buffer.try_claim(count) {
            for i in 0..count {
                let seq = start_seq + i as u64;
                let mut value = T::default();
                writer(i, &mut value);
                
                unsafe {
                    self.ring_buffer.write_slot(seq, value);
                }
                // Publish each slot immediately after writing
                self.ring_buffer.publish(seq);
            }
            
            Ok(count)
        } else {
            Err("Ring buffer full")
        }
    }

}

pub struct MpscProducerBuilder<T: RingBufferEntry> {
    ring_buffer: Option<Arc<MpscRingBuffer<T>>>,
}

impl<T: RingBufferEntry> MpscProducerBuilder<T> {
    pub fn new() -> Self {
        Self { ring_buffer: None }
    }

    pub fn with_ring_buffer(mut self, ring_buffer: Arc<MpscRingBuffer<T>>) -> Self {
        self.ring_buffer = Some(ring_buffer);
        self
    }

    pub fn build(self) -> Result<MpscProducer<T>, &'static str> {
        let ring_buffer = self.ring_buffer.ok_or("Ring buffer not set")?;
        Ok(MpscProducer::new(ring_buffer))
    }
}

impl<T: RingBufferEntry> Default for MpscProducerBuilder<T> {
    fn default() -> Self {
        Self::new()
    }
}

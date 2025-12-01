//! Consumer API for generic RingBuffer<T>

use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::marker::PhantomData;
use crate::disruptor::{RingBufferEntry, RingBuffer};

/// Event handler trait for RingConsumer
pub trait RingEventHandler<T: RingBufferEntry> {
    fn on_event(&mut self, event: &T, seq: u64, end_of_batch: bool);
}

/// Consumer for generic RingBuffer<T>
pub struct RingConsumer<T: RingBufferEntry> {
    ring_buffer: Arc<RingBuffer<T>>,
    cursor: u64,
    batch_size: usize,
    _phantom: PhantomData<T>,
}

impl<T: RingBufferEntry> RingConsumer<T> {
    pub fn new(ring_buffer: Arc<RingBuffer<T>>, batch_size: usize) -> Self {
        Self {
            ring_buffer,
            cursor: 0,
            batch_size,
            _phantom: PhantomData,
        }
    }

    /// Process available events
    #[inline(always)]
    pub fn process_events<H: RingEventHandler<T>>(&mut self, handler: &mut H) -> usize {
        let producer_seq = self.ring_buffer.producer_cursor().load(Ordering::Acquire);
        let available = producer_seq.saturating_sub(self.cursor);

        if available == 0 {
            return 0;
        }

        let to_consume = available.min(self.batch_size as u64) as usize;

        std::sync::atomic::fence(Ordering::Acquire);
        for i in 0..to_consume {
            let seq = self.cursor + i as u64;
            let end_of_batch = i == to_consume - 1;
            
            unsafe {
                let event = self.ring_buffer.read_slot(seq);
                handler.on_event(&event, seq, end_of_batch);
            }
        }

        self.cursor += to_consume as u64;
        self.ring_buffer.update_consumer(self.cursor);
        to_consume
    }
}

/// Builder for RingConsumer
pub struct RingConsumerBuilder<T: RingBufferEntry> {
    ring_buffer: Option<Arc<RingBuffer<T>>>,
    batch_size: usize,
}

impl<T: RingBufferEntry> RingConsumerBuilder<T> {
    pub fn new() -> Self {
        Self {
            ring_buffer: None,
            batch_size: 8192,
        }
    }

    pub fn with_ring_buffer(mut self, ring_buffer: Arc<RingBuffer<T>>) -> Self {
        self.ring_buffer = Some(ring_buffer);
        self
    }

    pub fn with_batch_size(mut self, batch_size: usize) -> Self {
        self.batch_size = batch_size;
        self
    }

    pub fn build(self) -> Result<RingConsumer<T>, &'static str> {
        let ring_buffer = self.ring_buffer.ok_or("Ring buffer not set")?;
        Ok(RingConsumer::new(ring_buffer, self.batch_size))
    }
}

impl<T: RingBufferEntry> Default for RingConsumerBuilder<T> {
    fn default() -> Self {
        Self::new()
    }
}

//! High-level Consumer API for MpscRingBuffer (Single Consumer)

use std::sync::Arc;
use std::marker::PhantomData;
use crate::disruptor::{ RingBufferEntry, MpscRingBuffer };

/// Event handler trait for MpscConsumer
pub trait MpscEventHandler<T: RingBufferEntry> {
    fn on_event(&mut self, event: &T, seq: u64, end_of_batch: bool);
}

/// Consumer for MPSC
pub struct MpscConsumer<T: RingBufferEntry> {
    ring_buffer: Arc<MpscRingBuffer<T>>,
    cursor: u64,
    batch_size: usize,
    _phantom: PhantomData<T>,
}

impl<T: RingBufferEntry> MpscConsumer<T> {
    pub fn new(ring_buffer: Arc<MpscRingBuffer<T>>, batch_size: usize) -> Self {
        Self {
            ring_buffer,
            cursor: 0,
            batch_size,
            _phantom: PhantomData,
        }
    }

    /// Process available events - uses published sequence for correctness

    pub fn process_events<H: MpscEventHandler<T>>(&mut self, handler: &mut H) -> usize {
        // Get highest contiguous published sequence
        let published_seq = self.ring_buffer.get_published_sequence();
        let available = published_seq.saturating_sub(self.cursor);

        if available == 0 {
            return 0;
        }

        let to_consume = available.min(self.batch_size as u64) as usize;

        for i in 0..to_consume {
            let seq = self.cursor + (i as u64);
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

pub struct MpscConsumerBuilder<T: RingBufferEntry> {
    ring_buffer: Option<Arc<MpscRingBuffer<T>>>,
    batch_size: usize,
}

impl<T: RingBufferEntry> MpscConsumerBuilder<T> {
    pub fn new() -> Self {
        Self {
            ring_buffer: None,
            batch_size: 8192,
        }
    }

    pub fn with_ring_buffer(mut self, ring_buffer: Arc<MpscRingBuffer<T>>) -> Self {
        self.ring_buffer = Some(ring_buffer);
        self
    }

    pub fn with_batch_size(mut self, batch_size: usize) -> Self {
        self.batch_size = batch_size;
        self
    }

    pub fn build(self) -> Result<MpscConsumer<T>, &'static str> {
        let ring_buffer = self.ring_buffer.ok_or("Ring buffer not set")?;
        Ok(MpscConsumer::new(ring_buffer, self.batch_size))
    }
}

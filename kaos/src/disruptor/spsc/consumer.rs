//! Consumer API for MessageRingBuffer
//!
//! Provides LMAX Disruptor-style event handler interface

use std::sync::Arc;
use crate::disruptor::{ MessageRingBuffer, MessageSlot, RingBufferEntry };

/// Event handler trait (LMAX Disruptor style)
pub trait EventHandler: Send {
    fn on_event(&mut self, event: &MessageSlot, sequence: u64, end_of_batch: bool);
}

/// Consumer for MessageRingBuffer
pub struct Consumer {
    ring_buffer: Arc<MessageRingBuffer>,
    consumer_id: usize,
    batch_size: usize,
}

impl Consumer {
    pub fn new(ring_buffer: Arc<MessageRingBuffer>, consumer_id: usize) -> Self {
        Self {
            ring_buffer,
            consumer_id,
            batch_size: 2048,
        }
    }

    pub fn with_batch_size(mut self, batch_size: usize) -> Self {
        self.batch_size = batch_size;
        self
    }

    /// Process available events with the given handler.
    pub fn process_events<H: EventHandler>(&self, handler: &mut H) -> usize {
        let rb = unsafe { &*Arc::as_ptr(&self.ring_buffer) };
        let batch = rb.try_consume_batch_relaxed(self.consumer_id, self.batch_size);

        if batch.is_empty() {
            return 0;
        }

        let count = batch.len();
        let last_idx = count - 1;

        for (i, event) in batch.iter().enumerate() {
            handler.on_event(event, event.sequence(), i == last_idx);
        }

        count
    }

    /// Run consumer in loop until stop flag is set.
    pub fn run_loop<H: EventHandler>(
        &self,
        handler: &mut H,
        stop_flag: &std::sync::atomic::AtomicBool
    ) {
        use std::sync::atomic::Ordering;

        while !stop_flag.load(Ordering::Relaxed) {
            let count = self.process_events(handler);
            if count == 0 {
                std::hint::spin_loop();
            }
        }

        // Final drain
        loop {
            let count = self.process_events(handler);
            if count == 0 {
                break;
            }
        }
    }
}

/// Builder for easy consumer creation
pub struct ConsumerBuilder {
    ring_buffer: Option<Arc<MessageRingBuffer>>,
    consumer_id: usize,
    batch_size: usize,
}

impl ConsumerBuilder {
    pub fn new() -> Self {
        Self {
            ring_buffer: None,
            consumer_id: 0,
            batch_size: 2048,
        }
    }

    pub fn with_ring_buffer(mut self, rb: Arc<MessageRingBuffer>) -> Self {
        self.ring_buffer = Some(rb);
        self
    }

    pub fn with_consumer_id(mut self, id: usize) -> Self {
        self.consumer_id = id;
        self
    }

    pub fn with_batch_size(mut self, size: usize) -> Self {
        self.batch_size = size;
        self
    }

    pub fn build(self) -> Result<Consumer, &'static str> {
        let ring_buffer = self.ring_buffer.ok_or("Ring buffer required")?;
        Ok(Consumer::new(ring_buffer, self.consumer_id).with_batch_size(self.batch_size))
    }
}

impl Default for ConsumerBuilder {
    fn default() -> Self {
        Self::new()
    }
}

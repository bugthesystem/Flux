//! High-level Producer API for MessageRingBuffer

use std::sync::Arc;
use crate::disruptor::{ MessageRingBuffer, MessageSlot };

/// Producer for MessageRingBuffer.
pub struct Producer {
    pub ring_buffer: *mut MessageRingBuffer,
    _arc: Arc<MessageRingBuffer>,
}

unsafe impl Send for Producer {}
// NOT Sync! Only one producer thread should use this.

impl Producer {
    pub fn new(ring_buffer: Arc<MessageRingBuffer>) -> Self {
        let ptr = Arc::as_ptr(&ring_buffer) as *mut MessageRingBuffer;
        Self {
            ring_buffer: ptr,
            _arc: ring_buffer,
        }
    }

    /// Publish batch of events with closure.
    pub fn publish_batch<T, F>(&mut self, items: &[T], writer: F) -> Result<usize, &'static str>
        where F: Fn(&mut MessageSlot, u64, &T)
    {
        let rb = unsafe { &mut *self.ring_buffer };

        let batch_size = items.len();
        if let Some((seq, slots)) = rb.try_claim_slots_relaxed(batch_size) {
            let count = slots.len();

            // OPTIMIZED: Unrolled loop for performance
            unsafe {
                let mut i = 0;
                while i + 8 <= count {
                    writer(slots.get_unchecked_mut(i), seq + (i as u64), &items[i]);
                    writer(slots.get_unchecked_mut(i + 1), seq + ((i + 1) as u64), &items[i + 1]);
                    writer(slots.get_unchecked_mut(i + 2), seq + ((i + 2) as u64), &items[i + 2]);
                    writer(slots.get_unchecked_mut(i + 3), seq + ((i + 3) as u64), &items[i + 3]);
                    writer(slots.get_unchecked_mut(i + 4), seq + ((i + 4) as u64), &items[i + 4]);
                    writer(slots.get_unchecked_mut(i + 5), seq + ((i + 5) as u64), &items[i + 5]);
                    writer(slots.get_unchecked_mut(i + 6), seq + ((i + 6) as u64), &items[i + 6]);
                    writer(slots.get_unchecked_mut(i + 7), seq + ((i + 7) as u64), &items[i + 7]);
                    i += 8;
                }
                while i < count {
                    writer(slots.get_unchecked_mut(i), seq + (i as u64), &items[i]);
                    i += 1;
                }
            }

            rb.publish_batch_relaxed(seq, count);
            Ok(count)
        } else {
            Err("Ring buffer full")
        }
    }
}

/// Builder for easy producer creation
pub struct ProducerBuilder {
    ring_buffer: Option<Arc<MessageRingBuffer>>,
}

impl ProducerBuilder {
    pub fn new() -> Self {
        Self { ring_buffer: None }
    }

    pub fn with_ring_buffer(mut self, rb: Arc<MessageRingBuffer>) -> Self {
        self.ring_buffer = Some(rb);
        self
    }

    pub fn build(self) -> Result<Producer, &'static str> {
        let ring_buffer = self.ring_buffer.ok_or("Ring buffer required")?;
        Ok(Producer::new(ring_buffer))
    }
}

impl Default for ProducerBuilder {
    fn default() -> Self {
        Self::new()
    }
}

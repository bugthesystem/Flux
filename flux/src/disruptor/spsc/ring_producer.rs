//! Producer API for generic RingBuffer<T>

use std::sync::Arc;
use std::marker::PhantomData;
use crate::disruptor::{RingBufferEntry, RingBuffer};

/// Producer for generic RingBuffer<T>
pub struct RingProducer<T: RingBufferEntry> {
    ring_buffer: *mut RingBuffer<T>,
    _arc: Arc<RingBuffer<T>>,
    cursor: u64,
    _phantom: PhantomData<T>,
}

unsafe impl<T: RingBufferEntry> Send for RingProducer<T> {}

impl<T: RingBufferEntry> RingProducer<T> {
    pub fn new(ring_buffer: Arc<RingBuffer<T>>) -> Self {
        let ptr = Arc::as_ptr(&ring_buffer) as *mut RingBuffer<T>;
        Self {
            ring_buffer: ptr,
            _arc: ring_buffer,
            cursor: 0,
            _phantom: PhantomData,
        }
    }

    /// Publish single event
    #[inline(always)]
    pub fn publish<F>(&mut self, writer: F) -> Result<(), &'static str>
    where
        F: FnOnce(&mut T),
    {
        let rb = unsafe { &mut *self.ring_buffer };
        
        if let Some(next) = rb.try_claim(1, self.cursor) {
            let seq = self.cursor;
            let mut value = T::default();
            writer(&mut value);
            
            unsafe { rb.write_slot(seq, value); }
            
            self.cursor = next;
            rb.publish(next);
            Ok(())
        } else {
            Err("Ring buffer full")
        }
    }

    /// Publish batch
    #[inline(always)]
    pub fn publish_batch<F>(&mut self, count: usize, mut writer: F) -> Result<usize, &'static str>
    where
        F: FnMut(usize, &mut T),
    {
        let rb = unsafe { &mut *self.ring_buffer };
        
        if let Some(next) = rb.try_claim(count, self.cursor) {
            let start_seq = self.cursor;
            for i in 0..count {
                let seq = start_seq + i as u64;
                let mut value = T::default();
                writer(i, &mut value);
                unsafe { rb.write_slot(seq, value); }
            }
            
            self.cursor = next;
            rb.publish(next);
            Ok(count)
        } else {
            Err("Ring buffer full")
        }
    }
    
    /// Try to publish batch - returns actual count
    #[inline(always)]
    pub fn try_publish_batch<F>(&mut self, max_count: usize, mut writer: F) -> usize
    where
        F: FnMut(usize, &mut T),
    {
        let rb = unsafe { &mut *self.ring_buffer };
        
        if let Some(next) = rb.try_claim(max_count, self.cursor) {
            let start_seq = self.cursor;
            for i in 0..max_count {
                let seq = start_seq + i as u64;
                let mut value = T::default();
                writer(i, &mut value);
                unsafe { rb.write_slot(seq, value); }
            }
            
            self.cursor = next;
            rb.publish(next);
            max_count
        } else {
            0
        }
    }
}

/// Builder for RingProducer
pub struct RingProducerBuilder<T: RingBufferEntry> {
    ring_buffer: Option<Arc<RingBuffer<T>>>,
}

impl<T: RingBufferEntry> RingProducerBuilder<T> {
    pub fn new() -> Self {
        Self { ring_buffer: None }
    }

    pub fn with_ring_buffer(mut self, ring_buffer: Arc<RingBuffer<T>>) -> Self {
        self.ring_buffer = Some(ring_buffer);
        self
    }

    pub fn build(self) -> Result<RingProducer<T>, &'static str> {
        let ring_buffer = self.ring_buffer.ok_or("Ring buffer not set")?;
        Ok(RingProducer::new(ring_buffer))
    }
}

impl<T: RingBufferEntry> Default for RingProducerBuilder<T> {
    fn default() -> Self {
        Self::new()
    }
}

//! High-level Producer API with excellent DevX
//!
//! Provides LMAX Disruptor-style API while maintaining performance

use std::sync::Arc;
use crate::disruptor::{MessageRingBuffer, MessageSlot};

/// High-performance producer for MessageRingBuffer
///
/// Uses local cursor to minimize atomic operations.
pub struct Producer {
    pub ring_buffer: *mut MessageRingBuffer,
    _arc: Arc<MessageRingBuffer>,
    
    /// Local cursor tracking next sequence to claim
    /// This is NOT atomic - only this producer thread accesses it!
    /// Eliminates cache ping-pong with consumer on every claim
    cursor: u64,
}

unsafe impl Send for Producer {}
// NOT Sync! Only one producer thread should use this.

impl Producer {
    pub fn new(ring_buffer: Arc<MessageRingBuffer>) -> Self {
        let ptr = Arc::as_ptr(&ring_buffer) as *mut MessageRingBuffer;
        Self {
            ring_buffer: ptr,
            _arc: ring_buffer,
            cursor: 0,  // Start at 0 (first claim will be sequence 1)
        }
    }
    
    /// Get current cursor value (for debugging/monitoring)
    #[inline(always)]
    pub fn cursor(&self) -> u64 {
        self.cursor
    }
    
    /// Publish single event with closure.
    #[inline(always)]
    pub fn publish_event<F>(&mut self, writer: F) -> Result<(), &'static str>
    where
        F: FnOnce(&mut MessageSlot, u64),
    {
        let rb = unsafe { &mut *self.ring_buffer };
        
        if let Some((seq, slots)) = rb.try_claim_slots_relaxed(1) {
            writer(&mut slots[0], seq);
            rb.publish_batch_relaxed(seq, 1);
            Ok(())
        } else {
            Err("Ring buffer full")
        }
    }
    
    /// Publish batch of events with closure.
    #[inline(always)]
    pub fn publish_batch<T, F>(
        &mut self,
        items: &[T],
        writer: F,
    ) -> Result<usize, &'static str>
    where
        F: Fn(&mut MessageSlot, u64, &T),
    {
        let rb = unsafe { &mut *self.ring_buffer };
        
        let batch_size = items.len();
        if let Some((seq, slots)) = rb.try_claim_slots_relaxed(batch_size) {
            let count = slots.len();
            
            // OPTIMIZED: Unrolled loop for performance
            unsafe {
                let mut i = 0;
                while i + 8 <= count {
                    writer(slots.get_unchecked_mut(i), seq + i as u64, &items[i]);
                    writer(slots.get_unchecked_mut(i+1), seq + (i+1) as u64, &items[i+1]);
                    writer(slots.get_unchecked_mut(i+2), seq + (i+2) as u64, &items[i+2]);
                    writer(slots.get_unchecked_mut(i+3), seq + (i+3) as u64, &items[i+3]);
                    writer(slots.get_unchecked_mut(i+4), seq + (i+4) as u64, &items[i+4]);
                    writer(slots.get_unchecked_mut(i+5), seq + (i+5) as u64, &items[i+5]);
                    writer(slots.get_unchecked_mut(i+6), seq + (i+6) as u64, &items[i+6]);
                    writer(slots.get_unchecked_mut(i+7), seq + (i+7) as u64, &items[i+7]);
                    i += 8;
                }
                while i < count {
                    writer(slots.get_unchecked_mut(i), seq + i as u64, &items[i]);
                    i += 1;
                }
            }
            
            rb.publish_batch_relaxed(seq, count);
            Ok(count)
        } else {
            Err("Ring buffer full")
        }
    }
    
    /// Try to publish, yields if full (for high-throughput loops)
    #[inline(always)]
    pub fn try_publish_batch<T, F>(
        &mut self,
        items: &[T],
        writer: F,
    ) -> usize
    where
        F: Fn(&mut MessageSlot, u64, &T),
    {
        match self.publish_batch(items, writer) {
            Ok(count) => count,
            Err(_) => {
                std::thread::yield_now();
                0
            }
        }
    }
    
    // ═══════════════════════════════════════════════════════════════
    // OPTIMIZED LMAX-STYLE API - Local Cursor (Zero Atomic Claim!)
    // ═══════════════════════════════════════════════════════════════
    
    /// Claim slots using local cursor (ZERO atomic operations!)
    ///
    /// This is the LMAX Disruptor pattern:
    /// - Read/write local cursor (non-atomic variable)
    /// - Only write to shared atomic on publish()
    /// 
    /// # Performance
    /// Claim slots without atomic operations on the claim path.
    /// Uses local cursor tracking - only publish uses atomics.
    #[inline(always)]
    pub fn try_claim_relaxed(&mut self, count: usize) -> Option<(u64, &mut [MessageSlot])> {
        let rb = unsafe { &mut *self.ring_buffer };
        
        // Read LOCAL cursor (not atomic!)
        let current = self.cursor;
        let next = current + (count as u64);  // Simple add (Java style!)
        
        // Check space available - lazy gating update
        let mut min_consumer_seq = rb.gating_sequence_relaxed();
        if next - min_consumer_seq > (rb.capacity() as u64) {
            // Ring appears full - update gating with fresh consumer sequences
            rb.update_gating_lazy();
            std::sync::atomic::fence(std::sync::atomic::Ordering::Acquire);
            min_consumer_seq = rb.gating_sequence_relaxed();
            
            // Re-check with fresh value
            if next - min_consumer_seq > (rb.capacity() as u64) {
                return None;  // Ring actually full
            }
        }
        
        // Update LOCAL cursor (not atomic!)
        self.cursor = next;
        
        // Calculate slot indices - simple arithmetic (Java style!)
        let start_idx = ((current + 1) as usize) & rb.mask();
        let end_idx = ((next + 1) as usize) & rb.mask();
        let start_seq = current + 1;
        
        // Return slice view
        let slots = if start_idx < end_idx {
            &mut rb.buffer_mut()[start_idx..end_idx]
        } else {
            let available_slots = rb.capacity() - start_idx;
            let actual_count = count.min(available_slots);
            &mut rb.buffer_mut()[start_idx..start_idx + actual_count]
        };
        
        Some((start_seq, slots))
    }
    
    /// Publish claimed slots (SINGLE atomic write!)
    ///
    /// This writes to the shared producer_sequence with Release ordering,
    /// making the batch visible to consumers.
    ///
    /// # Arguments
    /// - `start_seq`: Starting sequence from try_claim_relaxed()
    /// - `count`: Number of slots to publish
    #[inline(always)]
    pub fn publish_relaxed(&self, start_seq: u64, count: usize) {
        let rb = unsafe { &*self.ring_buffer };
        let end_seq = start_seq + (count as u64) - 1;
        
        // SINGLE atomic write with Release ordering
        // This is the ONLY write to shared memory in the claim+publish cycle!
        rb.publish_sequence(end_seq);
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

//! MessageRingBuffer - SPSC ring buffer for 128-byte MessageSlot.
//!
//! For generic fixed-size slot types, use `RingBuffer<T>` instead.

use std::sync::atomic::Ordering;

use crate::disruptor::{ RingBufferConfig, MessageSlot, RingBufferEntry };
use crate::disruptor::claim_batch::ClaimBatch;
use crate::error::{ Result, FluxError };
use crate::constants::CACHE_PREFETCH_LINES;

#[cfg(target_arch = "x86_64")]
use std::arch::x86_64::{ _mm256_loadu_si256, _mm256_storeu_si256, __m256i };
use crate::disruptor::common::{ PaddedProducerSequence, PaddedConsumerSequence };

/// MessageSlot-based ring buffer with cache-line padding
///
/// Optimized for 128-byte `MessageSlot` entries with:
/// - Cache-line aligned data structures
/// - Batch processing
/// - Prefetching
///
/// For generic slot types, use `RingBuffer<T>`.
pub struct MessageRingBuffer {
    config: RingBufferConfig,
    buffer: Box<[MessageSlot]>,
    mask: usize,
    producer_sequence: PaddedProducerSequence,
    /// Each consumer has its own sequence number for independent progress tracking
    consumer_sequences: Vec<PaddedConsumerSequence>,
    /// Gating sequence (cache-line padded)
    /// Used to prevent the producer from overwriting unread messages
    _gating_sequence: PaddedProducerSequence,
}

impl MessageRingBuffer {
    ///
    /// # Arguments
    ///
    /// * `config` - Ring buffer configuration including size, number of consumers, etc.
    ///
    /// Creates a new MessageRingBuffer with the given configuration.
    pub fn new(config: RingBufferConfig) -> Result<Self> {
        if config.size == 0 || (config.size & (config.size - 1)) != 0 {
            return Err(FluxError::config("Ring buffer size must be a power of 2"));
        }

        let buffer = vec![MessageSlot::default(); config.size].into_boxed_slice();
        let mask = config.size - 1;

        let consumer_sequences = (0..config.num_consumers)
            .map(|_| PaddedConsumerSequence::new(u64::MAX))
            .collect();

        Ok(Self {
            config,
            buffer,
            mask,
            producer_sequence: PaddedProducerSequence::new(u64::MAX), // Start at MAX (no slots claimed yet)
            consumer_sequences,
            _gating_sequence: PaddedProducerSequence::new(u64::MAX), // Start at MAX (LMAX convention)
        })
    }

    pub fn capacity(&self) -> usize {
        self.config.size
    }

    /// Get the number of consumers configured for this ring buffer
    pub fn consumer_count(&self) -> usize {
        self.config.num_consumers
    }

    ///
    /// and publishing in a single operation. For maximum performance, use `try_claim_slots`
    /// and `publish_batch` separately.
    ///
    /// # Arguments
    ///
    /// * `data_items` - Slice of data items to publish
    ///
    /// # Returns
    ///
    /// * `Result<u64>` - Number of items successfully published or an error
    pub fn try_publish_batch(&mut self, data_items: &[&[u8]]) -> Result<u64> {
        let count = data_items.len();
        if count == 0 {
            return Ok(0);
        }

        let current_seq = self.producer_sequence.sequence.load(Ordering::Relaxed);
        let next_seq = current_seq + (count as u64);

        let min_consumer_seq = self.get_minimum_consumer_sequence();
        if next_seq > min_consumer_seq + (self.config.size as u64) {
            return Err(FluxError::RingBufferFull);
        }

        match
            self.producer_sequence.sequence.compare_exchange_weak(
                current_seq,
                next_seq,
                Ordering::AcqRel,
                Ordering::Relaxed
            )
        {
            Ok(_) => {
                for (i, data) in data_items.iter().enumerate() {
                    let slot_index = ((current_seq + 1 + (i as u64)) & (self.mask as u64)) as usize;
                    let slot = &mut self.buffer[slot_index];

                    slot.set_sequence(current_seq + 1 + (i as u64));
                    slot.set_data(data);
                }

                std::sync::atomic::fence(Ordering::Release);

                Ok(count as u64)
            }
            Err(_) => Err(FluxError::RingBufferFull),
        }
    }

    /// Prefetch slots into L1 cache for better performance
    fn prefetch_slots(&self, start_index: usize, count: usize) {
        for i in 0..count.min(CACHE_PREFETCH_LINES * 32) {
            let slot_index = (start_index + i) & self.mask;
            let slot_ptr = &self.buffer[slot_index] as *const MessageSlot;

            #[cfg(target_arch = "aarch64")]
            unsafe {
                std::arch::asm!(
                    "prfm pldl1keep, [{ptr}]",
                    ptr = in(reg) slot_ptr,
                    options(nostack)
                );
            }

            #[cfg(target_arch = "x86_64")]
            unsafe {
                std::arch::x86_64::_mm_prefetch(
                    slot_ptr as *const i8,
                    std::arch::x86_64::_MM_HINT_T0
                );
            }
        }
    }

    /// Claim slots with BLOCKING wait (LMAX Disruptor style)
    /// Guarantees claim succeeds - waits until space available
    /// Use this for 100% delivery guarantee
    pub fn claim_slots(&mut self, count: usize) -> (u64, &mut [MessageSlot]) {
        let self_ptr = self as *mut MessageRingBuffer;
        loop {
            let rb = unsafe { &mut *self_ptr };
            if let Some(claimed) = rb.try_claim_slots(count) {
                return claimed;
            }
            std::hint::spin_loop();
        }
    }

    /// Direct access: Claim slots directly for maximum performance
    pub fn try_claim_slots(&mut self, count: usize) -> Option<(u64, &mut [MessageSlot])> {
        if count == 0 {
            return None;
        }

        let current_seq = self.producer_sequence.sequence.load(Ordering::Relaxed);
        let (slot_seq, next_seq) = if current_seq == u64::MAX {
            (0, (count as u64) - 1)
        } else {
            (current_seq + 1, current_seq + (count as u64))
        };

        let min_consumer_seq = self.get_minimum_consumer_sequence();
        if min_consumer_seq == u64::MAX {
            if next_seq > (self.config.size as u64) - 1 {
                return None;
            }
        } else if next_seq > min_consumer_seq + (self.config.size as u64) {
            return None;
        }

        match
            self.producer_sequence.sequence.compare_exchange_weak(
                current_seq,
                next_seq,
                Ordering::AcqRel,
                Ordering::Relaxed
            )
        {
            Ok(_) => {
                self.prefetch_slots((slot_seq as usize) & self.mask, count * 2);

                let start_idx = (slot_seq as usize) & self.mask;
                let end_idx = ((slot_seq + (count as u64)) as usize) & self.mask;

                if start_idx < end_idx {
                    Some((slot_seq, &mut self.buffer[start_idx..end_idx]))
                } else {
                    let available_slots = self.config.size - start_idx;
                    let actual_count = count.min(available_slots);
                    Some((slot_seq, &mut self.buffer[start_idx..start_idx + actual_count]))
                }
            }
            Err(_) => None,
        }
    }

    /// Claim slots with BLOCKING wait + relaxed ordering (fastest 100% delivery)
    /// Combines blocking wait with relaxed atomics for maximum throughput
    pub fn claim_slots_relaxed(&mut self, count: usize) -> (u64, &mut [MessageSlot]) {
        let self_ptr = self as *mut MessageRingBuffer;
        loop {
            let rb = unsafe { &mut *self_ptr };
            if let Some(claimed) = rb.try_claim_slots_relaxed(count) {
                return claimed;
            }
            std::hint::spin_loop();
        }
    }

    /// Try to claim slots with relaxed memory ordering for single-producer scenarios
    ///
    /// This is a performance-optimized variant that uses relaxed atomic ordering.
    /// Only use in single-producer scenarios where strict ordering is not required.
    ///
    #[inline(always)]
    pub fn try_claim_slots_relaxed(&mut self, count: usize) -> Option<(u64, &mut [MessageSlot])> {
        let current = self.producer_sequence.sequence.load(Ordering::Relaxed);
        let next = current + (count as u64);

        // Use cached gating sequence (O(1) instead of O(n), Relaxed instead of Acquire)
        let mut min_consumer_seq = self._gating_sequence.sequence.load(Ordering::Relaxed);
        if min_consumer_seq == u64::MAX {
            if next > (self.config.size as u64) {
                return None;
            }
        } else if next - min_consumer_seq > (self.config.size as u64) {
            // Ring appears full - update gating sequence LAZILY (LMAX pattern!)
            // This is the ONLY place we update it - only when producer needs fresh value
            self.update_gating_sequence();
            // Acquire fence after updating gating sequence
            std::sync::atomic::fence(Ordering::Acquire);
            min_consumer_seq = self._gating_sequence.sequence.load(Ordering::Relaxed);

            // Re-check with fresh value
            if next - min_consumer_seq > (self.config.size as u64) {
                return None;
            }
        }

        // DON'T update producer_sequence here!
        // Let publish_batch_relaxed() do it with Release ordering.
        // This matches the LMAX pattern: claim is local, publish is atomic.

        // Return slots starting from NEXT sequence (current + 1)
        let start_seq = current + 1;
        let start_idx = (start_seq as usize) & self.mask;
        let end_idx = ((start_seq + (count as u64)) as usize) & self.mask;

        let slots = if start_idx < end_idx {
            &mut self.buffer[start_idx..end_idx]
        } else {
            let available_slots = self.config.size - start_idx;
            let actual_count = count.min(available_slots);

            self.prefetch_slots_aggressive(start_idx, actual_count);

            &mut self.buffer[start_idx..start_idx + actual_count]
        };

        Some((start_seq, slots))
    }

    fn prefetch_slots_aggressive(&self, start_index: usize, count: usize) {
        const PREFETCH_LINES: usize = 8;

        for i in 0..count.min(PREFETCH_LINES * 16) {
            let slot_index = (start_index + i) & self.mask;
            let slot_ptr = unsafe { self.buffer.as_ptr().add(slot_index) };

            #[cfg(target_arch = "aarch64")]
            unsafe {
                std::arch::asm!(
                    "prfm pldl1keep, [{ptr}]",
                    "prfm pldl2keep, [{ptr}]",
                    ptr = in(reg) slot_ptr,
                    options(nostack)
                );
            }

            #[cfg(target_arch = "x86_64")]
            unsafe {
                std::arch::x86_64::_mm_prefetch(
                    slot_ptr as *const i8,
                    std::arch::x86_64::_MM_HINT_T0
                );
                std::arch::x86_64::_mm_prefetch(
                    slot_ptr as *const i8,
                    std::arch::x86_64::_MM_HINT_T1
                );
            }
        }
    }

    /// Try to claim batch with zero-allocation descriptor.
    /// Returns a ClaimBatch for direct pointer access.
    #[inline(always)]
    pub fn try_claim_batch_relaxed(&mut self, count: usize) -> Option<ClaimBatch> {
        let current = self.producer_sequence.sequence.load(Ordering::Relaxed);
        let next = current + (count as u64);

        // Use cached gating sequence
        let min_consumer_seq = self._gating_sequence.sequence.load(Ordering::Relaxed);
        if min_consumer_seq == u64::MAX {
            if next > (self.config.size as u64) {
                return None;
            }
        } else if next - min_consumer_seq > (self.config.size as u64) {
            return None;
        }

        // DON'T store here! publish_batch_relaxed() will do it with Release ordering.
        // Same critical bug as in try_claim_slots_relaxed!

        let start_idx = (current as usize) & self.mask;
        let end_idx = (next as usize) & self.mask;

        // Handle wraparound: limit to available slots before ring wraps
        let actual_count = if start_idx < end_idx {
            count
        } else {
            let available_slots = self.config.size - start_idx;
            count.min(available_slots)
        };

        // Get raw buffer pointer
        let buffer_ptr = self.buffer.as_mut_ptr();

        unsafe { Some(ClaimBatch::new(current, actual_count, buffer_ptr, self.mask)) }
    }

    /// Fast batch publish with minimal synchronization
    ///
    /// atomic ordering for maximum performance. It should only be used in scenarios
    /// where the caller can guarantee that the published slots will be consumed
    /// by a single consumer thread.
    /// Publish batch with relaxed memory ordering for single-producer scenarios
    ///
    /// This is a performance-optimized variant that uses relaxed atomic ordering.
    /// Only use in single-producer scenarios where strict ordering is not required.
    ///
    pub fn publish_batch_relaxed(&self, start_seq: u64, count: usize) {
        // Publish the END sequence of the batch with Release ordering
        // This makes all slot writes visible to consumers
        let end_seq = start_seq + (count as u64) - 1;
        self.producer_sequence.sequence.store(end_seq, Ordering::Release);
    }

    pub fn try_consume_batch(&self, consumer_id: usize, max_count: usize) -> Vec<&MessageSlot> {
        if consumer_id >= self.consumer_sequences.len() {
            return Vec::new();
        }

        let consumer_seq = &self.consumer_sequences[consumer_id];
        let current_seq = consumer_seq.sequence.load(Ordering::Relaxed);
        let producer_seq = self.producer_sequence.sequence.load(Ordering::Relaxed);

        // When consumer hasn't started (u64::MAX), producer_seq represents
        // the last published sequence. Messages are at [0, producer_seq].
        // When consumer has started, messages are at [current_seq+1, producer_seq].
        let (first_seq, available) = if current_seq == u64::MAX {
            // Consumer hasn't started - messages start at seq 0
            if producer_seq == u64::MAX {
                return Vec::new(); // Nothing published yet
            }
            (0u64, producer_seq + 1) // Sequences 0..=producer_seq (count = producer_seq + 1)
        } else {
            let next_seq = current_seq + 1;
            if next_seq > producer_seq {
                return Vec::new();
            }
            (next_seq, producer_seq - current_seq)
        };

        if available == 0 {
            return Vec::new();
        }

        let count = available.min(max_count as u64) as usize;
        let mut messages = Vec::with_capacity(count);

        self.prefetch_slots((first_seq & (self.mask as u64)) as usize, count * 2);

        for i in 0..count {
            let seq = first_seq + (i as u64);

            let slot_index = (seq & (self.mask as u64)) as usize;
            let slot = &self.buffer[slot_index];

            if slot.sequence() != seq {
                break;
            }

            messages.push(slot);
        }

        if !messages.is_empty() {
            // new_seq = last consumed sequence (inclusive)
            let new_seq = first_seq + (messages.len() as u64) - 1;
            consumer_seq.sequence.store(new_seq, Ordering::Relaxed);

            // Update gating sequence every 1000 messages to reduce overhead
            if new_seq % 1000 < (messages.len() as u64) {
                self.update_gating_sequence();
            }
        }

        messages
    }

    /// Peek at batch without consuming (non-destructive read for retransmission)
    pub fn peek_batch(&self, consumer_id: usize, max_count: usize) -> Vec<&MessageSlot> {
        if consumer_id >= self.consumer_sequences.len() {
            return Vec::new();
        }

        let consumer_seq = &self.consumer_sequences[consumer_id];
        let current_seq = consumer_seq.sequence.load(Ordering::Relaxed);
        let producer_seq = self.producer_sequence.sequence.load(Ordering::Acquire);

        let available = if current_seq == u64::MAX {
            producer_seq
        } else {
            producer_seq.saturating_sub(current_seq)
        };

        if available == 0 {
            return Vec::new();
        }

        let count = available.min(max_count as u64) as usize;
        let mut messages = Vec::with_capacity(count);

        for i in 0..count {
            let seq = if current_seq == u64::MAX { i as u64 } else { current_seq + 1 + (i as u64) };
            let slot_index = (seq & (self.mask as u64)) as usize;
            let slot = &self.buffer[slot_index];

            if slot.sequence() != seq {
                break;
            }

            messages.push(slot);
        }

        messages
    }

    /// Consume all available messages for the given consumer.
    #[inline(always)]
    pub fn consume_all_available_relaxed(&self, consumer_id: usize) -> &[MessageSlot] {
        let mut current = self.consumer_sequences[consumer_id].sequence.load(Ordering::Relaxed);
        let was_max = current == u64::MAX;
        if was_max {
            current = 0;
            self.consumer_sequences[consumer_id].sequence.store(0, Ordering::Relaxed);
        }
        let producer_seq = self.producer_sequence.sequence.load(Ordering::Relaxed);
        let available = producer_seq.saturating_sub(current);

        if available == 0 {
            return &[];
        }

        // LMAX style: Consume ALL available, not fixed batch
        let start_idx = (current as usize) & self.mask;
        let available_to_end = self.config.size - start_idx;
        let actual_batch_size = (available as usize).min(available_to_end);

        if actual_batch_size == 0 {
            return &[];
        }

        let new_seq = current + (actual_batch_size as u64);
        self.consumer_sequences[consumer_id].sequence.store(new_seq, Ordering::Release);

        // Update gating sequence every 1000 messages to reduce overhead
        if new_seq % 1000 < (actual_batch_size as u64) {
            self.update_gating_sequence();
        }

        &self.buffer[start_idx..start_idx + actual_batch_size]
    }

    /// Direct memory access batch consumption with relaxed ordering
    ///
    /// This is a performance-optimized variant for single-consumer scenarios.
    /// Returns a slice view instead of allocating a Vec.
    ///
    /// Faster than try_consume_batch() due to zero allocations
    #[inline(always)]
    pub fn try_consume_batch_relaxed(&self, consumer_id: usize, count: usize) -> &[MessageSlot] {
        let mut current = self.consumer_sequences[consumer_id].sequence.load(Ordering::Relaxed);
        let was_max = current == u64::MAX;
        if was_max {
            current = 0;
            self.consumer_sequences[consumer_id].sequence.store(0, Ordering::Relaxed);
        }
        let producer_seq = self.producer_sequence.sequence.load(Ordering::Relaxed);
        let available = producer_seq.saturating_sub(current);

        if available == 0 {
            return &[];
        }

        // Start reading from NEXT sequence (current+1, or 0 if transitioning from MAX)
        let start_seq = if was_max { 0 } else { current + 1 };
        let start_idx = (start_seq as usize) & self.mask;
        let available_to_end = self.config.size - start_idx;
        let requested_batch = count.min(available as usize);
        let actual_batch_size = requested_batch.min(available_to_end);

        if actual_batch_size == 0 {
            return &[];
        }

        let new_seq = start_seq + (actual_batch_size as u64) - 1; // End sequence (inclusive)
        self.consumer_sequences[consumer_id].sequence.store(new_seq, Ordering::Relaxed);

        // Update gating sequence every 1000 messages to reduce overhead
        if new_seq % 1000 < (actual_batch_size as u64) {
            self.update_gating_sequence();
        }

        &self.buffer[start_idx..start_idx + actual_batch_size]
    }

    /// Publish a batch of slots (called after filling them)
    pub fn publish_batch(&self, _start_seq: u64, _count: usize) {
        std::sync::atomic::fence(Ordering::Release);
    }

    pub fn try_consume(&self, consumer_id: usize) -> Result<&MessageSlot> {
        let messages = self.try_consume_batch(consumer_id, 1);
        if messages.is_empty() {
            Err(FluxError::RingBufferFull)
        } else {
            Ok(messages[0])
        }
    }

    /// Get the minimum consumer sequence (for producer gating)
    /// Cached in _gating_sequence to avoid O(n) scan on every claim
    fn get_minimum_consumer_sequence(&self) -> u64 {
        self._gating_sequence.sequence.load(Ordering::Relaxed) // Relaxed! Fence added when needed
    }

    /// Update gating sequence when consumer progresses
    fn update_gating_sequence(&self) {
        let min_seq = self.consumer_sequences
            .iter()
            .map(|cs| cs.sequence.load(Ordering::Relaxed)) // Relaxed reads
            .min()
            .unwrap_or(u64::MAX);
        self._gating_sequence.sequence.store(min_seq, Ordering::Relaxed); // Relaxed write
    }

    /// Lazy update of gating sequence (called by Producer when ring appears full)
    pub fn update_gating_lazy(&self) {
        self.update_gating_sequence();
    }

    // ═══════════════════════════════════════════════════════════════
    // Public API for Producer (LMAX-style local cursor pattern)
    // ═══════════════════════════════════════════════════════════════

    /// Get gating sequence with Relaxed ordering (for Producer claim check)
    #[inline(always)]
    pub fn gating_sequence_relaxed(&self) -> u64 {
        self._gating_sequence.sequence.load(Ordering::Relaxed)
    }

    /// Get ring buffer mask (for index calculation)
    #[inline(always)]
    pub fn mask(&self) -> usize {
        self.mask
    }

    /// Get mutable reference to buffer (for Producer slot access)
    #[inline(always)]
    pub fn buffer_mut(&mut self) -> &mut [MessageSlot] {
        &mut self.buffer
    }

    /// Publish sequence to consumers (SINGLE atomic write!)
    /// This is called by Producer after filling slots.
    #[inline(always)]
    pub fn publish_sequence(&self, seq: u64) {
        self.producer_sequence.sequence.store(seq, Ordering::Release);
    }

    /// Advance consumer sequence (for fire-and-forget patterns)
    /// This simulates immediate consumption/acknowledgment of messages.
    #[inline(always)]
    pub fn advance_consumer(&self, consumer_id: usize, seq: u64) {
        if consumer_id < self.consumer_sequences.len() {
            self.consumer_sequences[consumer_id].sequence.store(seq, Ordering::Release);
            self.update_gating_sequence();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ring_buffer_creation() {
        let config = RingBufferConfig::new(1024).unwrap().with_consumers(2).unwrap();

        let ring_buffer = MessageRingBuffer::new(config).unwrap();
        assert_eq!(ring_buffer.capacity(), 1024);
        assert_eq!(ring_buffer.consumer_count(), 2);
    }

    #[test]
    fn test_ring_buffer_operations() {
        let config = RingBufferConfig::new(1024).unwrap().with_consumers(1).unwrap();

        let ring_buffer = MessageRingBuffer::new(config).unwrap();

        let result = ring_buffer.try_consume(0);
        // Accept either an error or an empty message, depending on implementation
        assert!(
            result.is_err() ||
                (result.is_ok() &&
                    result
                        .as_ref()
                        .map(|slot| slot.data().is_empty())
                        .unwrap_or(false))
        );
    }
}

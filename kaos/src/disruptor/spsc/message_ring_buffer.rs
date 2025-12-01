//! MessageRingBuffer - SPSC ring buffer for 128-byte MessageSlot.
//!
//! For generic fixed-size slot types, use `RingBuffer<T>` instead.

use std::sync::atomic::Ordering;

use crate::disruptor::{ RingBufferConfig, MessageSlot, RingBufferEntry };
use crate::error::{ Result, KaosError };
use crate::disruptor::common::{ PaddedProducerSequence, PaddedConsumerSequence };

/// SPSC ring buffer for 128-byte `MessageSlot` entries.
pub struct MessageRingBuffer {
    config: RingBufferConfig,
    buffer: Box<[MessageSlot]>,
    mask: usize,
    producer_sequence: PaddedProducerSequence,
    consumer_sequences: Vec<PaddedConsumerSequence>,
    _gating_sequence: PaddedProducerSequence,
}

impl MessageRingBuffer {
    pub fn new(config: RingBufferConfig) -> Result<Self> {
        if config.size == 0 || (config.size & (config.size - 1)) != 0 {
            return Err(KaosError::config("Ring buffer size must be a power of 2"));
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

    /// Try to claim `count` slots. Returns (start_seq, slot_slice) or None if full.
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

    /// Relaxed ordering variant for SPSC.
    pub fn try_claim_slots_relaxed(&mut self, count: usize) -> Option<(u64, &mut [MessageSlot])> {
        let current = self.producer_sequence.sequence.load(Ordering::Relaxed);
        let next = current + (count as u64);

        let mut min_consumer_seq = self._gating_sequence.sequence.load(Ordering::Relaxed);
        if min_consumer_seq == u64::MAX {
            if next > (self.config.size as u64) {
                return None;
            }
        } else if next - min_consumer_seq > (self.config.size as u64) {
            self.update_gating_sequence();
            std::sync::atomic::fence(Ordering::Acquire);
            min_consumer_seq = self._gating_sequence.sequence.load(Ordering::Relaxed);
            if next - min_consumer_seq > (self.config.size as u64) {
                return None;
            }
        }

        let start_seq = current + 1;
        let start_idx = (start_seq as usize) & self.mask;
        let end_idx = ((start_seq + (count as u64)) as usize) & self.mask;

        let slots = if start_idx < end_idx {
            &mut self.buffer[start_idx..end_idx]
        } else {
            let available_slots = self.config.size - start_idx;
            let actual_count = count.min(available_slots);
            &mut self.buffer[start_idx..start_idx + actual_count]
        };
        Some((start_seq, slots))
    }

    /// Publish batch with Release ordering.
    pub fn publish_batch_relaxed(&self, start_seq: u64, count: usize) {
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

        let (first_seq, available) = if current_seq == u64::MAX {
            if producer_seq == u64::MAX {
                return Vec::new();
            }
            (0u64, producer_seq + 1)
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
            let new_seq = first_seq + (messages.len() as u64) - 1;
            consumer_seq.sequence.store(new_seq, Ordering::Relaxed);
            if new_seq % 1000 < (messages.len() as u64) {
                self.update_gating_sequence();
            }
        }

        messages
    }

    /// Peek without consuming (for retransmission).
    pub fn peek_batch(&self, consumer_id: usize, max_count: usize) -> Vec<&MessageSlot> {
        if consumer_id >= self.consumer_sequences.len() { return Vec::new(); }

        let consumer_seq = &self.consumer_sequences[consumer_id];
        let current_seq = consumer_seq.sequence.load(Ordering::Relaxed);
        let producer_seq = self.producer_sequence.sequence.load(Ordering::Acquire);

        let available = if current_seq == u64::MAX { producer_seq }
                        else { producer_seq.saturating_sub(current_seq) };
        if available == 0 { return Vec::new(); }

        let count = available.min(max_count as u64) as usize;
        let mut messages = Vec::with_capacity(count);

        for i in 0..count {
            let seq = if current_seq == u64::MAX { i as u64 } else { current_seq + 1 + (i as u64) };
            let slot_index = (seq & (self.mask as u64)) as usize;
            let slot = &self.buffer[slot_index];
            if slot.sequence() != seq { break; }
            messages.push(slot);
        }
        messages
    }

    /// Zero-allocation batch consumption returning a slice.
    pub fn try_consume_batch_relaxed(&self, consumer_id: usize, count: usize) -> &[MessageSlot] {
        let mut current = self.consumer_sequences[consumer_id].sequence.load(Ordering::Relaxed);
        let was_max = current == u64::MAX;
        if was_max {
            current = 0;
            self.consumer_sequences[consumer_id].sequence.store(0, Ordering::Relaxed);
        }
        let producer_seq = self.producer_sequence.sequence.load(Ordering::Relaxed);
        let available = producer_seq.saturating_sub(current);
        if available == 0 { return &[]; }

        let start_seq = if was_max { 0 } else { current + 1 };
        let start_idx = (start_seq as usize) & self.mask;
        let available_to_end = self.config.size - start_idx;
        let requested_batch = count.min(available as usize);
        let actual_batch_size = requested_batch.min(available_to_end);
        if actual_batch_size == 0 { return &[]; }

        let new_seq = start_seq + (actual_batch_size as u64) - 1;
        self.consumer_sequences[consumer_id].sequence.store(new_seq, Ordering::Relaxed);

        if new_seq % 1000 < (actual_batch_size as u64) {
            self.update_gating_sequence();
        }
        &self.buffer[start_idx..start_idx + actual_batch_size]
    }

    /// Publish batch - memory fence for visibility.
    pub fn publish_batch(&self, _start_seq: u64, _count: usize) {
        std::sync::atomic::fence(Ordering::Release);
    }

    fn get_minimum_consumer_sequence(&self) -> u64 {
        self._gating_sequence.sequence.load(Ordering::Relaxed)
    }

    fn update_gating_sequence(&self) {
        let min_seq = self.consumer_sequences.iter()
            .map(|cs| cs.sequence.load(Ordering::Relaxed))
            .min().unwrap_or(u64::MAX);
        self._gating_sequence.sequence.store(min_seq, Ordering::Relaxed);
    }

    /// Advance consumer sequence (for fire-and-forget patterns)
    /// This simulates immediate consumption/acknowledgment of messages.

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
        let _ring_buffer = MessageRingBuffer::new(config).unwrap();
        // Config validated by constructor
    }

    #[test]
    fn test_ring_buffer_operations() {
        let config = RingBufferConfig::new(1024).unwrap().with_consumers(1).unwrap();
        let ring_buffer = MessageRingBuffer::new(config).unwrap();
        
        // Test empty buffer returns empty batch
        let result = ring_buffer.try_consume_batch(0, 1);
        assert!(result.is_empty());
    }
}

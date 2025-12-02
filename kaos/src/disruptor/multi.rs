//! Multi-producer/consumer ring buffers.
//!
//! - `MpscRingBuffer<T>` - Multiple producers, single consumer
//! - `SpmcRingBuffer<T>` - Single producer, multiple consumers (work-stealing)
//! - `MpmcRingBuffer<T>` - Multiple producers, multiple consumers

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::marker::PhantomData;

use crate::disruptor::completion::{CompletionTracker, ReadGuard, BatchReadGuard, ReadableRing};
use crate::disruptor::RingBufferEntry;
use crate::error::{Result, KaosError};

// ============================================================================
// MPSC - Multi-Producer Single Consumer
// ============================================================================

pub struct MpscRingBuffer<T: RingBufferEntry> {
    buffer: Box<[T]>,
    mask: usize,
    consumer_cursor: Arc<AtomicU64>,
    claim_cursor: Arc<AtomicU64>,
    available: Box<[AtomicU64]>,
    index_mask: usize,
    index_shift: usize,
}

impl<T: RingBufferEntry> MpscRingBuffer<T> {
    pub fn new(size: usize) -> Result<Self> {
        if !size.is_power_of_two() { return Err(KaosError::config("Size must be power of 2")); }
        if size < 64 { return Err(KaosError::config("MPSC ring buffer must be at least 64 slots")); }

        let buffer = (0..size).map(|_| T::default()).collect::<Vec<_>>().into_boxed_slice();
        let u64_needed = size / 64;
        let available = (0..u64_needed).map(|_| AtomicU64::new(!0)).collect::<Vec<_>>().into_boxed_slice();

        Ok(Self {
            buffer, mask: size - 1, consumer_cursor: Arc::new(AtomicU64::new(0)),
            claim_cursor: Arc::new(AtomicU64::new(0)), available, index_mask: size - 1, index_shift: Self::log2(size),
        })
    }

    fn log2(i: usize) -> usize { std::mem::size_of::<usize>() * 8 - (i.leading_zeros() as usize) - 1 }

    pub fn try_claim(&self, count: usize) -> Option<u64> {
        loop {
            let current = self.claim_cursor.load(Ordering::Relaxed);
            let next = current + (count as u64);
            let consumer_seq = self.consumer_cursor.load(Ordering::Acquire);
            if (self.buffer.len() as u64) <= next - consumer_seq { return None; }

            match self.claim_cursor.compare_exchange_weak(current, next, Ordering::Acquire, Ordering::Relaxed) {
                Ok(_) => return Some(current),
                Err(_) => std::hint::spin_loop(),
            }
        }
    }

    pub unsafe fn write_slot(&self, sequence: u64, value: T) {
        let idx = (sequence as usize) & self.mask;
        std::ptr::write_volatile(self.buffer.as_ptr().add(idx) as *mut T, value);
    }

    fn calculate_indices(&self, sequence: u64) -> (usize, usize) {
        let slot_index = (sequence as usize) & self.index_mask;
        (slot_index >> 6, slot_index & 63)
    }

    fn calculate_flag(&self, sequence: u64) -> u64 { (sequence >> self.index_shift) & 1 }

    pub fn publish(&self, sequence: u64) {
        let (avail_idx, bit_idx) = self.calculate_indices(sequence);
        self.available[avail_idx].fetch_xor(1u64 << bit_idx, Ordering::Release);
    }

    pub unsafe fn read_slot(&self, sequence: u64) -> T {
        let idx = (sequence as usize) & self.mask;
        std::ptr::read_volatile(self.buffer.as_ptr().add(idx))
    }

    pub fn update_consumer(&self, sequence: u64) { self.consumer_cursor.store(sequence, Ordering::Release); }

    pub fn get_published_sequence(&self) -> u64 {
        let prev = self.consumer_cursor.load(Ordering::Relaxed);
        let claim_seq = self.claim_cursor.load(Ordering::Acquire);
        if prev >= claim_seq { return prev; }

        let mut availability_flag = self.calculate_flag(prev);
        let (mut availability_index, mut bit_index) = self.calculate_indices(prev);
        let mut availability = self.available[availability_index].load(Ordering::Acquire) >> bit_index;
        let mut highest_available = prev;

        loop {
            if (availability & 1) != availability_flag { return highest_available.saturating_sub(1); }
            highest_available += 1;
            if highest_available >= claim_seq { return claim_seq; }

            if bit_index < 63 {
                bit_index += 1;
                availability >>= 1;
            } else {
                (availability_index, _) = self.calculate_indices(highest_available);
                availability = self.available[availability_index].load(Ordering::Acquire);
                bit_index = 0;
                if availability_index == 0 { availability_flag ^= 1; }
            }
        }
    }
}

unsafe impl<T: RingBufferEntry> Send for MpscRingBuffer<T> {}
unsafe impl<T: RingBufferEntry> Sync for MpscRingBuffer<T> {}

pub struct MpscProducer<T: RingBufferEntry> {
    ring_buffer: Arc<MpscRingBuffer<T>>,
    _phantom: PhantomData<T>,
}

unsafe impl<T: RingBufferEntry> Send for MpscProducer<T> {}

impl<T: RingBufferEntry> MpscProducer<T> {
    pub fn new(ring_buffer: Arc<MpscRingBuffer<T>>) -> Self { Self { ring_buffer, _phantom: PhantomData } }

    pub fn publish<F>(&self, writer: F) -> std::result::Result<(), &'static str> where F: FnOnce(&mut T) {
        if let Some(seq) = self.ring_buffer.try_claim(1) {
            let mut value = T::default();
            writer(&mut value);
            unsafe { self.ring_buffer.write_slot(seq, value); }
            self.ring_buffer.publish(seq);
            Ok(())
        } else { Err("Ring buffer full") }
    }

    pub fn publish_batch<F>(&self, count: usize, mut writer: F) -> std::result::Result<usize, &'static str>
    where F: FnMut(usize, &mut T)
    {
        if let Some(start_seq) = self.ring_buffer.try_claim(count) {
            for i in 0..count {
                let seq = start_seq + (i as u64);
                let mut value = T::default();
                writer(i, &mut value);
                unsafe { self.ring_buffer.write_slot(seq, value); }
                self.ring_buffer.publish(seq);
            }
            Ok(count)
        } else { Err("Ring buffer full") }
    }
}

pub struct MpscProducerBuilder<T: RingBufferEntry> { ring_buffer: Option<Arc<MpscRingBuffer<T>>> }

impl<T: RingBufferEntry> MpscProducerBuilder<T> {
    pub fn new() -> Self { Self { ring_buffer: None } }
    pub fn with_ring_buffer(mut self, rb: Arc<MpscRingBuffer<T>>) -> Self { self.ring_buffer = Some(rb); self }
    pub fn build(self) -> std::result::Result<MpscProducer<T>, &'static str> {
        self.ring_buffer.map(MpscProducer::new).ok_or("Ring buffer not set")
    }
}

pub trait MpscEventHandler<T: RingBufferEntry> {
    fn on_event(&mut self, event: &T, seq: u64, end_of_batch: bool);
}

pub struct MpscConsumer<T: RingBufferEntry> {
    ring_buffer: Arc<MpscRingBuffer<T>>,
    cursor: u64,
    batch_size: usize,
    _phantom: PhantomData<T>,
}

impl<T: RingBufferEntry> MpscConsumer<T> {
    pub fn new(ring_buffer: Arc<MpscRingBuffer<T>>, batch_size: usize) -> Self {
        Self { ring_buffer, cursor: 0, batch_size, _phantom: PhantomData }
    }

    pub fn process_events<H: MpscEventHandler<T>>(&mut self, handler: &mut H) -> usize {
        let published_seq = self.ring_buffer.get_published_sequence();
        let available = published_seq.saturating_sub(self.cursor);
        if available == 0 { return 0; }

        let to_consume = available.min(self.batch_size as u64) as usize;
        for i in 0..to_consume {
            let seq = self.cursor + (i as u64);
            unsafe {
                let event = self.ring_buffer.read_slot(seq);
                handler.on_event(&event, seq, i == to_consume - 1);
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
    pub fn new() -> Self { Self { ring_buffer: None, batch_size: 8192 } }
    pub fn with_ring_buffer(mut self, rb: Arc<MpscRingBuffer<T>>) -> Self { self.ring_buffer = Some(rb); self }
    pub fn with_batch_size(mut self, size: usize) -> Self { self.batch_size = size; self }
    pub fn build(self) -> std::result::Result<MpscConsumer<T>, &'static str> {
        self.ring_buffer.map(|rb| MpscConsumer::new(rb, self.batch_size)).ok_or("Ring buffer not set")
    }
}

// ============================================================================
// SPMC - Single Producer Multiple Consumers (work-stealing)
// ============================================================================

pub struct SpmcRingBuffer<T: RingBufferEntry> {
    buffer: Box<[T]>,
    mask: usize,
    producer_cursor: Arc<AtomicU64>,
    completion_tracker: CompletionTracker,
}

impl<T: RingBufferEntry> SpmcRingBuffer<T> {
    pub fn new(size: usize) -> Result<Self> {
        if !size.is_power_of_two() { return Err(KaosError::config("Size must be power of 2")); }
        let buffer = (0..size).map(|_| T::default()).collect::<Vec<_>>().into_boxed_slice();
        Ok(Self { buffer, mask: size - 1, producer_cursor: Arc::new(AtomicU64::new(0)), completion_tracker: CompletionTracker::new() })
    }

    pub fn try_claim(&self, count: usize, current_cursor: u64) -> Option<u64> {
        let next = current_cursor + (count as u64);
        let consumer_seq = self.completion_tracker.completed_cursor();
        if (self.buffer.len() as u64) - (next - consumer_seq) > 0 { Some(next) } else { None }
    }

    pub unsafe fn write_slot(&self, sequence: u64, value: T) {
        let idx = (sequence as usize) & self.mask;
        std::ptr::write_volatile(self.buffer.as_ptr().add(idx) as *mut T, value);
    }

    pub fn publish(&self, sequence: u64) {
        std::sync::atomic::fence(Ordering::Release);
        self.producer_cursor.store(sequence, Ordering::Relaxed);
    }

    pub fn try_read(&self) -> Option<ReadGuard<'_, T, Self>> {
        let producer_seq = self.producer_cursor.load(Ordering::Relaxed);
        if let Some(sequence) = self.completion_tracker.try_claim(producer_seq) {
            std::sync::atomic::fence(Ordering::Acquire);
            Some(ReadGuard::new(self, sequence))
        } else { None }
    }

    pub fn try_read_batch(&self, max_count: usize) -> Option<BatchReadGuard<'_, T, Self>> {
        let producer_seq = self.producer_cursor.load(Ordering::Relaxed);
        if let Some((start, count)) = self.completion_tracker.try_claim_batch(max_count, producer_seq) {
            std::sync::atomic::fence(Ordering::Acquire);
            Some(BatchReadGuard::new(self, start, count))
        } else { None }
    }

    pub fn try_claim_read(&self) -> Option<u64> {
        let producer_seq = self.producer_cursor.load(Ordering::Relaxed);
        if let Some(sequence) = self.completion_tracker.try_claim(producer_seq) {
            std::sync::atomic::fence(Ordering::Acquire);
            Some(sequence)
        } else { None }
    }

    pub unsafe fn read_slot(&self, sequence: u64) -> T {
        let idx = (sequence as usize) & self.mask;
        std::ptr::read_volatile(self.buffer.as_ptr().add(idx))
    }

    pub fn complete_read(&self, sequence: u64) { self.completion_tracker.complete(sequence); }

    pub fn get_read_batch_fast(&self, consumer_cursor: u64, max_count: usize) -> &[T] {
        let producer_seq = self.producer_cursor.load(Ordering::Acquire);
        let available = producer_seq.saturating_sub(consumer_cursor) as usize;
        if available == 0 { return &[]; }

        let count = available.min(max_count);
        let start_idx = (consumer_cursor as usize) & self.mask;
        let end_idx = start_idx + count;

        if end_idx <= self.buffer.len() { &self.buffer[start_idx..end_idx] }
        else { &self.buffer[start_idx..start_idx + (self.buffer.len() - start_idx)] }
    }

    pub fn update_consumer_fast(&self, cursor: u64) { self.completion_tracker.set_completed_cursor(cursor); }
    pub fn producer_cursor(&self) -> Arc<AtomicU64> { self.producer_cursor.clone() }
    pub fn completed_cursor(&self) -> u64 { self.completion_tracker.completed_cursor() }
}

impl<T: RingBufferEntry> ReadableRing<T> for SpmcRingBuffer<T> {
    fn read_slot_ref(&self, sequence: u64) -> &T { &self.buffer[(sequence as usize) & self.mask] }
    fn complete_read(&self, sequence: u64) { self.completion_tracker.complete(sequence); }
    fn complete_read_batch(&self, start: u64, count: usize) { self.completion_tracker.complete_batch(start, count); }
}

unsafe impl<T: RingBufferEntry> Send for SpmcRingBuffer<T> {}
unsafe impl<T: RingBufferEntry> Sync for SpmcRingBuffer<T> {}

// ============================================================================
// MPMC - Multi-Producer Multi-Consumer
// ============================================================================

pub struct MpmcRingBuffer<T: RingBufferEntry> {
    buffer: Box<[T]>,
    mask: usize,
    producer_cursor: Arc<AtomicU64>,
    completion_tracker: CompletionTracker,
}

impl<T: RingBufferEntry> MpmcRingBuffer<T> {
    pub fn new(size: usize) -> Result<Self> {
        if !size.is_power_of_two() { return Err(KaosError::config("Size must be power of 2")); }
        let buffer = (0..size).map(|_| T::default()).collect::<Vec<_>>().into_boxed_slice();
        Ok(Self { buffer, mask: size - 1, producer_cursor: Arc::new(AtomicU64::new(0)), completion_tracker: CompletionTracker::new() })
    }

    pub fn try_claim(&self, count: usize) -> Option<u64> {
        let current = self.producer_cursor.load(Ordering::Relaxed);
        let next = current + (count as u64);
        let consumer_seq = self.completion_tracker.completed_cursor();
        if (self.buffer.len() as u64) - (next - consumer_seq) <= 0 { return None; }

        match self.producer_cursor.compare_exchange_weak(current, next, Ordering::Relaxed, Ordering::Relaxed) {
            Ok(_) => Some(current),
            Err(_) => None,
        }
    }

    pub unsafe fn write_slot(&self, sequence: u64, value: T) {
        let idx = (sequence as usize) & self.mask;
        std::ptr::write_volatile(self.buffer.as_ptr().add(idx) as *mut T, value);
    }

    pub fn publish(&self, _sequence: u64) { std::sync::atomic::fence(Ordering::Release); }

    pub fn try_read(&self) -> Option<ReadGuard<'_, T, Self>> {
        let producer_seq = self.producer_cursor.load(Ordering::Relaxed);
        if let Some(sequence) = self.completion_tracker.try_claim(producer_seq) {
            std::sync::atomic::fence(Ordering::Acquire);
            Some(ReadGuard::new(self, sequence))
        } else { None }
    }

    pub fn producer_cursor(&self) -> Arc<AtomicU64> { self.producer_cursor.clone() }
    pub fn completed_cursor(&self) -> u64 { self.completion_tracker.completed_cursor() }
}

impl<T: RingBufferEntry> ReadableRing<T> for MpmcRingBuffer<T> {
    fn read_slot_ref(&self, sequence: u64) -> &T { &self.buffer[(sequence as usize) & self.mask] }
    fn complete_read(&self, sequence: u64) { self.completion_tracker.complete(sequence); }
    fn complete_read_batch(&self, start: u64, count: usize) { self.completion_tracker.complete_batch(start, count); }
}

unsafe impl<T: RingBufferEntry> Send for MpmcRingBuffer<T> {}
unsafe impl<T: RingBufferEntry> Sync for MpmcRingBuffer<T> {}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::disruptor::Slot8;
    use std::thread;

    #[test]
    fn test_spmc_basic() {
        let ring = SpmcRingBuffer::<Slot8>::new(1024).unwrap();
        let next = ring.try_claim(1, 0).unwrap();
        unsafe { ring.write_slot(0, Slot8 { value: 42 }); }
        ring.publish(next);

        let guard = ring.try_read().unwrap();
        assert_eq!(guard.get().value, 42);
        drop(guard);
        assert_eq!(ring.completed_cursor(), 1);
    }

    #[test]
    fn test_spmc_batch() {
        let ring = SpmcRingBuffer::<Slot8>::new(1024).unwrap();
        let mut cursor = 0u64;
        for i in 0..10 {
            let next = ring.try_claim(1, cursor).unwrap();
            unsafe { ring.write_slot(cursor, Slot8 { value: i + 1 }); }
            ring.publish(next);
            cursor = next;
        }

        let batch = ring.try_read_batch(5).unwrap();
        assert_eq!(batch.count(), 5);
        drop(batch);
        assert_eq!(ring.completed_cursor(), 5);
    }

    #[test]
    fn test_mpmc_basic() {
        let ring = MpmcRingBuffer::<Slot8>::new(1024).unwrap();
        let seq = ring.try_claim(1).unwrap();
        unsafe { ring.write_slot(seq, Slot8 { value: 42 }); }
        ring.publish(seq);

        let guard = ring.try_read().unwrap();
        assert_eq!(guard.get().value, 42);
        drop(guard);
        assert_eq!(ring.completed_cursor(), 1);
    }

    #[test]
    fn test_spmc_multi_threaded() {
        let ring = Arc::new(SpmcRingBuffer::<Slot8>::new(1024).unwrap());
        let num_items = 1000u64;
        let num_consumers = 4;

        let ring_producer = ring.clone();
        let producer = thread::spawn(move || {
            let mut cursor = 0u64;
            for i in 1..=num_items {
                loop {
                    if let Some(next) = ring_producer.try_claim(1, cursor) {
                        unsafe { ring_producer.write_slot(cursor, Slot8 { value: i }); }
                        ring_producer.publish(next);
                        cursor = next;
                        break;
                    }
                    std::hint::spin_loop();
                }
            }
            for _ in 0..num_consumers {
                loop {
                    if let Some(next) = ring_producer.try_claim(1, cursor) {
                        unsafe { ring_producer.write_slot(cursor, Slot8 { value: 0 }); }
                        ring_producer.publish(next);
                        cursor = next;
                        break;
                    }
                    std::hint::spin_loop();
                }
            }
        });

        let mut consumers = vec![];
        for _ in 0..num_consumers {
            let ring_consumer = ring.clone();
            consumers.push(thread::spawn(move || {
                let mut sum = 0u64;
                let mut count = 0u64;
                loop {
                    if let Some(guard) = ring_consumer.try_read() {
                        let value = guard.get().value;
                        if value == 0 { break; }
                        sum += value;
                        count += 1;
                    } else { std::hint::spin_loop(); }
                }
                (sum, count)
            }));
        }

        producer.join().unwrap();
        let mut total_sum = 0u64;
        let mut total_count = 0u64;
        for consumer in consumers {
            let (sum, count) = consumer.join().unwrap();
            total_sum += sum;
            total_count += count;
        }
        assert_eq!(total_sum, (num_items * (num_items + 1)) / 2);
        assert_eq!(total_count, num_items);
    }
}

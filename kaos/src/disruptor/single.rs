//! RingBuffer - High performance SPSC ring buffers
//!
//! - `RingBuffer<T>` - Single consumer, fastest
//! - `BroadcastRingBuffer<T>` - Multiple consumers see ALL messages (fan-out)
//!
//! Constructors:
//! - `new()` - Heap allocation
//! - `new_mapped()` - Memory-mapped with mlock (faster, no page faults)
//! - `new_broadcast()` - Multiple consumer broadcast pattern

use crate::disruptor::{RingBufferConfig, RingBufferEntry};
use crate::error::{KaosError, Result};
use std::marker::PhantomData;
use std::ptr;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

// ============================================================================
// Shared: Cache-line padded atomic (prevents false sharing)
// ============================================================================

#[repr(align(128))]
struct PaddedAtomicU64(AtomicU64);

impl PaddedAtomicU64 {
    fn new(v: u64) -> Self {
        Self(AtomicU64::new(v))
    }
    fn load(&self, order: Ordering) -> u64 {
        self.0.load(order)
    }
    fn store(&self, v: u64, order: Ordering) {
        self.0.store(v, order)
    }
    fn compare_exchange_weak(
        &self,
        current: u64,
        new: u64,
        success: Ordering,
        failure: Ordering,
    ) -> std::result::Result<u64, u64> {
        self.0.compare_exchange_weak(current, new, success, failure)
    }
}

// ============================================================================
// RingBuffer<T> - Simple SPSC (Single Producer, Single Consumer)
// ============================================================================

pub struct RingBuffer<T: RingBufferEntry> {
    buffer: *mut T,
    size: usize,
    mask: usize,
    producer_cursor: Arc<AtomicU64>,
    consumer_cursor: Arc<AtomicU64>,
    _heap: Option<Box<[T]>>,
    is_mapped: bool,
}

impl<T: RingBufferEntry> RingBuffer<T> {
    /// Create with heap allocation
    pub fn new(size: usize) -> Result<Self> {
        if !size.is_power_of_two() {
            return Err(KaosError::config("Size must be power of 2"));
        }

        let buffer: Box<[T]> = (0..size)
            .map(|_| T::default())
            .collect::<Vec<_>>()
            .into_boxed_slice();
        let ptr = buffer.as_ptr() as *mut T;

        Ok(Self {
            buffer: ptr,
            size,
            mask: size - 1,
            producer_cursor: Arc::new(AtomicU64::new(0)),
            consumer_cursor: Arc::new(AtomicU64::new(0)),
            _heap: Some(buffer),
            is_mapped: false,
        })
    }

    /// Create with memory-mapped allocation (mmap + mlock)
    pub fn new_mapped(size: usize) -> Result<Self> {
        if !size.is_power_of_two() {
            return Err(KaosError::config("Size must be power of 2"));
        }

        let buffer_size = size
            .checked_mul(std::mem::size_of::<T>())
            .ok_or_else(|| KaosError::config("Buffer size overflow"))?;
        let ptr = unsafe {
            let p = libc::mmap(
                ptr::null_mut(),
                buffer_size,
                libc::PROT_READ | libc::PROT_WRITE,
                libc::MAP_PRIVATE | libc::MAP_ANONYMOUS,
                -1,
                0,
            );
            if p == libc::MAP_FAILED {
                return Err(KaosError::config("mmap failed"));
            }
            let _ = libc::mlock(p, buffer_size);
            std::ptr::write_bytes(p as *mut u8, 0, buffer_size);
            p as *mut T
        };

        Ok(Self {
            buffer: ptr,
            size,
            mask: size - 1,
            producer_cursor: Arc::new(AtomicU64::new(0)),
            consumer_cursor: Arc::new(AtomicU64::new(0)),
            _heap: None,
            is_mapped: true,
        })
    }

    /// Create a broadcast ring buffer (multiple consumers see ALL messages)
    pub fn new_broadcast(config: RingBufferConfig) -> Result<BroadcastRingBuffer<T>> {
        BroadcastRingBuffer::new(config)
    }

    pub fn consumer_cursor(&self) -> Arc<AtomicU64> {
        self.consumer_cursor.clone()
    }
    pub fn producer_cursor(&self) -> Arc<AtomicU64> {
        self.producer_cursor.clone()
    }

    pub fn try_claim(&mut self, count: usize, local_cursor: u64) -> Option<u64> {
        let next = local_cursor + (count as u64);
        let consumer_seq = self.consumer_cursor.load(Ordering::Relaxed);
        if next.wrapping_sub(consumer_seq) < (self.size as u64) {
            Some(next)
        } else {
            None
        }
    }

    pub fn try_claim_slots(&self, count: usize, local_cursor: u64) -> Option<(u64, &mut [T])> {
        if count == 0 {
            return None;
        }
        let next = local_cursor + (count as u64);
        let consumer_seq = self.consumer_cursor.load(Ordering::Relaxed);
        if next.wrapping_sub(consumer_seq) < (self.size as u64) {
            let start_idx = (local_cursor as usize) & self.mask;
            let end_idx = start_idx + count;
            debug_assert!(
                start_idx < self.size,
                "try_claim_slots: start {} >= size {}",
                start_idx,
                self.size
            );
            let slots = if end_idx <= self.size {
                unsafe { std::slice::from_raw_parts_mut(self.buffer.add(start_idx), count) }
            } else {
                let len = self.size - start_idx;
                debug_assert!(
                    len <= self.size,
                    "try_claim_slots: len {} > size {}",
                    len,
                    self.size
                );
                unsafe { std::slice::from_raw_parts_mut(self.buffer.add(start_idx), len) }
            };
            Some((local_cursor, slots))
        } else {
            None
        }
    }

    /// Publish single event with in-place mutation.
    /// Returns the sequence number, or None if full.
    #[inline]
    pub fn try_publish_with<F>(&self, local_cursor: u64, update: F) -> Option<u64>
    where
        F: FnOnce(&mut T),
    {
        let next = local_cursor + 1;
        let consumer_seq = self.consumer_cursor.load(Ordering::Relaxed);
        if next.wrapping_sub(consumer_seq) < (self.size as u64) {
            let idx = (local_cursor as usize) & self.mask;
            let slot = unsafe { &mut *self.buffer.add(idx) };
            update(slot);
            self.publish(next);
            Some(local_cursor)
        } else {
            None
        }
    }

    /// Publish batch with in-place mutation (zero-copy writes).
    /// Returns (start_sequence, count), or None if not enough space.
    #[inline]
    pub fn try_publish_batch_with<F>(
        &self,
        local_cursor: u64,
        count: usize,
        update: F,
    ) -> Option<(u64, usize)>
    where
        F: FnMut(&mut T, u64), // (slot, sequence)
    {
        if count == 0 {
            return Some((local_cursor, 0));
        }
        let next = local_cursor + (count as u64);
        let consumer_seq = self.consumer_cursor.load(Ordering::Relaxed);
        if next.wrapping_sub(consumer_seq) < (self.size as u64) {
            let start_idx = (local_cursor as usize) & self.mask;
            let available = if start_idx + count <= self.size {
                count
            } else {
                self.size - start_idx
            };

            let mut update = update;
            for i in 0..available {
                let slot = unsafe { &mut *self.buffer.add(start_idx + i) };
                update(slot, local_cursor + (i as u64));
            }

            self.publish(local_cursor + (available as u64));
            Some((local_cursor, available))
        } else {
            None
        }
    }

    /// Get a batch of slots for reading. Safe path includes bounds check.
    #[cfg(feature = "unsafe-perf")]
    #[inline(always)]
    pub fn get_read_batch(&self, cursor: u64, count: usize) -> &[T] {
        let start_idx = (cursor as usize) & self.mask;
        let actual = count.min(self.size - start_idx);
        unsafe { std::slice::from_raw_parts(self.buffer.add(start_idx), actual) }
    }

    /// Get a batch of slots for reading. Safe path includes bounds check.
    #[cfg(not(feature = "unsafe-perf"))]
    #[inline(always)]
    pub fn get_read_batch(&self, cursor: u64, count: usize) -> &[T] {
        let start_idx = (cursor as usize) & self.mask;
        let actual = count.min(self.size - start_idx);
        debug_assert!(
            start_idx + actual <= self.size,
            "get_read_batch: end {} > size {}",
            start_idx + actual,
            self.size
        );
        unsafe { std::slice::from_raw_parts(self.buffer.add(start_idx), actual) }
    }

    /// Write a value to a slot. Safe path includes bounds check.
    #[cfg(feature = "unsafe-perf")]
    #[inline(always)]
    pub unsafe fn write_slot(&self, sequence: u64, value: T) {
        let idx = (sequence as usize) & self.mask;
        std::ptr::write_volatile(self.buffer.add(idx), value);
    }

    /// Write a value to a slot. Safe path includes bounds check.
    #[cfg(not(feature = "unsafe-perf"))]
    #[inline(always)]
    pub unsafe fn write_slot(&self, sequence: u64, value: T) {
        let idx = (sequence as usize) & self.mask;
        debug_assert!(
            idx < self.size,
            "write_slot: idx {} >= size {}",
            idx,
            self.size
        );
        std::ptr::write_volatile(self.buffer.add(idx), value);
    }

    pub fn publish(&self, sequence: u64) {
        std::sync::atomic::fence(Ordering::Release);
        self.producer_cursor.store(sequence, Ordering::Relaxed);
    }

    /// Read a value from a slot. Safe path includes bounds check.
    #[cfg(feature = "unsafe-perf")]
    #[inline(always)]
    pub unsafe fn read_slot(&self, sequence: u64) -> T {
        let idx = (sequence as usize) & self.mask;
        std::ptr::read_volatile(self.buffer.add(idx))
    }

    /// Read a value from a slot. Safe path includes bounds check.
    #[cfg(not(feature = "unsafe-perf"))]
    #[inline(always)]
    pub unsafe fn read_slot(&self, sequence: u64) -> T {
        let idx = (sequence as usize) & self.mask;
        debug_assert!(
            idx < self.size,
            "read_slot: idx {} >= size {}",
            idx,
            self.size
        );
        std::ptr::read_volatile(self.buffer.add(idx))
    }

    pub fn update_consumer(&self, sequence: u64) {
        self.consumer_cursor.store(sequence, Ordering::Relaxed);
    }
}

impl<T: RingBufferEntry> Drop for RingBuffer<T> {
    fn drop(&mut self) {
        if self.is_mapped && !self.buffer.is_null() {
            unsafe {
                libc::munmap(
                    self.buffer as *mut libc::c_void,
                    self.size * std::mem::size_of::<T>(),
                );
            }
        }
    }
}

unsafe impl<T: RingBufferEntry> Send for RingBuffer<T> {}
unsafe impl<T: RingBufferEntry> Sync for RingBuffer<T> {}

// ============================================================================
// FastProducer<T> - Per-event producer with cached consumer check
// ============================================================================

/// Producer that caches consumer position to reduce atomic loads.
pub struct FastProducer<T: RingBufferEntry> {
    ring: Arc<RingBuffer<T>>,
    sequence: u64,
    sequence_clear_of_consumers: u64,
}

impl<T: RingBufferEntry> FastProducer<T> {
    pub fn new(ring: Arc<RingBuffer<T>>) -> Self {
        let size = ring.size as u64;
        Self {
            ring,
            sequence: 0,
            sequence_clear_of_consumers: size - 1,
        }
    }

    #[inline]
    pub fn sequence(&self) -> u64 {
        self.sequence
    }

    /// Try to publish. Returns sequence on success, None if full.
    #[inline]
    pub fn try_publish<F>(&mut self, update: F) -> Option<u64>
    where
        F: FnOnce(&mut T),
    {
        let seq = self.sequence;

        if seq > self.sequence_clear_of_consumers {
            let consumer_seq = self.ring.consumer_cursor.load(Ordering::Acquire);
            let free_slots = (self.ring.size as u64).wrapping_sub(seq.wrapping_sub(consumer_seq));

            if free_slots == 0 {
                return None;
            }

            self.sequence_clear_of_consumers = consumer_seq + (self.ring.size as u64) - 1;
        }

        let idx = (seq as usize) & self.ring.mask;
        let slot = unsafe { &mut *self.ring.buffer.add(idx) };
        update(slot);

        self.sequence += 1;
        self.ring
            .producer_cursor
            .store(self.sequence, Ordering::Release);

        Some(seq)
    }

    /// Publish, spinning until space is available.
    #[inline]
    pub fn publish<F>(&mut self, mut update: F)
    where
        F: FnMut(&mut T),
    {
        loop {
            let seq = self.sequence;

            if seq <= self.sequence_clear_of_consumers {
                let idx = (seq as usize) & self.ring.mask;
                let slot = unsafe { &mut *self.ring.buffer.add(idx) };
                update(slot);
                self.sequence += 1;
                self.ring
                    .producer_cursor
                    .store(self.sequence, Ordering::Release);
                return;
            }

            let consumer_seq = self.ring.consumer_cursor.load(Ordering::Acquire);
            let free_slots = (self.ring.size as u64).wrapping_sub(seq.wrapping_sub(consumer_seq));

            if free_slots > 0 {
                self.sequence_clear_of_consumers = consumer_seq + (self.ring.size as u64) - 1;
                let idx = (seq as usize) & self.ring.mask;
                let slot = unsafe { &mut *self.ring.buffer.add(idx) };
                update(slot);
                self.sequence += 1;
                self.ring
                    .producer_cursor
                    .store(self.sequence, Ordering::Release);
                return;
            }

            std::hint::spin_loop();
        }
    }

    /// Try to publish batch. Returns (start_seq, count) on success.
    #[inline]
    pub fn try_publish_batch<F>(&mut self, count: usize, mut update: F) -> Option<(u64, usize)>
    where
        F: FnMut(&mut T, u64),
    {
        if count == 0 {
            return Some((self.sequence, 0));
        }

        let start_seq = self.sequence;
        let end_seq = start_seq + (count as u64) - 1;

        // Check if we have enough space
        if end_seq > self.sequence_clear_of_consumers {
            let consumer_seq = self.ring.consumer_cursor.load(Ordering::Acquire);
            let free_slots =
                (self.ring.size as u64).wrapping_sub(start_seq.wrapping_sub(consumer_seq));

            if free_slots < (count as u64) {
                return None;
            }

            self.sequence_clear_of_consumers = consumer_seq + (self.ring.size as u64) - 1;
        }

        // Write all slots
        let start_idx = (start_seq as usize) & self.ring.mask;
        let available_to_end = self.ring.size - start_idx;
        let actual = count.min(available_to_end);

        for i in 0..actual {
            let slot = unsafe { &mut *self.ring.buffer.add(start_idx + i) };
            update(slot, start_seq + (i as u64));
        }

        // Publish
        self.sequence = start_seq + (actual as u64);
        self.ring
            .producer_cursor
            .store(self.sequence, Ordering::Release);

        Some((start_seq, actual))
    }
}

// ============================================================================
// BroadcastRingBuffer<T> - Multiple consumers see ALL messages
// ============================================================================

pub struct BroadcastRingBuffer<T: RingBufferEntry> {
    config: RingBufferConfig,
    buffer: Box<[T]>,
    mask: usize,
    producer_sequence: PaddedAtomicU64,
    consumer_sequences: Vec<PaddedAtomicU64>,
    gating_sequence: PaddedAtomicU64,
}

unsafe impl<T: RingBufferEntry> Send for BroadcastRingBuffer<T> {}
unsafe impl<T: RingBufferEntry> Sync for BroadcastRingBuffer<T> {}

impl<T: RingBufferEntry> BroadcastRingBuffer<T> {
    pub fn new(config: RingBufferConfig) -> Result<Self> {
        if config.size == 0 || !config.size.is_power_of_two() {
            return Err(KaosError::config("Ring buffer size must be a power of 2"));
        }

        let mask = config.size - 1;
        let buffer = (0..config.size)
            .map(|_| T::default())
            .collect::<Vec<_>>()
            .into_boxed_slice();
        let consumer_sequences = (0..config.num_consumers)
            .map(|_| PaddedAtomicU64::new(u64::MAX))
            .collect();

        Ok(Self {
            config,
            buffer,
            mask,
            producer_sequence: PaddedAtomicU64::new(u64::MAX),
            consumer_sequences,
            gating_sequence: PaddedAtomicU64::new(u64::MAX),
        })
    }

    pub fn try_claim_slots(&mut self, count: usize) -> Option<(u64, &mut [T])> {
        if count == 0 {
            return None;
        }

        let current = self.producer_sequence.load(Ordering::Relaxed);
        let (slot_seq, next) = if current == u64::MAX {
            (0, (count as u64) - 1)
        } else {
            (current + 1, current + (count as u64))
        };

        let min_consumer = self.gating_sequence.load(Ordering::Relaxed);
        if min_consumer == u64::MAX {
            if next > (self.config.size as u64) - 1 {
                return None;
            }
        } else if next > min_consumer + (self.config.size as u64) {
            return None;
        }

        match self.producer_sequence.compare_exchange_weak(
            current,
            next,
            Ordering::AcqRel,
            Ordering::Relaxed,
        ) {
            Ok(_) => {
                let start = (slot_seq as usize) & self.mask;
                let end = ((slot_seq + (count as u64)) as usize) & self.mask;
                let slots = if start < end {
                    &mut self.buffer[start..end]
                } else {
                    let available = self.config.size - start;
                    &mut self.buffer[start..start + count.min(available)]
                };
                Some((slot_seq, slots))
            }
            Err(_) => None,
        }
    }

    pub fn try_claim_slots_relaxed(&mut self, count: usize) -> Option<(u64, &mut [T])> {
        let current = self.producer_sequence.load(Ordering::Relaxed);
        let next = current.wrapping_add(count as u64);

        let mut min_consumer = self.gating_sequence.load(Ordering::Relaxed);
        if min_consumer == u64::MAX {
            if next > (self.config.size as u64) {
                return None;
            }
        } else if next.wrapping_sub(min_consumer) > (self.config.size as u64) {
            self.update_gating_sequence();
            std::sync::atomic::fence(Ordering::Acquire);
            min_consumer = self.gating_sequence.load(Ordering::Relaxed);
            if next.wrapping_sub(min_consumer) > (self.config.size as u64) {
                return None;
            }
        }

        let start_seq = current.wrapping_add(1);
        let start_idx = (start_seq as usize) & self.mask;
        let available = self.config.size - start_idx;
        let slots = &mut self.buffer[start_idx..start_idx + count.min(available)];
        Some((start_seq, slots))
    }

    pub fn publish_batch_relaxed(&self, _start: u64, end: u64) {
        self.producer_sequence.store(end, Ordering::Release);
    }

    pub fn try_consume_batch(&self, consumer_id: usize, max_count: usize) -> Vec<&T> {
        if consumer_id >= self.consumer_sequences.len() {
            return Vec::new();
        }

        let current = self.consumer_sequences[consumer_id].load(Ordering::Relaxed);
        let producer = self.producer_sequence.load(Ordering::Relaxed);

        let (first, available) = if current == u64::MAX {
            if producer == u64::MAX {
                return Vec::new();
            }
            (0u64, producer + 1)
        } else {
            let next = current + 1;
            if next > producer {
                return Vec::new();
            }
            (next, producer - current)
        };

        if available == 0 {
            return Vec::new();
        }

        let count = available.min(max_count as u64) as usize;
        let mut messages = Vec::with_capacity(count);

        for i in 0..count {
            let seq = first + (i as u64);
            let slot = &self.buffer[(seq as usize) & self.mask];
            if slot.sequence() != seq {
                break;
            }
            messages.push(slot);
        }

        if !messages.is_empty() {
            let new_seq = first + (messages.len() as u64) - 1;
            self.consumer_sequences[consumer_id].store(new_seq, Ordering::Relaxed);
            if new_seq % 1000 < (messages.len() as u64) {
                self.update_gating_sequence();
            }
        }
        messages
    }

    pub fn try_consume_batch_relaxed(&self, consumer_id: usize, count: usize) -> &[T] {
        let current = self.consumer_sequences[consumer_id].load(Ordering::Relaxed);
        let producer = self.producer_sequence.load(Ordering::Relaxed);

        // Both uninitialized - nothing to consume
        if producer == u64::MAX {
            return &[];
        }

        // Calculate available: if current is MAX, we start from 0
        // producer = last published seq, so available = producer - last_consumed
        let (start_seq, available) = if current == u64::MAX {
            // First consume: sequences 0..producer are available (producer+1 items)
            (0u64, producer + 1)
        } else {
            // Normal: sequences current+1..producer are available
            let avail = producer.saturating_sub(current);
            if avail == 0 {
                return &[];
            }
            (current + 1, avail)
        };

        let start_idx = (start_seq as usize) & self.mask;
        let available_to_end = self.config.size - start_idx;
        let actual = count.min(available as usize).min(available_to_end);
        if actual == 0 {
            return &[];
        }

        let new_seq = start_seq + (actual as u64) - 1;
        self.consumer_sequences[consumer_id].store(new_seq, Ordering::Relaxed);
        if new_seq % 1000 < (actual as u64) {
            self.update_gating_sequence();
        }
        &self.buffer[start_idx..start_idx + actual]
    }

    pub fn publish_batch(&self, _start: u64, _count: usize) {
        std::sync::atomic::fence(Ordering::Release);
    }

    pub fn peek_batch(&self, consumer_id: usize, max_count: usize) -> Vec<&T> {
        if consumer_id >= self.consumer_sequences.len() {
            return Vec::new();
        }

        let current = self.consumer_sequences[consumer_id].load(Ordering::Relaxed);
        let producer = self.producer_sequence.load(Ordering::Acquire);

        let available = if current == u64::MAX {
            producer
        } else {
            producer.saturating_sub(current)
        };
        if available == 0 {
            return Vec::new();
        }

        let count = available.min(max_count as u64) as usize;
        let mut messages = Vec::with_capacity(count);

        for i in 0..count {
            let seq = if current == u64::MAX {
                i as u64
            } else {
                current + 1 + (i as u64)
            };
            let slot = &self.buffer[(seq as usize) & self.mask];
            if slot.sequence() != seq {
                break;
            }
            messages.push(slot);
        }
        messages
    }

    fn update_gating_sequence(&self) {
        let min = self
            .consumer_sequences
            .iter()
            .map(|cs| cs.load(Ordering::Relaxed))
            .min()
            .unwrap_or(u64::MAX);
        self.gating_sequence.store(min, Ordering::Relaxed);
    }

    pub fn advance_consumer(&self, consumer_id: usize, seq: u64) {
        if consumer_id < self.consumer_sequences.len() {
            self.consumer_sequences[consumer_id].store(seq, Ordering::Release);
            self.update_gating_sequence();
        }
    }
}

// Type alias for backwards compatibility
pub type MessageRingBuffer = BroadcastRingBuffer<crate::disruptor::MessageSlot>;

// ============================================================================
// Producer/Consumer/EventHandler - Generic wrappers
// ============================================================================

pub struct Producer<T: RingBufferEntry> {
    /// Raw pointer for macro access (publish_unrolled!)
    pub ring_buffer: *mut BroadcastRingBuffer<T>,
    _arc: Arc<BroadcastRingBuffer<T>>,
}

unsafe impl<T: RingBufferEntry> Send for Producer<T> {}

impl<T: RingBufferEntry> Producer<T> {
    pub fn new(ring_buffer: Arc<BroadcastRingBuffer<T>>) -> Self {
        let ptr = Arc::as_ptr(&ring_buffer) as *mut BroadcastRingBuffer<T>;
        Self {
            ring_buffer: ptr,
            _arc: ring_buffer,
        }
    }

    pub fn publish_batch<I, F>(
        &mut self,
        items: &[I],
        writer: F,
    ) -> std::result::Result<usize, &'static str>
    where
        F: Fn(&mut T, u64, &I),
    {
        let rb = unsafe { &mut *self.ring_buffer };
        if let Some((seq, slots)) = rb.try_claim_slots_relaxed(items.len()) {
            let count = slots.len();
            for (i, slot) in slots.iter_mut().enumerate() {
                writer(slot, seq + (i as u64), &items[i]);
            }
            rb.publish_batch_relaxed(seq, seq + (count as u64) - 1);
            Ok(count)
        } else {
            Err("Ring buffer full")
        }
    }
}

pub struct ProducerBuilder<T: RingBufferEntry> {
    ring_buffer: Option<Arc<BroadcastRingBuffer<T>>>,
    _phantom: PhantomData<T>,
}

impl<T: RingBufferEntry> ProducerBuilder<T> {
    pub fn new() -> Self {
        Self {
            ring_buffer: None,
            _phantom: PhantomData,
        }
    }
    pub fn with_ring_buffer(mut self, rb: Arc<BroadcastRingBuffer<T>>) -> Self {
        self.ring_buffer = Some(rb);
        self
    }
    pub fn build(self) -> std::result::Result<Producer<T>, &'static str> {
        self.ring_buffer
            .map(Producer::new)
            .ok_or("Ring buffer required")
    }
}

pub trait EventHandler<T: RingBufferEntry>: Send {
    fn on_event(&mut self, event: &T, sequence: u64, end_of_batch: bool);
}

pub struct Consumer<T: RingBufferEntry> {
    ring_buffer: Arc<BroadcastRingBuffer<T>>,
    consumer_id: usize,
    batch_size: usize,
}

impl<T: RingBufferEntry> Consumer<T> {
    pub fn new(ring_buffer: Arc<BroadcastRingBuffer<T>>, consumer_id: usize) -> Self {
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

    pub fn process_events<H: EventHandler<T>>(&self, handler: &mut H) -> usize {
        let rb = unsafe { &*Arc::as_ptr(&self.ring_buffer) };
        let batch = rb.try_consume_batch_relaxed(self.consumer_id, self.batch_size);
        if batch.is_empty() {
            return 0;
        }

        let count = batch.len();
        for (i, event) in batch.iter().enumerate() {
            handler.on_event(event, event.sequence(), i == count - 1);
        }
        count
    }

    pub fn run_loop<H: EventHandler<T>>(
        &self,
        handler: &mut H,
        stop_flag: &std::sync::atomic::AtomicBool,
    ) {
        while !stop_flag.load(Ordering::Relaxed) {
            if self.process_events(handler) == 0 {
                std::hint::spin_loop();
            }
        }
        while self.process_events(handler) > 0 {}
    }
}

pub struct ConsumerBuilder<T: RingBufferEntry> {
    ring_buffer: Option<Arc<BroadcastRingBuffer<T>>>,
    consumer_id: usize,
    batch_size: usize,
    _phantom: PhantomData<T>,
}

impl<T: RingBufferEntry> ConsumerBuilder<T> {
    pub fn new() -> Self {
        Self {
            ring_buffer: None,
            consumer_id: 0,
            batch_size: 2048,
            _phantom: PhantomData,
        }
    }
    pub fn with_ring_buffer(mut self, rb: Arc<BroadcastRingBuffer<T>>) -> Self {
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
    pub fn build(self) -> std::result::Result<Consumer<T>, &'static str> {
        self.ring_buffer
            .map(|rb| Consumer::new(rb, self.consumer_id).with_batch_size(self.batch_size))
            .ok_or("Ring buffer required")
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::disruptor::{MessageSlot, Slot8};

    #[test]
    fn test_spsc_heap() {
        let ring = RingBuffer::<Slot8>::new(1024).unwrap();
        assert_eq!(ring.producer_cursor().load(Ordering::Relaxed), 0);
    }

    #[test]
    fn test_spsc_mapped() {
        let ring = RingBuffer::<Slot8>::new_mapped(1024).unwrap();
        assert_eq!(ring.producer_cursor().load(Ordering::Relaxed), 0);
    }

    #[test]
    fn test_spsc_write_read() {
        let mut ring = RingBuffer::<Slot8>::new(1024).unwrap();
        if let Some(next) = ring.try_claim(3, 0) {
            unsafe {
                ring.write_slot(0, Slot8 { value: 1 });
                ring.write_slot(1, Slot8 { value: 2 });
                ring.write_slot(2, Slot8 { value: 3 });
            }
            ring.publish(next);
            std::sync::atomic::fence(Ordering::Acquire);
            unsafe {
                assert_eq!(ring.read_slot(0).value, 1);
                assert_eq!(ring.read_slot(1).value, 2);
                assert_eq!(ring.read_slot(2).value, 3);
            }
        }
    }

    #[test]
    fn test_broadcast_creation() {
        let config = RingBufferConfig::new(1024)
            .unwrap()
            .with_consumers(2)
            .unwrap();
        let _ring = BroadcastRingBuffer::<MessageSlot>::new(config).unwrap();
    }

    #[test]
    fn test_broadcast_generic_slot8() {
        let config = RingBufferConfig::new(1024)
            .unwrap()
            .with_consumers(1)
            .unwrap();
        let ring = BroadcastRingBuffer::<Slot8>::new(config).unwrap();
        assert!(ring.try_consume_batch(0, 1).is_empty());
    }

    #[test]
    fn test_new_broadcast_constructor() {
        let config = RingBufferConfig::new(1024)
            .unwrap()
            .with_consumers(2)
            .unwrap();
        let _ring = RingBuffer::<Slot8>::new_broadcast(config).unwrap();
    }

    #[test]
    fn test_invalid_size() {
        assert!(RingBuffer::<Slot8>::new(1000).is_err());
        assert!(RingBuffer::<Slot8>::new_mapped(1000).is_err());
    }
}

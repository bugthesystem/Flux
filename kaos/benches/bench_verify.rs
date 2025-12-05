//! Data Integrity Verification benchmarks
//!
//! Tests throughput while verifying no data loss:
//! - Sum verification
//! - Sequence verification
//! - XOR checksum verification
//! - MessageSlot variable-length data
//!
//! Run: cargo bench --bench bench_verify

use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use std::hint::black_box;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::thread;

use kaos::disruptor::{
    MessageRingBuffer, MessageSlot, RingBuffer, RingBufferConfig, RingBufferEntry, Slot8,
};

const RING_SIZE: usize = 1024 * 1024;
const BATCH_SIZE: usize = 8192;
const TOTAL_EVENTS: u64 = 10_000_000;
const MSG_EVENTS: u64 = 1_000_000;

// ============================================================================
// Sum Verification
// ============================================================================

fn bench_sum_verify(events: u64) -> u64 {
    let ring = Arc::new(RingBuffer::<Slot8>::new(RING_SIZE).unwrap());
    let producer_cursor = ring.producer_cursor();
    let consumer_sum = Arc::new(AtomicU64::new(0));

    let expected_sum: u64 = (0..events).map(|i| i % 100).sum();

    let ring_cons = ring.clone();
    let sum = consumer_sum.clone();
    let consumer = thread::spawn(move || {
        let mut cursor = 0u64;
        let mut local_sum = 0u64;

        while cursor < events {
            let prod_seq = producer_cursor.load(Ordering::Acquire);
            let available = prod_seq.saturating_sub(cursor);
            if available > 0 {
                let batch = (available as usize).min(BATCH_SIZE);
                let slots = ring_cons.get_read_batch(cursor, batch);
                for slot in slots {
                    local_sum += slot.value;
                }
                cursor += slots.len() as u64;
                ring_cons.update_consumer(cursor);
            } else {
                std::hint::spin_loop();
            }
        }
        sum.store(local_sum, Ordering::Release);
    });

    let ring_prod = ring.clone();
    let mut cursor = 0u64;
    while cursor < events {
        let remaining = (events - cursor) as usize;
        let batch = remaining.min(BATCH_SIZE);
        if let Some((seq, slots)) = ring_prod.try_claim_slots(batch, cursor) {
            for (i, slot) in slots.iter_mut().enumerate() {
                slot.value = (cursor + i as u64) % 100;
            }
            ring_prod.publish(seq + slots.len() as u64);
            cursor = seq + slots.len() as u64;
        }
    }

    consumer.join().unwrap();

    let actual_sum = consumer_sum.load(Ordering::Acquire);
    assert_eq!(actual_sum, expected_sum, "Sum verification failed!");
    events
}

// ============================================================================
// Sequence Verification
// ============================================================================

fn bench_sequence_verify(events: u64) -> u64 {
    let ring = Arc::new(RingBuffer::<Slot8>::new(RING_SIZE).unwrap());
    let producer_cursor = ring.producer_cursor();
    let errors = Arc::new(AtomicU64::new(0));

    let ring_cons = ring.clone();
    let err = errors.clone();
    let consumer = thread::spawn(move || {
        let mut cursor = 0u64;
        let mut expected = 0u64;

        while cursor < events {
            let prod_seq = producer_cursor.load(Ordering::Acquire);
            let available = prod_seq.saturating_sub(cursor);
            if available > 0 {
                let batch = (available as usize).min(BATCH_SIZE);
                let slots = ring_cons.get_read_batch(cursor, batch);
                for slot in slots {
                    if slot.value != expected {
                        err.fetch_add(1, Ordering::Relaxed);
                    }
                    expected += 1;
                }
                cursor += slots.len() as u64;
                ring_cons.update_consumer(cursor);
            } else {
                std::hint::spin_loop();
            }
        }
    });

    let ring_prod = ring.clone();
    let mut cursor = 0u64;
    let mut seq_num = 0u64;
    while cursor < events {
        let remaining = (events - cursor) as usize;
        let batch = remaining.min(BATCH_SIZE);
        if let Some((seq, slots)) = ring_prod.try_claim_slots(batch, cursor) {
            for slot in slots.iter_mut() {
                slot.value = seq_num;
                seq_num += 1;
            }
            ring_prod.publish(seq + slots.len() as u64);
            cursor = seq + slots.len() as u64;
        }
    }

    consumer.join().unwrap();

    let error_count = errors.load(Ordering::Acquire);
    assert_eq!(error_count, 0, "Sequence errors detected!");
    events
}

// ============================================================================
// XOR Checksum Verification
// ============================================================================

fn bench_xor_verify(events: u64) -> u64 {
    let ring = Arc::new(RingBuffer::<MessageSlot>::new(RING_SIZE).unwrap());
    let producer_cursor = ring.producer_cursor();
    let producer_xor = Arc::new(AtomicU64::new(0));
    let consumer_xor = Arc::new(AtomicU64::new(0));

    let ring_cons = ring.clone();
    let cons_xor = consumer_xor.clone();
    let consumer = thread::spawn(move || {
        let mut cursor = 0u64;
        let mut local_xor = 0u64;
        while cursor < events {
            let prod_seq = producer_cursor.load(Ordering::Acquire);
            if prod_seq > cursor {
                let batch = ((prod_seq - cursor) as usize).min(BATCH_SIZE);
                std::sync::atomic::fence(Ordering::Acquire);
                for i in 0..batch {
                    let seq = cursor + (i as u64);
                    unsafe {
                        let slot = ring_cons.read_slot(seq);
                        local_xor ^= slot.sequence();
                    }
                }
                cursor += batch as u64;
                ring_cons.update_consumer(cursor);
            }
        }
        cons_xor.store(local_xor, Ordering::Release);
    });

    let ring_prod = ring.clone();
    let prod_xor = producer_xor.clone();
    let mut cursor = 0u64;
    let mut value = 1u64;
    let mut local_xor = 0u64;

    let ring_ptr = Arc::as_ptr(&ring_prod) as *mut RingBuffer<MessageSlot>;

    while cursor < events {
        let ring_mut = unsafe { &mut *ring_ptr };
        let batch = ((events - cursor) as usize).min(BATCH_SIZE);
        if let Some(next) = ring_mut.try_claim(batch, cursor) {
            for i in 0..batch {
                let seq = cursor + (i as u64);
                let mut slot = MessageSlot::default();
                slot.set_sequence(value);
                unsafe {
                    ring_mut.write_slot(seq, slot);
                }
                local_xor ^= value;
                value = value.wrapping_add(1);
            }
            cursor = next;
            ring_mut.publish(next);
        }
    }

    prod_xor.store(local_xor, Ordering::Release);
    consumer.join().unwrap();

    let p = producer_xor.load(Ordering::Acquire);
    let c = consumer_xor.load(Ordering::Acquire);
    assert_eq!(p, c, "XOR checksum failed!");

    black_box(events)
}

// ============================================================================
// MessageSlot Variable Data (uses lock-free BroadcastRingBuffer)
// ============================================================================

fn bench_message_slot(events: u64) -> u64 {
    let config = RingBufferConfig::new(RING_SIZE).unwrap();
    let ring = Arc::new(MessageRingBuffer::new(config).unwrap());
    let received_count = Arc::new(AtomicU64::new(0));

    let ring_cons = ring.clone();
    let recv_cnt = received_count.clone();
    let consumer = thread::spawn(move || {
        let mut received = 0u64;
        while received < events {
            let batch = ring_cons.try_consume_batch_relaxed(0, 1024);
            if !batch.is_empty() {
                for slot in batch {
                    black_box(slot.data.len());
                }
                received += batch.len() as u64;
                recv_cnt.store(received, Ordering::Release);
            } else {
                std::hint::spin_loop();
            }
        }
    });

    let msg = b"Hello, World! This is a test message for benchmarking.";
    let mut sent = 0u64;

    // Use raw pointer for producer (same pattern as bench_xor_verify)
    let ring_ptr = Arc::as_ptr(&ring) as *mut MessageRingBuffer;

    while sent < events {
        let ring_mut = unsafe { &mut *ring_ptr };
        let batch = ((events - sent) as usize).min(1024);
        if let Some((seq, slots)) = ring_mut.try_claim_slots_relaxed(batch) {
            let count = slots.len();
            for slot in slots {
                slot.set_data(msg);
            }
            ring_mut.publish_batch_relaxed(seq, seq + (count as u64) - 1);
            sent += count as u64;
        } else {
            std::hint::spin_loop();
        }
    }

    consumer.join().unwrap();
    black_box(received_count.load(Ordering::Acquire))
}

// ============================================================================
// Benchmark Groups
// ============================================================================

fn benchmark_integrity(c: &mut Criterion) {
    let mut group = c.benchmark_group("Data Integrity (10M events)");
    group.throughput(Throughput::Elements(TOTAL_EVENTS));
    group.sample_size(10);

    group.bench_function(BenchmarkId::new("verify", "sum"), |b| {
        b.iter(|| bench_sum_verify(TOTAL_EVENTS))
    });

    group.bench_function(BenchmarkId::new("verify", "sequence"), |b| {
        b.iter(|| bench_sequence_verify(TOTAL_EVENTS))
    });

    group.finish();
}

fn benchmark_xor(c: &mut Criterion) {
    let mut group = c.benchmark_group("XOR Verified (5M events)");
    group.throughput(Throughput::Elements(5_000_000));
    group.sample_size(10);

    group.bench_function("128B_xor_verified", |b| {
        b.iter(|| bench_xor_verify(5_000_000))
    });

    group.finish();
}

fn benchmark_message_slot(c: &mut Criterion) {
    let mut group = c.benchmark_group("MessageSlot (1M events)");
    group.throughput(Throughput::Elements(MSG_EVENTS));
    group.sample_size(10);

    group.bench_function("128B_variable", |b| {
        b.iter(|| bench_message_slot(MSG_EVENTS))
    });

    group.finish();
}

criterion_group!(
    benches,
    benchmark_integrity,
    benchmark_xor,
    benchmark_message_slot
);
criterion_main!(benches);

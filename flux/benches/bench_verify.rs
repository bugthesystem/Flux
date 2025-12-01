//! Benchmark with data integrity verification using Criterion
//!
//! Ensures no data loss during high-throughput operations.

use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::thread;

use flux::disruptor::{RingBuffer, SmallSlot};

const RING_SIZE: usize = 1024 * 1024;
const BATCH_SIZE: usize = 8192;
const TOTAL_EVENTS: u64 = 10_000_000;

/// Benchmark with sum verification
fn bench_with_verification(events: u64) -> u64 {
    let ring = Arc::new(RingBuffer::<SmallSlot>::new(RING_SIZE).unwrap());
    let producer_cursor = ring.producer_cursor();
    let consumer_sum = Arc::new(AtomicU64::new(0));

    // Calculate expected sum
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
    assert_eq!(actual_sum, expected_sum, "Data integrity check failed!");
    events
}

/// Benchmark with sequence verification
fn bench_sequence_verification(events: u64) -> u64 {
    let ring = Arc::new(RingBuffer::<SmallSlot>::new(RING_SIZE).unwrap());
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

fn benchmark_verification(c: &mut Criterion) {
    let mut group = c.benchmark_group("Data Integrity (10M events)");
    group.throughput(Throughput::Elements(TOTAL_EVENTS));
    group.sample_size(10);

    group.bench_function(BenchmarkId::new("verify", "sum"), |b| {
        b.iter(|| bench_with_verification(TOTAL_EVENTS))
    });

    group.bench_function(BenchmarkId::new("verify", "sequence"), |b| {
        b.iter(|| bench_sequence_verification(TOTAL_EVENTS))
    });

    group.finish();
}

criterion_group!(benches, benchmark_verification);
criterion_main!(benches);

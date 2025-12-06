//! Benchmark: Closure-based publish vs Copy-based publish
//!
//! Tests the optimization that avoids copying entire slots.
//! Run: cargo bench --bench bench_closure

use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use std::hint::black_box;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::thread;

use kaos::disruptor::{RingBuffer, RingBufferEntry, Slot64, Slot8};

const RING_SIZE: usize = 1024 * 1024;
const BATCH_SIZE: usize = 8192;
const TOTAL_EVENTS: u64 = 10_000_000;

// ============================================================================
// OLD API: Copy-based (copies entire slot)
// ============================================================================

fn bench_copy_api<T: RingBufferEntry + Copy + Default + Send + Sync + 'static>(events: u64) -> u64 {
    let ring = Arc::new(RingBuffer::<T>::new(RING_SIZE).unwrap());
    let producer_cursor = ring.producer_cursor();

    let ring_cons = ring.clone();
    let consumer = thread::spawn(move || {
        let mut cursor = 0u64;
        while cursor < events {
            let prod_seq = producer_cursor.load(Ordering::Acquire);
            let available = prod_seq.saturating_sub(cursor);
            if available > 0 {
                let batch = (available as usize).min(BATCH_SIZE);
                let slots = ring_cons.get_read_batch(cursor, batch);
                for slot in slots {
                    black_box(slot.sequence());
                }
                cursor += slots.len() as u64;
                ring_cons.update_consumer(cursor);
            } else {
                std::hint::spin_loop();
            }
        }
    });

    // OLD: Copy entire slot
    let ring_prod = ring.clone();
    let mut cursor = 0u64;
    while cursor < events {
        let remaining = (events - cursor) as usize;
        let batch = remaining.min(BATCH_SIZE);
        if let Some((seq, slots)) = unsafe { ring_prod.try_claim_slots_unchecked(batch, cursor) } {
            for (i, slot) in slots.iter_mut().enumerate() {
                slot.set_sequence(((cursor + (i as u64)) % 5) + 1);
            }
            ring_prod.publish(seq + (slots.len() as u64));
            cursor = seq + (slots.len() as u64);
        }
    }

    consumer.join().unwrap();
    events
}

// ============================================================================
// NEW API: Closure-based (in-place mutation, zero-copy)
// ============================================================================

fn bench_closure_api<T: RingBufferEntry + Copy + Default + Send + Sync + 'static>(
    events: u64,
) -> u64 {
    let ring = Arc::new(RingBuffer::<T>::new(RING_SIZE).unwrap());
    let producer_cursor = ring.producer_cursor();

    let ring_cons = ring.clone();
    let consumer = thread::spawn(move || {
        let mut cursor = 0u64;
        while cursor < events {
            let prod_seq = producer_cursor.load(Ordering::Acquire);
            let available = prod_seq.saturating_sub(cursor);
            if available > 0 {
                let batch = (available as usize).min(BATCH_SIZE);
                let slots = ring_cons.get_read_batch(cursor, batch);
                for slot in slots {
                    black_box(slot.sequence());
                }
                cursor += slots.len() as u64;
                ring_cons.update_consumer(cursor);
            } else {
                std::hint::spin_loop();
            }
        }
    });

    // NEW: Closure-based in-place mutation
    let ring_prod = ring.clone();
    let mut cursor = 0u64;
    while cursor < events {
        let remaining = (events - cursor) as usize;
        let batch = remaining.min(BATCH_SIZE);
        if let Some((start, count)) =
            ring_prod.try_publish_batch_with(cursor, batch, |slot, seq| {
                slot.set_sequence((seq % 5) + 1);
            })
        {
            cursor = start + (count as u64);
        }
    }

    consumer.join().unwrap();
    events
}

// ============================================================================
// Benchmarks
// ============================================================================

fn benchmark_8b(c: &mut Criterion) {
    let mut group = c.benchmark_group("8B Slot (10M events)");
    group.throughput(Throughput::Elements(TOTAL_EVENTS));
    group.sample_size(20);

    group.bench_function(BenchmarkId::new("copy", "old"), |b| {
        b.iter(|| bench_copy_api::<Slot8>(TOTAL_EVENTS))
    });

    group.bench_function(BenchmarkId::new("closure", "new"), |b| {
        b.iter(|| bench_closure_api::<Slot8>(TOTAL_EVENTS))
    });

    group.finish();
}

fn benchmark_64b(c: &mut Criterion) {
    let mut group = c.benchmark_group("64B Slot (10M events)");
    group.throughput(Throughput::Elements(TOTAL_EVENTS));
    group.sample_size(20);

    group.bench_function(BenchmarkId::new("copy", "old"), |b| {
        b.iter(|| bench_copy_api::<Slot64>(TOTAL_EVENTS))
    });

    group.bench_function(BenchmarkId::new("closure", "new"), |b| {
        b.iter(|| bench_closure_api::<Slot64>(TOTAL_EVENTS))
    });

    group.finish();
}

criterion_group!(benches, benchmark_8b, benchmark_64b);
criterion_main!(benches);


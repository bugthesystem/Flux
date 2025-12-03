//! kaos vs disruptor-rs benchmark
//!
//! Side-by-side comparison with identical parameters.
//! Run: cargo bench --bench bench_comparison
//!
//! Parameters:
//! - Ring size: 1M slots
//! - Events: 10M
//! - Slot size: 8 bytes

use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use std::hint::black_box;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::thread;

// kaos
use kaos::disruptor::{RingBuffer, RingBufferEntry, Slot8};

// disruptor-rs
use disruptor::{build_single_producer, build_multi_producer, BusySpin, Producer};

const RING_SIZE: usize = 1024 * 1024;
const BATCH_SIZE: usize = 8192;
const TOTAL_EVENTS: u64 = 10_000_000;

// =============================================================================
// KAOS BENCHMARKS
// =============================================================================

/// kaos batch API (highest throughput)
fn kaos_batch(events: u64) -> u64 {
    let ring = Arc::new(RingBuffer::<Slot8>::new(RING_SIZE).unwrap());
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

    let ring_ptr = Arc::as_ptr(&ring) as *const RingBuffer<Slot8>;
    let mut cursor = 0u64;
    while cursor < events {
        let ring_ref = unsafe { &*ring_ptr };
        let remaining = (events - cursor) as usize;
        let batch = remaining.min(BATCH_SIZE);
        if let Some((seq, slots)) = ring_ref.try_claim_slots(batch, cursor) {
            for (i, slot) in slots.iter_mut().enumerate() {
                slot.set_sequence((cursor + i as u64) % 5 + 1);
            }
            ring_ref.publish(seq + slots.len() as u64);
            cursor = seq + slots.len() as u64;
        }
    }

    consumer.join().unwrap();
    events
}

/// kaos per-event API (fair comparison with disruptor-rs)
fn kaos_per_event(events: u64) -> u64 {
    let ring = Arc::new(RingBuffer::<Slot8>::new(RING_SIZE).unwrap());
    let producer_cursor = ring.producer_cursor();

    let ring_cons = ring.clone();
    let consumer = thread::spawn(move || {
        let mut cursor = 0u64;
        while cursor < events {
            let prod_seq = producer_cursor.load(Ordering::Acquire);
            if prod_seq > cursor {
                let slot = ring_cons.get_read_batch(cursor, 1);
                if !slot.is_empty() {
                    black_box(slot[0].sequence());
                    cursor += 1;
                    ring_cons.update_consumer(cursor);
                }
            } else {
                std::hint::spin_loop();
            }
        }
    });

    let ring_ptr = Arc::as_ptr(&ring) as *const RingBuffer<Slot8>;
    let mut cursor = 0u64;
    while cursor < events {
        let ring_ref = unsafe { &*ring_ptr };
        if let Some((seq, slots)) = ring_ref.try_claim_slots(1, cursor) {
            slots[0].set_sequence(cursor % 5 + 1);
            ring_ref.publish(seq + 1);
            cursor = seq + 1;
        }
    }

    consumer.join().unwrap();
    events
}

// =============================================================================
// DISRUPTOR-RS BENCHMARKS
// =============================================================================

/// disruptor-rs SPSC (per-event only)
fn disruptor_spsc(events: u64) -> u64 {
    let processor = move |data: &u64, _seq: i64, _eob: bool| {
        black_box(*data);
    };

    let mut producer = build_single_producer(RING_SIZE, || 0u64, BusySpin)
        .handle_events_with(processor)
        .build();

    for i in 0..events {
        producer.publish(|slot| {
            *slot = i % 5 + 1;
        });
    }

    drop(producer);
    events
}

/// disruptor-rs MPSC
fn disruptor_mpsc(events: u64, num_producers: usize) -> u64 {
    let events_per_producer = events / num_producers as u64;

    let processor = move |data: &u64, _seq: i64, _eob: bool| {
        black_box(*data);
    };

    let producer = build_multi_producer(RING_SIZE, || 0u64, BusySpin)
        .handle_events_with(processor)
        .build();

    let handles: Vec<_> = (0..num_producers)
        .map(|_| {
            let mut p = producer.clone();
            thread::spawn(move || {
                for i in 0..events_per_producer {
                    p.publish(|slot| {
                        *slot = i % 5 + 1;
                    });
                }
            })
        })
        .collect();

    for h in handles {
        h.join().unwrap();
    }

    drop(producer);
    events
}

// =============================================================================
// CRITERION BENCHMARKS
// =============================================================================

fn benchmark_spsc_comparison(c: &mut Criterion) {
    let mut group = c.benchmark_group("SPSC (8B slot, 10M events)");
    group.throughput(Throughput::Elements(TOTAL_EVENTS));
    group.sample_size(20);

    group.bench_function(BenchmarkId::new("kaos", "batch"), |b| {
        b.iter(|| kaos_batch(TOTAL_EVENTS))
    });

    group.bench_function(BenchmarkId::new("kaos", "per-event"), |b| {
        b.iter(|| kaos_per_event(TOTAL_EVENTS))
    });

    group.bench_function(BenchmarkId::new("disruptor-rs", "per-event"), |b| {
        b.iter(|| disruptor_spsc(TOTAL_EVENTS))
    });

    group.finish();
}

fn benchmark_mpsc_comparison(c: &mut Criterion) {
    let mut group = c.benchmark_group("MPSC (8B slot, 10M events)");
    group.throughput(Throughput::Elements(TOTAL_EVENTS));
    group.sample_size(20);

    group.bench_function(BenchmarkId::new("disruptor-rs", "2 producers"), |b| {
        b.iter(|| disruptor_mpsc(TOTAL_EVENTS, 2))
    });

    group.bench_function(BenchmarkId::new("disruptor-rs", "4 producers"), |b| {
        b.iter(|| disruptor_mpsc(TOTAL_EVENTS, 4))
    });

    group.finish();
}

criterion_group!(
    benches,
    benchmark_spsc_comparison,
    benchmark_mpsc_comparison,
);
criterion_main!(benches);

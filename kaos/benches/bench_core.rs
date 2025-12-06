//! Core SPSC Ring Buffer benchmarks
//!
//! Tests fundamental throughput:
//! - Fast mapped allocation (primary benchmark)
//! - Heap vs mapped comparison
//! - Minimal overhead baseline
//!
//! Run: cargo bench --bench bench_core

use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use std::hint::black_box;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::thread;

use kaos::disruptor::{RingBuffer, Slot8};

const RING_SIZE: usize = 1024 * 1024;
const BATCH_SIZE: usize = 8192;
const TOTAL_EVENTS: u64 = 10_000_000;

// ============================================================================
// Core benchmark function - used by all tests
// ============================================================================

fn bench_spsc(events: u64, use_mapped: bool, process_data: bool) -> u64 {
    let ring = if use_mapped {
        Arc::new(RingBuffer::<Slot8>::new_mapped(RING_SIZE).unwrap())
    } else {
        Arc::new(RingBuffer::<Slot8>::new(RING_SIZE).unwrap())
    };
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
                if process_data {
                    for slot in slots {
                        black_box(slot.value);
                    }
                } else {
                    black_box(slots.len());
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
    while cursor < events {
        let remaining = (events - cursor) as usize;
        let batch = remaining.min(BATCH_SIZE);
        // SAFETY: Single producer thread, no concurrent writes
        if let Some((seq, slots)) = unsafe { ring_prod.try_claim_slots_unchecked(batch, cursor) } {
            if process_data {
                for (i, slot) in slots.iter_mut().enumerate() {
                    slot.value = cursor + (i as u64);
                }
            } else {
                black_box(slots.len());
            }
            ring_prod.publish(seq + (slots.len() as u64));
            cursor = seq + (slots.len() as u64);
        }
    }

    consumer.join().unwrap();
    events
}

// ============================================================================
// Benchmarks
// ============================================================================

fn benchmark_throughput(c: &mut Criterion) {
    let mut group = c.benchmark_group("Core Throughput (10M events)");
    group.throughput(Throughput::Elements(TOTAL_EVENTS));
    group.sample_size(20);

    // Primary benchmark - fast mapped
    group.bench_function("mapped_batch", |b| {
        b.iter(|| bench_spsc(TOTAL_EVENTS, true, true))
    });

    // Heap comparison
    group.bench_function("heap_batch", |b| {
        b.iter(|| bench_spsc(TOTAL_EVENTS, false, true))
    });

    // Minimal overhead baseline
    group.bench_function("minimal_overhead", |b| {
        b.iter(|| bench_spsc(TOTAL_EVENTS, false, false))
    });

    group.finish();
}

fn benchmark_allocation(c: &mut Criterion) {
    let mut group = c.benchmark_group("Allocation Comparison");
    group.throughput(Throughput::Elements(TOTAL_EVENTS));
    group.sample_size(20);

    group.bench_function(BenchmarkId::new("alloc", "heap"), |b| {
        b.iter(|| bench_spsc(TOTAL_EVENTS, false, true))
    });

    group.bench_function(BenchmarkId::new("alloc", "mapped"), |b| {
        b.iter(|| bench_spsc(TOTAL_EVENTS, true, true))
    });

    group.finish();
}

criterion_group!(benches, benchmark_throughput, benchmark_allocation);
criterion_main!(benches);

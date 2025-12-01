//! Criterion-based flux benchmark
//!
//! Run: cargo bench --bench bench_criterion
//!
//! For disruptor-rs comparison, see: ext-benches/disruptor-rs-bench/

use criterion::{ criterion_group, criterion_main, BenchmarkId, Criterion, Throughput };
use std::hint::black_box;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::thread;

use flux::disruptor::{
    RingBuffer,
    RingBufferEntry,
    SmallSlot,
    Slot16,
    Slot32,
    Slot64,
    MessageSlot,
};

const RING_SIZE: usize = 1024 * 1024;
const BATCH_SIZE: usize = 8192;
const TOTAL_EVENTS: u64 = 10_000_000;

/// Batch API benchmark
fn flux_batch<T: RingBufferEntry + Copy + Default + Send + Sync + 'static>(events: u64) -> u64 {
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

    let ring_ptr = Arc::as_ptr(&ring) as *const RingBuffer<T>;
    let mut cursor = 0u64;

    while cursor < events {
        let ring_ref = unsafe { &*ring_ptr };
        let remaining = (events - cursor) as usize;
        let batch = remaining.min(BATCH_SIZE);

        if let Some((seq, slots)) = ring_ref.try_claim_slots(batch, cursor) {
            for (i, slot) in slots.iter_mut().enumerate() {
                slot.set_sequence(((cursor + (i as u64)) % 5) + 1);
            }
            let next = seq + (slots.len() as u64);
            ring_ref.publish(next);
            cursor = next;
        }
    }

    consumer.join().unwrap();
    events
}

/// Per-event API benchmark
fn flux_per_event<T: RingBufferEntry + Copy + Default + Send + Sync + 'static>(events: u64) -> u64 {
    let ring = Arc::new(RingBuffer::<T>::new(RING_SIZE).unwrap());
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

    let ring_ptr = Arc::as_ptr(&ring) as *const RingBuffer<T>;
    let mut cursor = 0u64;

    while cursor < events {
        let ring_ref = unsafe { &*ring_ptr };
        if let Some((seq, slots)) = ring_ref.try_claim_slots(1, cursor) {
            slots[0].set_sequence((cursor % 5) + 1);
            ring_ref.publish(seq + 1);
            cursor = seq + 1;
        }
    }

    consumer.join().unwrap();
    events
}

fn benchmark_slot_sizes_batch(c: &mut Criterion) {
    let mut group = c.benchmark_group("Batch API by Slot Size");
    group.throughput(Throughput::Elements(TOTAL_EVENTS));
    group.sample_size(20);

    group.bench_function(BenchmarkId::new("flux", "8B (SmallSlot)"), |b| {
        b.iter(|| flux_batch::<SmallSlot>(TOTAL_EVENTS))
    });

    group.bench_function(BenchmarkId::new("flux", "16B (Slot16)"), |b| {
        b.iter(|| flux_batch::<Slot16>(TOTAL_EVENTS))
    });

    group.bench_function(BenchmarkId::new("flux", "32B (Slot32)"), |b| {
        b.iter(|| flux_batch::<Slot32>(TOTAL_EVENTS))
    });

    group.bench_function(BenchmarkId::new("flux", "64B (Slot64)"), |b| {
        b.iter(|| flux_batch::<Slot64>(TOTAL_EVENTS))
    });

    group.bench_function(BenchmarkId::new("flux", "128B (MessageSlot)"), |b| {
        b.iter(|| flux_batch::<MessageSlot>(TOTAL_EVENTS))
    });

    group.finish();
}

fn benchmark_batch_vs_per_event(c: &mut Criterion) {
    let mut group = c.benchmark_group("Batch vs Per-Event (8B)");
    group.throughput(Throughput::Elements(TOTAL_EVENTS));
    group.sample_size(20);

    group.bench_function("batch (8192/call)", |b| {
        b.iter(|| flux_batch::<SmallSlot>(TOTAL_EVENTS))
    });

    group.bench_function("per-event (1/call)", |b| {
        b.iter(|| flux_per_event::<SmallSlot>(TOTAL_EVENTS))
    });

    group.finish();
}

criterion_group!(benches, benchmark_slot_sizes_batch, benchmark_batch_vs_per_event);
criterion_main!(benches);

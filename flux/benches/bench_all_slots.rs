//! Benchmark all slot sizes with Criterion
//!
//! Tests throughput across different slot sizes to help choose the right one.

use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use std::hint::black_box;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::thread;

use flux::disruptor::{RingBuffer, RingBufferEntry, SmallSlot, Slot16, Slot32, Slot64, MessageSlot};

const RING_SIZE: usize = 1024 * 1024;
const BATCH_SIZE: usize = 8192;
const TOTAL_EVENTS: u64 = 10_000_000;

fn bench_slot<T: RingBufferEntry + Copy + Default + Send + Sync + 'static>(events: u64) -> u64 {
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

    let ring_prod = ring.clone();
    let mut cursor = 0u64;
    while cursor < events {
        let remaining = (events - cursor) as usize;
        let batch = remaining.min(BATCH_SIZE);
        if let Some((seq, slots)) = ring_prod.try_claim_slots(batch, cursor) {
            for (i, slot) in slots.iter_mut().enumerate() {
                slot.set_sequence((cursor + i as u64) % 5 + 1);
            }
            ring_prod.publish(seq + slots.len() as u64);
            cursor = seq + slots.len() as u64;
        }
    }

    consumer.join().unwrap();
    events
}

fn benchmark_all_slots(c: &mut Criterion) {
    let mut group = c.benchmark_group("Slot Sizes (10M events)");
    group.throughput(Throughput::Elements(TOTAL_EVENTS));
    group.sample_size(20);

    group.bench_function(BenchmarkId::new("size", "8B"), |b| {
        b.iter(|| bench_slot::<SmallSlot>(TOTAL_EVENTS))
    });

    group.bench_function(BenchmarkId::new("size", "16B"), |b| {
        b.iter(|| bench_slot::<Slot16>(TOTAL_EVENTS))
    });

    group.bench_function(BenchmarkId::new("size", "32B"), |b| {
        b.iter(|| bench_slot::<Slot32>(TOTAL_EVENTS))
    });

    group.bench_function(BenchmarkId::new("size", "64B"), |b| {
        b.iter(|| bench_slot::<Slot64>(TOTAL_EVENTS))
    });

    group.bench_function(BenchmarkId::new("size", "128B"), |b| {
        b.iter(|| bench_slot::<MessageSlot>(TOTAL_EVENTS))
    });

    group.finish();
}

criterion_group!(benches, benchmark_all_slots);
criterion_main!(benches);

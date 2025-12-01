//! Minimal overhead benchmark with Criterion
//!
//! Tests raw ring buffer performance with minimal processing.

use criterion::{criterion_group, criterion_main, Criterion, Throughput};
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::thread;
use std::hint::black_box;

use flux::disruptor::{RingBuffer, SmallSlot};

const RING_SIZE: usize = 1024 * 1024;
const BATCH_SIZE: usize = 8192;
const TOTAL_EVENTS: u64 = 10_000_000;

fn bench_minimal(events: u64) -> u64 {
    let ring = Arc::new(RingBuffer::<SmallSlot>::new(RING_SIZE).unwrap());
    let producer_cursor = ring.producer_cursor();

    let ring_cons = ring.clone();
    let consumer = thread::spawn(move || {
        let mut cursor = 0u64;
        while cursor < events {
            let prod_seq = producer_cursor.load(Ordering::Acquire);
            if prod_seq > cursor {
                let batch = ((prod_seq - cursor) as usize).min(BATCH_SIZE);
                let slots = ring_cons.get_read_batch(cursor, batch);
                black_box(slots.len());
                cursor += slots.len() as u64;
                ring_cons.update_consumer(cursor);
            }
        }
    });

    let ring_prod = ring.clone();
    let mut cursor = 0u64;
    while cursor < events {
        let batch = ((events - cursor) as usize).min(BATCH_SIZE);
        if let Some((seq, slots)) = ring_prod.try_claim_slots(batch, cursor) {
            black_box(slots.len());
            ring_prod.publish(seq + slots.len() as u64);
            cursor = seq + slots.len() as u64;
        }
    }

    consumer.join().unwrap();
    events
}

fn benchmark_minimal(c: &mut Criterion) {
    let mut group = c.benchmark_group("Minimal Overhead (10M events)");
    group.throughput(Throughput::Elements(TOTAL_EVENTS));
    group.sample_size(20);

    group.bench_function("raw_throughput", |b| {
        b.iter(|| bench_minimal(TOTAL_EVENTS))
    });

    group.finish();
}

criterion_group!(benches, benchmark_minimal);
criterion_main!(benches);

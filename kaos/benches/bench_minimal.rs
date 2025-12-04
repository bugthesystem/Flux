//! Minimal test to verify criterion works

use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use std::hint::black_box;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::thread;

use kaos::disruptor::{
    ConsumerBuilder,
    EventHandler,
    MessageRingBuffer,
    MessageSlot,
    ProducerBuilder,
    RingBuffer,
    RingBufferConfig,
    RingBufferEntry,
    Slot8,
};
use kaos::{consume_batch, publish_batch, publish_unrolled};

const RING_SIZE: usize = 1024 * 1024;
const BATCH_SIZE: usize = 8192;
const API_EVENTS: u64 = 100_000;

fn bench_batch(events: u64) -> u64 {
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
                    black_box(slot.value);
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
                slot.value = cursor + (i as u64);
            }
            ring_prod.publish(seq + (slots.len() as u64));
            cursor = seq + (slots.len() as u64);
        } else {
            std::hint::spin_loop();
        }
    }

    consumer.join().unwrap();
    events
}

fn benchmark_api(c: &mut Criterion) {
    let mut group = c.benchmark_group("Minimal API Test");
    group.throughput(Throughput::Elements(API_EVENTS));
    group.sample_size(10);

    group.bench_function("batch", |b| {
        b.iter(|| bench_batch(API_EVENTS))
    });

    group.finish();
}

criterion_group!(benches, benchmark_api);
criterion_main!(benches);

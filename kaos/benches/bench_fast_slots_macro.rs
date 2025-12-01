//! Slot sizes benchmark with Criterion
//!
//! Tests performance across 16, 32, 64 byte slots.

use criterion::{ criterion_group, criterion_main, Criterion, Throughput };
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::thread;
use std::hint::black_box;

use kaos::disruptor::{ RingBuffer, Slot16, Slot32, Slot64 };

const RING_SIZE: usize = 1024 * 1024;
const BATCH_SIZE: usize = 8192;
const TOTAL_EVENTS: u64 = 10_000_000;

fn bench_slot16(events: u64) -> u64 {
    let ring = Arc::new(RingBuffer::<Slot16>::new(RING_SIZE).unwrap());
    let producer_cursor = ring.producer_cursor();

    let ring_cons = ring.clone();
    let consumer = thread::spawn(move || {
        let mut cursor = 0u64;
        while cursor < events {
            let prod_seq = producer_cursor.load(Ordering::Acquire);
            if prod_seq > cursor {
                let batch = ((prod_seq - cursor) as usize).min(BATCH_SIZE);
                let slots = ring_cons.get_read_batch(cursor, batch);
                for slot in slots {
                    black_box(slot.value1);
                }
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
            for (i, slot) in slots.iter_mut().enumerate() {
                slot.value1 = cursor + (i as u64);
                slot.value2 = 12345;
            }
            ring_prod.publish(seq + (slots.len() as u64));
            cursor = seq + (slots.len() as u64);
        }
    }

    consumer.join().unwrap();
    events
}

fn bench_slot32(events: u64) -> u64 {
    let ring = Arc::new(RingBuffer::<Slot32>::new(RING_SIZE).unwrap());
    let producer_cursor = ring.producer_cursor();

    let ring_cons = ring.clone();
    let consumer = thread::spawn(move || {
        let mut cursor = 0u64;
        while cursor < events {
            let prod_seq = producer_cursor.load(Ordering::Acquire);
            if prod_seq > cursor {
                let batch = ((prod_seq - cursor) as usize).min(BATCH_SIZE);
                let slots = ring_cons.get_read_batch(cursor, batch);
                for slot in slots {
                    black_box(slot.value1);
                }
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
            for (i, slot) in slots.iter_mut().enumerate() {
                slot.value1 = cursor + (i as u64);
                slot.value2 = 12345;
                slot.value3 = 67890;
                slot.value4 = 11111;
            }
            ring_prod.publish(seq + (slots.len() as u64));
            cursor = seq + (slots.len() as u64);
        }
    }

    consumer.join().unwrap();
    events
}

fn bench_slot64(events: u64) -> u64 {
    let ring = Arc::new(RingBuffer::<Slot64>::new(RING_SIZE).unwrap());
    let producer_cursor = ring.producer_cursor();

    let ring_cons = ring.clone();
    let consumer = thread::spawn(move || {
        let mut cursor = 0u64;
        while cursor < events {
            let prod_seq = producer_cursor.load(Ordering::Acquire);
            if prod_seq > cursor {
                let batch = ((prod_seq - cursor) as usize).min(BATCH_SIZE);
                let slots = ring_cons.get_read_batch(cursor, batch);
                for slot in slots {
                    black_box(slot.values[0]);
                }
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
            for (i, slot) in slots.iter_mut().enumerate() {
                slot.values = [cursor + (i as u64), 1, 2, 3, 4, 5, 6, 7];
            }
            ring_prod.publish(seq + (slots.len() as u64));
            cursor = seq + (slots.len() as u64);
        }
    }

    consumer.join().unwrap();
    events
}

fn benchmark_slots(c: &mut Criterion) {
    let mut group = c.benchmark_group("Slot Sizes (10M events)");
    group.throughput(Throughput::Elements(TOTAL_EVENTS));
    group.sample_size(20);

    group.bench_function("16B_slot", |b| b.iter(|| bench_slot16(TOTAL_EVENTS)));
    group.bench_function("32B_slot", |b| b.iter(|| bench_slot32(TOTAL_EVENTS)));
    group.bench_function("64B_slot", |b| b.iter(|| bench_slot64(TOTAL_EVENTS)));

    group.finish();
}

criterion_group!(benches, benchmark_slots);
criterion_main!(benches);

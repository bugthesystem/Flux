//! Fast Ring Buffer benchmark with Criterion
//!
//! Tests maximum throughput with mapped allocation and batch API.

use criterion::{ criterion_group, criterion_main, Criterion, Throughput };
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::thread;
use std::hint::black_box;

use flux::disruptor::{ RingBuffer, SmallSlot };

const RING_SIZE: usize = 1024 * 1024;
const BATCH_SIZE: usize = 8192;
const TOTAL_EVENTS: u64 = 10_000_000;

fn bench_fast_mapped(events: u64) -> u64 {
    let ring = Arc::new(RingBuffer::<SmallSlot>::new_mapped(RING_SIZE).unwrap());
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
        }
    }

    consumer.join().unwrap();
    events
}

fn benchmark_fast(c: &mut Criterion) {
    let mut group = c.benchmark_group("Fast Mapped (10M events)");
    group.throughput(Throughput::Elements(TOTAL_EVENTS));
    group.sample_size(20);

    group.bench_function("8B_mapped_batch", |b| { b.iter(|| bench_fast_mapped(TOTAL_EVENTS)) });

    group.finish();
}

criterion_group!(benches, benchmark_fast);
criterion_main!(benches);

//! Macro API benchmark with Criterion
//!
//! Tests the publish_batch! and consume_batch! macros.

use criterion::{ criterion_group, criterion_main, Criterion, Throughput };
use std::sync::Arc;
use std::thread;

use kaos::disruptor::{ RingBuffer, Slot8 };
use kaos::{ publish_batch, consume_batch };

const RING_SIZE: usize = 1024 * 1024;
const BATCH_SIZE: usize = 8192;
const TOTAL_EVENTS: u64 = 10_000_000;

fn bench_with_macros(events: u64) -> u64 {
    let ring = Arc::new(RingBuffer::<Slot8>::new(RING_SIZE).unwrap());
    let producer_cursor = ring.producer_cursor();

    let ring_cons = ring.clone();
    let consumer = thread::spawn(move || {
        let mut cursor = 0u64;
        while cursor < events {
            let count = consume_batch!(
                Slot8,
                ring_cons,
                producer_cursor,
                cursor,
                BATCH_SIZE,
                slot,
                {
                    std::hint::black_box(slot.value);
                }
            );
            if count == 0 {
                std::hint::spin_loop();
            }
        }
    });

    let ring_prod = ring.clone();
    let mut cursor = 0u64;
    while cursor < events {
        let remaining = (events - cursor) as usize;
        let batch = remaining.min(BATCH_SIZE);
        let _ = publish_batch!(Slot8, ring_prod, cursor, batch, i, slot, {
            slot.value = cursor + (i as u64);
        });
    }

    consumer.join().unwrap();
    events
}

fn benchmark_macros(c: &mut Criterion) {
    let mut group = c.benchmark_group("Macro API (10M events)");
    group.throughput(Throughput::Elements(TOTAL_EVENTS));
    group.sample_size(20);

    group.bench_function("publish_consume_batch", |b| {
        b.iter(|| bench_with_macros(TOTAL_EVENTS))
    });

    group.finish();
}

criterion_group!(benches, benchmark_macros);
criterion_main!(benches);

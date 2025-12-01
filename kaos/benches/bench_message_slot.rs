//! MessageSlot benchmark with Criterion
//!
//! Tests variable-length message handling with 128-byte slots.

use criterion::{ criterion_group, criterion_main, Criterion, Throughput };
use std::sync::Arc;
use std::thread;
use std::hint::black_box;
use std::sync::atomic::{ AtomicU64, Ordering };

use kaos::disruptor::{ MessageRingBuffer, RingBufferConfig };

const RING_SIZE: usize = 1024 * 1024;
const BATCH_SIZE: usize = 1024;
const TOTAL_EVENTS: u64 = 1_000_000;

fn bench_message_slot(events: u64) -> u64 {
    let config = RingBufferConfig::new(RING_SIZE).unwrap();
    let ring = Arc::new(parking_lot::Mutex::new(MessageRingBuffer::new(config).unwrap()));
    let received_count = Arc::new(AtomicU64::new(0));

    let ring_cons = ring.clone();
    let recv_cnt = received_count.clone();
    let consumer = thread::spawn(move || {
        let mut received = 0u64;
        while received < events {
            let count = {
                let guard = ring_cons.lock();
                let slots = guard.try_consume_batch(0, BATCH_SIZE);
                let c = slots.len();
                for slot in slots {
                    black_box(slot.data.len());
                }
                c
            };

            if count > 0 {
                received += count as u64;
                recv_cnt.store(received, Ordering::Release);
                let mut guard = ring_cons.lock();
                guard.advance_consumer(0, received.saturating_sub(1));
            } else {
                std::hint::spin_loop();
            }
        }
    });

    let msg = b"Hello, World! This is a test message for benchmarking.";
    let mut sent = 0u64;
    while sent < events {
        let batch = ((events - sent) as usize).min(BATCH_SIZE);
        let mut guard = ring.lock();
        if let Some((_seq, slots)) = guard.try_claim_slots(batch) {
            let count = slots.len();
            for slot in slots {
                slot.set_data(msg);
            }
            guard.publish_batch(sent, count);
            sent += count as u64;
        }
    }

    consumer.join().unwrap();
    events
}

fn benchmark_message(c: &mut Criterion) {
    let mut group = c.benchmark_group("MessageSlot (1M events)");
    group.throughput(Throughput::Elements(TOTAL_EVENTS));
    group.sample_size(10);

    group.bench_function("128B_variable", |b| { b.iter(|| bench_message_slot(TOTAL_EVENTS)) });

    group.finish();
}

criterion_group!(benches, benchmark_message);
criterion_main!(benches);

//! SPSC Macro benchmark with Criterion
//!
//! Tests the publish_unrolled! macro performance.

use criterion::{ criterion_group, criterion_main, Criterion, Throughput };
use std::sync::Arc;
use std::thread;
use std::hint::black_box;

use kaos::disruptor::{ MessageRingBuffer, RingBufferConfig, ProducerBuilder, RingBufferEntry };
use kaos::publish_unrolled;

const RING_SIZE: usize = 1024 * 1024;
const BATCH_SIZE: usize = 8192;
const TOTAL_EVENTS: u64 = 10_000_000;

fn bench_macro_publish(events: u64) -> u64 {
    let config = RingBufferConfig::new(RING_SIZE).unwrap();
    let ring_buffer = Arc::new(MessageRingBuffer::new(config).unwrap());
    let consumed = Arc::new(std::sync::atomic::AtomicU64::new(0));

    let rb_cons = ring_buffer.clone();
    let cons_count = consumed.clone();
    let consumer = thread::spawn(move || {
        let mut cursor = 0u64;
        while cursor < events {
            let batch = rb_cons.try_consume_batch_relaxed(0, BATCH_SIZE);
            if !batch.is_empty() {
                cursor += batch.len() as u64;
                cons_count.fetch_add(batch.len() as u64, std::sync::atomic::Ordering::Relaxed);
            }
        }
    });

    let mut producer = ProducerBuilder::new()
        .with_ring_buffer(ring_buffer.clone())
        .build()
        .unwrap();

    let test_data = b"BENCHMARK_MESSAGE_DATA_PAYLOAD_64_BYTES_XXXXXXXXXXXXXXXXXXXXXXX";
    let mut sent = 0u64;

    while sent < events {
        match
            publish_unrolled!(producer, BATCH_SIZE, seq, i, slot, {
                slot.set_sequence(seq + (i as u64));
                slot.set_data(test_data);
            })
        {
            Ok(count) => {
                sent += count as u64;
            }
            Err(_) => std::thread::yield_now(),
        }
    }

    consumer.join().unwrap();
    black_box(consumed.load(std::sync::atomic::Ordering::Relaxed))
}

fn benchmark_macro(c: &mut Criterion) {
    let mut group = c.benchmark_group("Macro Publish (10M events)");
    group.throughput(Throughput::Elements(TOTAL_EVENTS));
    group.sample_size(10);

    group.bench_function("publish_unrolled", |b| { b.iter(|| bench_macro_publish(TOTAL_EVENTS)) });

    group.finish();
}

criterion_group!(benches, benchmark_macro);
criterion_main!(benches);

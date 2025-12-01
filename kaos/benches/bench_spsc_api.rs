//! SPSC API benchmark with Criterion
//!
//! Tests the high-level Producer/Consumer API performance.

use criterion::{criterion_group, criterion_main, Criterion, Throughput};
use std::sync::Arc;
use std::thread;
use std::hint::black_box;

use kaos::disruptor::{MessageRingBuffer, RingBufferConfig, ProducerBuilder, ConsumerBuilder, EventHandler, MessageSlot, RingBufferEntry};

const RING_SIZE: usize = 1024 * 1024;
const BATCH_SIZE: usize = 2048;
const TOTAL_EVENTS: u64 = 1_000_000;

fn bench_producer_consumer_api(events: u64) -> u64 {
    let config = RingBufferConfig::new(RING_SIZE).unwrap();
    let ring_buffer = Arc::new(MessageRingBuffer::new(config).unwrap());
    let consumed = Arc::new(std::sync::atomic::AtomicU64::new(0));
    let stop = Arc::new(std::sync::atomic::AtomicBool::new(false));

    let mut producer = ProducerBuilder::new()
        .with_ring_buffer(ring_buffer.clone())
        .build()
        .unwrap();

    let consumer = ConsumerBuilder::new()
        .with_ring_buffer(ring_buffer.clone())
        .with_consumer_id(0)
        .with_batch_size(BATCH_SIZE)
        .build()
        .unwrap();

    struct CountHandler {
        count: Arc<std::sync::atomic::AtomicU64>,
    }

    impl EventHandler for CountHandler {
        fn on_event(&mut self, _event: &MessageSlot, _seq: u64, _end_of_batch: bool) {
            self.count.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        }
    }

    let cons_count = consumed.clone();
    let cons_stop = stop.clone();
    let consumer_thread = thread::spawn(move || {
        let mut handler = CountHandler { count: cons_count };
        consumer.run_loop(&mut handler, &cons_stop);
    });

    let test_data = b"BENCHMARK_MESSAGE_DATA_PAYLOAD_64_BYTES_XXXXXXXXXXXXXXXXXXXXXXX";
    let batch: Vec<&[u8]> = vec![test_data; BATCH_SIZE];
    let mut sent = 0u64;

    while sent < events {
        match producer.publish_batch(&batch, |event, seq, data| {
            event.set_sequence(seq);
            event.set_data(data);
        }) {
            Ok(count) => sent += count as u64,
            Err(_) => std::thread::yield_now(),
        }
    }

    // Wait for consumer
    while consumed.load(std::sync::atomic::Ordering::Relaxed) < events {
        std::thread::yield_now();
    }

    stop.store(true, std::sync::atomic::Ordering::Relaxed);
    consumer_thread.join().unwrap();

    black_box(consumed.load(std::sync::atomic::Ordering::Relaxed))
}

fn benchmark_api(c: &mut Criterion) {
    let mut group = c.benchmark_group("SPSC API (1M events)");
    group.throughput(Throughput::Elements(TOTAL_EVENTS));
    group.sample_size(10);

    group.bench_function("producer_consumer", |b| {
        b.iter(|| bench_producer_consumer_api(TOTAL_EVENTS))
    });

    group.finish();
}

criterion_group!(benches, benchmark_api);
criterion_main!(benches);

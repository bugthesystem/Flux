//! High-level API benchmarks
//!
//! Tests Producer/Consumer API, macros, and batch vs single operations.
//!
//! Run: cargo bench --bench bench_api

use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use std::hint::black_box;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::thread;

use kaos::disruptor::{
    ConsumerBuilder, EventHandler, MessageRingBuffer, MessageSlot, ProducerBuilder,
    RingBuffer, RingBufferConfig, RingBufferEntry, Slot8,
};
use kaos::{consume_batch, publish_batch, publish_unrolled};

const RING_SIZE: usize = 1024 * 1024;
const BATCH_SIZE: usize = 8192;
const TOTAL_EVENTS: u64 = 10_000_000;
const API_EVENTS: u64 = 1_000_000; // Smaller for API tests

// ============================================================================
// Low-level API: Batch vs Single
// ============================================================================

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
                slot.value = cursor + i as u64;
            }
            ring_prod.publish(seq + slots.len() as u64);
            cursor = seq + slots.len() as u64;
        }
    }

    consumer.join().unwrap();
    events
}

fn bench_single(events: u64) -> u64 {
    let ring = Arc::new(RingBuffer::<Slot8>::new(RING_SIZE).unwrap());
    let producer_cursor = ring.producer_cursor();

    let ring_cons = ring.clone();
    let consumer = thread::spawn(move || {
        let mut cursor = 0u64;
        while cursor < events {
            let prod_seq = producer_cursor.load(Ordering::Acquire);
            if prod_seq > cursor {
                let slots = ring_cons.get_read_batch(cursor, 1);
                black_box(slots[0].value);
                cursor += 1;
                ring_cons.update_consumer(cursor);
            } else {
                std::hint::spin_loop();
            }
        }
    });

    let ring_prod = ring.clone();
    let mut cursor = 0u64;
    while cursor < events {
        if let Some((seq, slots)) = ring_prod.try_claim_slots(1, cursor) {
            slots[0].value = cursor;
            ring_prod.publish(seq + 1);
            cursor = seq + 1;
        }
    }

    consumer.join().unwrap();
    events
}

// ============================================================================
// Macro API
// ============================================================================

fn bench_macros(events: u64) -> u64 {
    let ring = Arc::new(RingBuffer::<Slot8>::new(RING_SIZE).unwrap());
    let producer_cursor = ring.producer_cursor();

    let ring_cons = ring.clone();
    let consumer = thread::spawn(move || {
        let mut cursor = 0u64;
        while cursor < events {
            let count = consume_batch!(Slot8, ring_cons, producer_cursor, cursor, BATCH_SIZE, slot, {
                black_box(slot.value);
            });
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

fn bench_unrolled_macro(events: u64) -> u64 {
    let config = RingBufferConfig::new(RING_SIZE).unwrap();
    let ring_buffer = Arc::new(MessageRingBuffer::new(config).unwrap());
    let consumed = Arc::new(AtomicU64::new(0));

    let rb_cons = ring_buffer.clone();
    let cons_count = consumed.clone();
    let consumer = thread::spawn(move || {
        let mut cursor = 0u64;
        while cursor < events {
            let batch = rb_cons.try_consume_batch_relaxed(0, BATCH_SIZE);
            if !batch.is_empty() {
                cursor += batch.len() as u64;
                cons_count.fetch_add(batch.len() as u64, Ordering::Relaxed);
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
        match publish_unrolled!(producer, BATCH_SIZE, seq, i, slot, {
            slot.set_sequence(seq + (i as u64));
            slot.set_data(test_data);
        }) {
            Ok(count) => sent += count as u64,
            Err(_) => std::thread::yield_now(),
        }
    }

    consumer.join().unwrap();
    black_box(consumed.load(Ordering::Relaxed))
}

// ============================================================================
// High-level Producer/Consumer API
// ============================================================================

fn bench_producer_consumer(events: u64) -> u64 {
    let config = RingBufferConfig::new(RING_SIZE).unwrap();
    let ring_buffer = Arc::new(MessageRingBuffer::new(config).unwrap());
    let consumed = Arc::new(AtomicU64::new(0));
    let stop = Arc::new(AtomicBool::new(false));

    let mut producer = ProducerBuilder::new()
        .with_ring_buffer(ring_buffer.clone())
        .build()
        .unwrap();

    let consumer = ConsumerBuilder::new()
        .with_ring_buffer(ring_buffer.clone())
        .with_consumer_id(0)
        .with_batch_size(2048)
        .build()
        .unwrap();

    struct CountHandler {
        count: Arc<AtomicU64>,
    }

    impl EventHandler for CountHandler {
        fn on_event(&mut self, _event: &MessageSlot, _seq: u64, _end_of_batch: bool) {
            self.count.fetch_add(1, Ordering::Relaxed);
        }
    }

    let cons_count = consumed.clone();
    let cons_stop = stop.clone();
    let consumer_thread = thread::spawn(move || {
        let mut handler = CountHandler { count: cons_count };
        consumer.run_loop(&mut handler, &cons_stop);
    });

    let test_data = b"BENCHMARK_MESSAGE_DATA_PAYLOAD_64_BYTES_XXXXXXXXXXXXXXXXXXXXXXX";
    let batch: Vec<&[u8]> = vec![test_data; 2048];
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

    while consumed.load(Ordering::Relaxed) < events {
        std::thread::yield_now();
    }

    stop.store(true, Ordering::Relaxed);
    consumer_thread.join().unwrap();

    black_box(consumed.load(Ordering::Relaxed))
}

// ============================================================================
// Benchmark Groups
// ============================================================================

fn benchmark_batch_vs_single(c: &mut Criterion) {
    let mut group = c.benchmark_group("Batch vs Single (10M events)");
    group.throughput(Throughput::Elements(TOTAL_EVENTS));
    group.sample_size(20);

    group.bench_function(BenchmarkId::new("api", "batch"), |b| {
        b.iter(|| bench_batch(TOTAL_EVENTS))
    });

    group.bench_function(BenchmarkId::new("api", "single"), |b| {
        b.iter(|| bench_single(TOTAL_EVENTS))
    });

    group.finish();
}

fn benchmark_macros(c: &mut Criterion) {
    let mut group = c.benchmark_group("Macro API (10M events)");
    group.throughput(Throughput::Elements(TOTAL_EVENTS));
    group.sample_size(20);

    group.bench_function("publish_consume_batch", |b| {
        b.iter(|| bench_macros(TOTAL_EVENTS))
    });

    group.bench_function("publish_unrolled", |b| {
        b.iter(|| bench_unrolled_macro(TOTAL_EVENTS))
    });

    group.finish();
}

fn benchmark_high_level(c: &mut Criterion) {
    let mut group = c.benchmark_group("Producer/Consumer API (1M events)");
    group.throughput(Throughput::Elements(API_EVENTS));
    group.sample_size(10);

    group.bench_function("producer_consumer", |b| {
        b.iter(|| bench_producer_consumer(API_EVENTS))
    });

    group.finish();
}

criterion_group!(
    benches,
    benchmark_batch_vs_single,
    benchmark_macros,
    benchmark_high_level
);
criterion_main!(benches);


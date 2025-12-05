//! High-level API benchmarks
//!
//! Tests low-level batch API, macros, and Producer/Consumer patterns.
//!
//! Run: cargo bench --bench bench_api

use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use std::hint::black_box;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::thread;

use kaos::disruptor::{
    ConsumerBuilder, EventHandler, MessageRingBuffer, MessageSlot, ProducerBuilder, RingBuffer,
    RingBufferConfig, RingBufferEntry, Slot8,
};
use kaos::{consume_batch, publish_batch, publish_unrolled};

const RING_SIZE: usize = 1024 * 1024;
const BATCH_SIZE: usize = 8192;
const API_EVENTS: u64 = 1_000_000;

// ============================================================================
// Low-level batch API
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

// ============================================================================
// Macro API (publish_batch! / consume_batch!)
// ============================================================================

fn bench_macros(events: u64) -> u64 {
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
                    black_box(slot.value);
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
        let batch = ((events - cursor) as usize).min(BATCH_SIZE);
        if publish_batch!(Slot8, ring_prod, cursor, batch, i, slot, {
            slot.value = cursor + (i as u64);
        })
        .is_err()
        {
            std::hint::spin_loop();
        }
    }

    consumer.join().unwrap();
    events
}

// ============================================================================
// publish_unrolled! macro with BroadcastRingBuffer
// ============================================================================

fn bench_unrolled(events: u64) -> u64 {
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
            } else {
                std::hint::spin_loop();
            }
        }
    });

    let producer = ProducerBuilder::new()
        .with_ring_buffer(ring_buffer.clone())
        .build()
        .unwrap();

    let test_data = b"BENCHMARK_64_BYTES_PAYLOAD_XXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXX";
    let mut sent = 0u64;

    #[allow(unused_mut)]
    let mut producer = producer;
    while sent < events {
        match publish_unrolled!(producer, BATCH_SIZE, seq, i, slot, {
            slot.set_sequence(seq + (i as u64));
            slot.set_data(test_data);
        }) {
            Ok(count) => {
                sent += count as u64;
            }
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
    impl EventHandler<MessageSlot> for CountHandler {
        fn on_event(&mut self, _: &MessageSlot, _: u64, _: bool) {
            self.count.fetch_add(1, Ordering::Relaxed);
        }
    }

    let cons_count = consumed.clone();
    let cons_stop = stop.clone();
    let consumer_thread = thread::spawn(move || {
        let mut handler = CountHandler { count: cons_count };
        consumer.run_loop(&mut handler, &cons_stop);
    });

    let test_data = b"BENCHMARK_64_BYTES_PAYLOAD_XXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXX";
    let batch: Vec<&[u8]> = vec![test_data; 2048];
    let mut sent = 0u64;

    while sent < events {
        match producer.publish_batch(&batch, |event, seq, data| {
            event.set_sequence(seq);
            event.set_data(data);
        }) {
            Ok(count) => {
                sent += count as u64;
            }
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

fn benchmark_low_level(c: &mut Criterion) {
    let mut group = c.benchmark_group("Low-Level API (1M events)");
    group.throughput(Throughput::Elements(API_EVENTS));
    group.sample_size(20);

    group.bench_function(BenchmarkId::new("batch", "direct"), |b| {
        b.iter(|| bench_batch(API_EVENTS))
    });

    group.bench_function(BenchmarkId::new("batch", "macros"), |b| {
        b.iter(|| bench_macros(API_EVENTS))
    });

    group.finish();
}

fn benchmark_high_level(c: &mut Criterion) {
    let events = 100_000u64; // Smaller for high-level API (slower)
    let mut group = c.benchmark_group("High-Level API (100K events)");
    group.throughput(Throughput::Elements(events));
    group.sample_size(10);

    group.bench_function("publish_unrolled", |b| b.iter(|| bench_unrolled(events)));

    group.bench_function("producer_consumer", |b| {
        b.iter(|| bench_producer_consumer(events))
    });

    group.finish();
}

criterion_group!(benches, benchmark_low_level, benchmark_high_level);
criterion_main!(benches);

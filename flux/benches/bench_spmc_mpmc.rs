//! Benchmark SPMC and MPMC patterns with Criterion
//!
//! Tests multi-consumer and multi-producer patterns.

use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use std::sync::atomic::{AtomicU64, AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;

use flux::disruptor::{SmallSlot, SpmcRingBuffer, MpmcRingBuffer};

const RING_SIZE: usize = 1024 * 1024;
const BATCH_SIZE: usize = 8192;
const TOTAL_EVENTS: u64 = 10_000_000;

/// SPMC with single consumer (fast path)
fn bench_spmc_single_consumer(events: u64) -> u64 {
    let ring = Arc::new(SpmcRingBuffer::<SmallSlot>::new(RING_SIZE).unwrap());
    let producer_cursor = ring.producer_cursor();
    let received = Arc::new(AtomicU64::new(0));

    let ring_cons = ring.clone();
    let recv = received.clone();
    let consumer = thread::spawn(move || {
        let mut cursor = 0u64;
        while cursor < events {
            let slots = ring_cons.get_read_batch_fast(cursor, BATCH_SIZE);
            if !slots.is_empty() {
                cursor += slots.len() as u64;
                ring_cons.update_consumer_fast(cursor);
            } else {
                let prod_seq = producer_cursor.load(Ordering::Acquire);
                if prod_seq <= cursor {
                    std::hint::spin_loop();
                }
            }
        }
        recv.store(cursor, Ordering::Release);
    });

    let ring_prod = ring.clone();
    let mut cursor = 0u64;
    while cursor < events {
        let remaining = (events - cursor) as usize;
        let batch = remaining.min(BATCH_SIZE);
        if let Some(next) = ring_prod.try_claim(batch, cursor) {
            for i in 0..batch {
                unsafe {
                    let mut slot = SmallSlot::default();
                    slot.value = (cursor + i as u64) % 5 + 1;
                    ring_prod.write_slot(cursor + i as u64, slot);
                }
            }
            ring_prod.publish(next);
            cursor = next;
        }
    }

    consumer.join().unwrap();
    received.load(Ordering::Acquire)
}

/// MPMC with 2 producers, 1 consumer
fn bench_mpmc_2p1c(events: u64) -> u64 {
    let ring = Arc::new(MpmcRingBuffer::<SmallSlot>::new(RING_SIZE).unwrap());
    let events_per_producer = events / 2;
    let done = Arc::new(AtomicBool::new(false));
    let received = Arc::new(AtomicU64::new(0));

    // Consumer using try_read (guard-based API)
    let ring_cons = ring.clone();
    let done_cons = done.clone();
    let recv = received.clone();
    let consumer = thread::spawn(move || {
        let mut total = 0u64;
        while total < events || !done_cons.load(Ordering::Relaxed) {
            if let Some(guard) = ring_cons.try_read() {
                std::hint::black_box(guard.get().value);
                total += 1;
                // Guard auto-commits on drop
            } else {
                if done_cons.load(Ordering::Relaxed) && total >= events {
                    break;
                }
                std::hint::spin_loop();
            }
        }
        recv.store(total, Ordering::Release);
    });

    // Producers using low-level API
    let handles: Vec<_> = (0..2).map(|_| {
        let ring_prod = ring.clone();
        thread::spawn(move || {
            let mut sent = 0u64;
            while sent < events_per_producer {
                if let Some(seq) = ring_prod.try_claim(1) {
                    unsafe {
                        ring_prod.write_slot(seq, SmallSlot { value: sent % 5 + 1 });
                    }
                    ring_prod.publish(seq);
                    sent += 1;
                } else {
                    std::hint::spin_loop();
                }
            }
        })
    }).collect();

    for h in handles {
        h.join().unwrap();
    }
    done.store(true, Ordering::Release);
    consumer.join().unwrap();
    received.load(Ordering::Acquire)
}

fn benchmark_multi_patterns(c: &mut Criterion) {
    let mut group = c.benchmark_group("Multi-Producer/Consumer (10M events)");
    group.throughput(Throughput::Elements(TOTAL_EVENTS));
    group.sample_size(10);

    group.bench_function(BenchmarkId::new("pattern", "SPMC-1C"), |b| {
        b.iter(|| bench_spmc_single_consumer(TOTAL_EVENTS))
    });

    group.bench_function(BenchmarkId::new("pattern", "MPMC-2P1C"), |b| {
        b.iter(|| bench_mpmc_2p1c(TOTAL_EVENTS))
    });

    group.finish();
}

criterion_group!(benches, benchmark_multi_patterns);
criterion_main!(benches);

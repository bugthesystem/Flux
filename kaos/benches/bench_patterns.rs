//! Multi-Producer/Consumer pattern benchmarks
//!
//! Run: cargo bench --bench bench_patterns

use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::thread;

use kaos::disruptor::{MpmcRingBuffer, Slot8, SpmcRingBuffer};

const RING_SIZE: usize = 64 * 1024; // 64K slots
const BATCH_SIZE: usize = 64;
const TOTAL_EVENTS: u64 = 1_000_000; // 1M events

/// SPMC with single consumer
fn bench_spmc_1c(events: u64) -> u64 {
    let ring = Arc::new(SpmcRingBuffer::<Slot8>::new(RING_SIZE).unwrap());
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
                std::hint::spin_loop();
            }
        }
        recv.store(cursor, Ordering::Release);
    });

    // Producer
    let mut cursor = 0u64;
    while cursor < events {
        let batch = ((events - cursor) as usize).min(BATCH_SIZE);
        if let Some(next) = ring.try_claim(batch, cursor) {
            for i in 0..batch {
                unsafe {
                    ring.write_slot(cursor + i as u64, Slot8 { value: i as u64 });
                }
            }
            ring.publish(next);
            cursor = next;
        } else {
            std::hint::spin_loop();
        }
    }

    consumer.join().unwrap();
    received.load(Ordering::Acquire)
}

/// MPMC with 2 producers, 1 consumer
fn bench_mpmc_2p1c(events: u64) -> u64 {
    let ring = Arc::new(MpmcRingBuffer::<Slot8>::new(RING_SIZE).unwrap());
    let events_per_producer = events / 2;
    let produced = Arc::new(AtomicU64::new(0));
    let received = Arc::new(AtomicU64::new(0));

    // Consumer
    let ring_cons = ring.clone();
    let recv = received.clone();
    let prod_count = produced.clone();
    let consumer = thread::spawn(move || {
        let mut total = 0u64;
        loop {
            if let Some(guard) = ring_cons.try_read() {
                std::hint::black_box(guard.get().value);
                total += 1;
                if total >= events {
                    break;
                }
            } else {
                // Check if all produced and we've consumed all
                if prod_count.load(Ordering::Acquire) >= events && total >= events {
                    break;
                }
                std::hint::spin_loop();
            }
        }
        recv.store(total, Ordering::Release);
    });

    // 2 Producers
    let handles: Vec<_> = (0..2)
        .map(|_| {
            let ring_prod = ring.clone();
            let prod = produced.clone();
            thread::spawn(move || {
                for i in 0..events_per_producer {
                    loop {
                        if let Some(seq) = ring_prod.try_claim(1) {
                            unsafe {
                                ring_prod.write_slot(seq, Slot8 { value: i });
                            }
                            ring_prod.publish(seq);
                            prod.fetch_add(1, Ordering::Release);
                            break;
                        }
                        std::hint::spin_loop();
                    }
                }
            })
        })
        .collect();

    for h in handles {
        h.join().unwrap();
    }
    consumer.join().unwrap();
    received.load(Ordering::Acquire)
}

/// MPMC with batch claiming (producers batch, consumer single)
fn bench_mpmc_batch(events: u64) -> u64 {
    let ring = Arc::new(MpmcRingBuffer::<Slot8>::new(RING_SIZE).unwrap());
    let events_per_producer = events / 2;
    let produced = Arc::new(AtomicU64::new(0));
    let received = Arc::new(AtomicU64::new(0));

    // Consumer (single read - MPMC doesn't have batch read)
    let ring_cons = ring.clone();
    let recv = received.clone();
    let consumer = thread::spawn(move || {
        let mut total = 0u64;
        while total < events {
            if let Some(guard) = ring_cons.try_read() {
                std::hint::black_box(guard.get().value);
                total += 1;
            } else {
                std::hint::spin_loop();
            }
        }
        recv.store(total, Ordering::Release);
    });

    // 2 Producers with batch claims
    let handles: Vec<_> = (0..2)
        .map(|_| {
            let ring_prod = ring.clone();
            let prod = produced.clone();
            thread::spawn(move || {
                let mut sent = 0u64;
                while sent < events_per_producer {
                    let batch = ((events_per_producer - sent) as usize).min(BATCH_SIZE);
                    if let Some(seq) = ring_prod.try_claim(batch) {
                        for i in 0..batch {
                            unsafe {
                                ring_prod.write_slot(seq + i as u64, Slot8 { value: i as u64 });
                            }
                            ring_prod.publish(seq + i as u64);
                        }
                        sent += batch as u64;
                        prod.fetch_add(batch as u64, Ordering::Relaxed);
                    } else {
                        std::hint::spin_loop();
                    }
                }
            })
        })
        .collect();

    for h in handles {
        h.join().unwrap();
    }
    consumer.join().unwrap();
    received.load(Ordering::Acquire)
}

fn benchmark_patterns(c: &mut Criterion) {
    let mut group = c.benchmark_group("Multi-Pattern");
    group.throughput(Throughput::Elements(TOTAL_EVENTS));
    group.sample_size(10);

    group.bench_function(BenchmarkId::new("pattern", "SPMC-1C"), |b| {
        b.iter(|| bench_spmc_1c(TOTAL_EVENTS))
    });

    group.bench_function(BenchmarkId::new("pattern", "MPMC-2P1C-single"), |b| {
        b.iter(|| bench_mpmc_2p1c(TOTAL_EVENTS))
    });

    group.bench_function(BenchmarkId::new("pattern", "MPMC-2P1C-batch"), |b| {
        b.iter(|| bench_mpmc_batch(TOTAL_EVENTS))
    });

    group.finish();
}

criterion_group!(benches, benchmark_patterns);
criterion_main!(benches);

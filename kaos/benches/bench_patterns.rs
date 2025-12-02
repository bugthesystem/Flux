//! Multi-Producer/Consumer pattern benchmarks
//!
//! Tests SPMC and MPMC ring buffer patterns.
//!
//! Run: cargo bench --bench bench_patterns

use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::thread;

use kaos::disruptor::{MpmcRingBuffer, Slot8, SpmcRingBuffer};

const RING_SIZE: usize = 1024 * 1024;
const BATCH_SIZE: usize = 8192;
const TOTAL_EVENTS: u64 = 10_000_000;

// ============================================================================
// SPMC Benchmarks
// ============================================================================

/// SPMC with single consumer (fast path)
fn bench_spmc_1c(events: u64) -> u64 {
    let ring = Arc::new(SpmcRingBuffer::<Slot8>::new(RING_SIZE).unwrap());
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
                    let mut slot = Slot8::default();
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

/// SPMC with 2 consumers
fn bench_spmc_2c(events: u64) -> u64 {
    let ring = Arc::new(SpmcRingBuffer::<Slot8>::new(RING_SIZE).unwrap());
    let producer_cursor = ring.producer_cursor();
    let total_received = Arc::new(AtomicU64::new(0));
    let done = Arc::new(AtomicBool::new(false));

    // Spawn 2 consumers
    let handles: Vec<_> = (0..2)
        .map(|_| {
            let ring_cons = ring.clone();
            let recv = total_received.clone();
            let done_flag = done.clone();
            let prod_cursor = producer_cursor.clone();
            thread::spawn(move || {
                let mut local_count = 0u64;
                while !done_flag.load(Ordering::Relaxed) || prod_cursor.load(Ordering::Acquire) > 0 {
                    if let Some(guard) = ring_cons.try_read() {
                        std::hint::black_box(guard.get().value);
                        local_count += 1;
                        // Guard auto-commits on drop
                    } else {
                        if done_flag.load(Ordering::Relaxed) {
                            break;
                        }
                        std::hint::spin_loop();
                    }
                }
                recv.fetch_add(local_count, Ordering::Relaxed);
            })
        })
        .collect();

    let ring_prod = ring.clone();
    let mut cursor = 0u64;
    while cursor < events {
        let remaining = (events - cursor) as usize;
        let batch = remaining.min(BATCH_SIZE);
        if let Some(next) = ring_prod.try_claim(batch, cursor) {
            for i in 0..batch {
                unsafe {
                    let mut slot = Slot8::default();
                    slot.value = (cursor + i as u64) % 5 + 1;
                    ring_prod.write_slot(cursor + i as u64, slot);
                }
            }
            ring_prod.publish(next);
            cursor = next;
        }
    }

    // Wait for consumers to catch up
    while total_received.load(Ordering::Relaxed) < events {
        std::thread::yield_now();
    }

    done.store(true, Ordering::Release);
    for h in handles {
        h.join().unwrap();
    }

    total_received.load(Ordering::Acquire)
}

// ============================================================================
// MPMC Benchmarks
// ============================================================================

/// MPMC with 2 producers, 1 consumer
fn bench_mpmc_2p1c(events: u64) -> u64 {
    let ring = Arc::new(MpmcRingBuffer::<Slot8>::new(RING_SIZE).unwrap());
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
    let handles: Vec<_> = (0..2)
        .map(|_| {
            let ring_prod = ring.clone();
            thread::spawn(move || {
                let mut sent = 0u64;
                while sent < events_per_producer {
                    if let Some(seq) = ring_prod.try_claim(1) {
                        unsafe {
                            ring_prod.write_slot(seq, Slot8 { value: sent % 5 + 1 });
                        }
                        ring_prod.publish(seq);
                        sent += 1;
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
    done.store(true, Ordering::Release);
    consumer.join().unwrap();
    received.load(Ordering::Acquire)
}

/// MPMC with 4 producers, 2 consumers
fn bench_mpmc_4p2c(events: u64) -> u64 {
    let ring = Arc::new(MpmcRingBuffer::<Slot8>::new(RING_SIZE).unwrap());
    let events_per_producer = events / 4;
    let done = Arc::new(AtomicBool::new(false));
    let received = Arc::new(AtomicU64::new(0));

    // 2 Consumers
    let consumer_handles: Vec<_> = (0..2)
        .map(|_| {
            let ring_cons = ring.clone();
            let done_cons = done.clone();
            let recv = received.clone();
            thread::spawn(move || {
                let mut local = 0u64;
                loop {
                    if let Some(guard) = ring_cons.try_read() {
                        std::hint::black_box(guard.get().value);
                        local += 1;
                    } else {
                        if done_cons.load(Ordering::Relaxed) {
                            break;
                        }
                        std::hint::spin_loop();
                    }
                }
                recv.fetch_add(local, Ordering::Relaxed);
            })
        })
        .collect();

    // 4 Producers
    let producer_handles: Vec<_> = (0..4)
        .map(|_| {
            let ring_prod = ring.clone();
            thread::spawn(move || {
                let mut sent = 0u64;
                while sent < events_per_producer {
                    if let Some(seq) = ring_prod.try_claim(1) {
                        unsafe {
                            ring_prod.write_slot(seq, Slot8 { value: sent % 5 + 1 });
                        }
                        ring_prod.publish(seq);
                        sent += 1;
                    } else {
                        std::hint::spin_loop();
                    }
                }
            })
        })
        .collect();

    for h in producer_handles {
        h.join().unwrap();
    }

    // Wait for consumers
    while received.load(Ordering::Relaxed) < events {
        std::thread::yield_now();
    }

    done.store(true, Ordering::Release);
    for h in consumer_handles {
        h.join().unwrap();
    }

    received.load(Ordering::Acquire)
}

// ============================================================================
// Benchmark Groups
// ============================================================================

fn benchmark_patterns(c: &mut Criterion) {
    let mut group = c.benchmark_group("Multi-Pattern (10M events)");
    group.throughput(Throughput::Elements(TOTAL_EVENTS));
    group.sample_size(10);

    group.bench_function(BenchmarkId::new("pattern", "SPMC-1C"), |b| {
        b.iter(|| bench_spmc_1c(TOTAL_EVENTS))
    });

    group.bench_function(BenchmarkId::new("pattern", "SPMC-2C"), |b| {
        b.iter(|| bench_spmc_2c(TOTAL_EVENTS))
    });

    group.bench_function(BenchmarkId::new("pattern", "MPMC-2P1C"), |b| {
        b.iter(|| bench_mpmc_2p1c(TOTAL_EVENTS))
    });

    group.bench_function(BenchmarkId::new("pattern", "MPMC-4P2C"), |b| {
        b.iter(|| bench_mpmc_4p2c(TOTAL_EVENTS))
    });

    group.finish();
}

criterion_group!(benches, benchmark_patterns);
criterion_main!(benches);


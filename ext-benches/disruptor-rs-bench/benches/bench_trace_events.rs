//! Trace Events Comparison: Kaos vs disruptor-rs
//!
//! Real-world benchmark simulating mobile user interaction tracking.
//! Same workload for both libraries for fair comparison.

use criterion::{criterion_group, criterion_main, Criterion, Throughput};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::thread;

// Event types (mobile user interactions)
const EVENT_CLICK: u64 = 1;
const EVENT_SCROLL: u64 = 2;
const EVENT_PAGEVIEW: u64 = 3;
const EVENT_PURCHASE: u64 = 4;
const EVENT_LOGIN: u64 = 5;

const EVENTS_PER_TYPE: u64 = 20_000_000; // 20M per type = 100M total
const TOTAL_EVENTS: u64 = EVENTS_PER_TYPE * 5;
const RING_SIZE: usize = 1024 * 1024;
const BATCH_SIZE: usize = 8192;

/// Kaos implementation - using batch API
fn run_kaos_trace_events() -> (u64, bool) {
    use kaos::disruptor::{RingBuffer, Slot8};

    let ring = Arc::new(RingBuffer::<Slot8>::new(RING_SIZE).unwrap());
    let producer_cursor = ring.producer_cursor();

    let count_click = Arc::new(AtomicU64::new(0));
    let count_scroll = Arc::new(AtomicU64::new(0));
    let count_pageview = Arc::new(AtomicU64::new(0));
    let count_purchase = Arc::new(AtomicU64::new(0));
    let count_login = Arc::new(AtomicU64::new(0));

    let ring_cons = ring.clone();
    let c_click = count_click.clone();
    let c_scroll = count_scroll.clone();
    let c_pageview = count_pageview.clone();
    let c_purchase = count_purchase.clone();
    let c_login = count_login.clone();

    let consumer = thread::spawn(move || {
        let mut cursor = 0u64;
        let mut local_click = 0u64;
        let mut local_scroll = 0u64;
        let mut local_pageview = 0u64;
        let mut local_purchase = 0u64;
        let mut local_login = 0u64;

        while cursor < TOTAL_EVENTS {
            let prod_seq = producer_cursor.load(Ordering::Acquire);
            let available = prod_seq.saturating_sub(cursor);

            if available > 0 {
                let batch = (available as usize).min(BATCH_SIZE);
                let slots = ring_cons.get_read_batch(cursor, batch);

                for slot in slots {
                    match slot.value {
                        EVENT_CLICK => local_click += 1,
                        EVENT_SCROLL => local_scroll += 1,
                        EVENT_PAGEVIEW => local_pageview += 1,
                        EVENT_PURCHASE => local_purchase += 1,
                        EVENT_LOGIN => local_login += 1,
                        _ => {}
                    }
                }

                cursor += slots.len() as u64;
                ring_cons.update_consumer(cursor);
            } else {
                std::hint::spin_loop();
            }
        }

        c_click.store(local_click, Ordering::Release);
        c_scroll.store(local_scroll, Ordering::Release);
        c_pageview.store(local_pageview, Ordering::Release);
        c_purchase.store(local_purchase, Ordering::Release);
        c_login.store(local_login, Ordering::Release);
    });

    // Producer
    let ring_prod = ring.clone();
    let mut cursor = 0u64;
    let mut event_type = EVENT_CLICK;

    while cursor < TOTAL_EVENTS {
        let remaining = (TOTAL_EVENTS - cursor) as usize;
        let batch = remaining.min(BATCH_SIZE);

        if let Some((seq, slots)) = ring_prod.try_claim_slots(batch, cursor) {
            for slot in slots.iter_mut() {
                slot.value = event_type;
                event_type = if event_type >= EVENT_LOGIN { EVENT_CLICK } else { event_type + 1 };
            }
            ring_prod.publish(seq + slots.len() as u64);
            cursor = seq + slots.len() as u64;
        }
    }

    consumer.join().unwrap();

    let counts = [
        count_click.load(Ordering::Relaxed),
        count_scroll.load(Ordering::Relaxed),
        count_pageview.load(Ordering::Relaxed),
        count_purchase.load(Ordering::Relaxed),
        count_login.load(Ordering::Relaxed),
    ];

    let total: u64 = counts.iter().sum();
    let verified = total == TOTAL_EVENTS && counts.iter().all(|&c| c == EVENTS_PER_TYPE);

    (total, verified)
}

/// disruptor-rs implementation - using their event handler pattern
fn run_disruptor_trace_events() -> (u64, bool) {
    use disruptor::*;

    let count_click = Arc::new(AtomicU64::new(0));
    let count_scroll = Arc::new(AtomicU64::new(0));
    let count_pageview = Arc::new(AtomicU64::new(0));
    let count_purchase = Arc::new(AtomicU64::new(0));
    let count_login = Arc::new(AtomicU64::new(0));
    let total_processed = Arc::new(AtomicU64::new(0));

    let c_click = count_click.clone();
    let c_scroll = count_scroll.clone();
    let c_pageview = count_pageview.clone();
    let c_purchase = count_purchase.clone();
    let c_login = count_login.clone();
    let processed = total_processed.clone();

    let factory = || 0u64;

    let mut producer = build_single_producer(RING_SIZE, factory, BusySpin)
        .handle_events_with(move |event: &u64, _seq: Sequence, _eob: bool| {
            match *event {
                EVENT_CLICK => { c_click.fetch_add(1, Ordering::Relaxed); }
                EVENT_SCROLL => { c_scroll.fetch_add(1, Ordering::Relaxed); }
                EVENT_PAGEVIEW => { c_pageview.fetch_add(1, Ordering::Relaxed); }
                EVENT_PURCHASE => { c_purchase.fetch_add(1, Ordering::Relaxed); }
                EVENT_LOGIN => { c_login.fetch_add(1, Ordering::Relaxed); }
                _ => {}
            }
            processed.fetch_add(1, Ordering::Relaxed);
        })
        .build();

    // Publish all events
    let mut event_type = EVENT_CLICK;
    for _ in 0..TOTAL_EVENTS {
        let et = event_type;
        producer.publish(|slot| {
            *slot = et;
        });
        event_type = if event_type >= EVENT_LOGIN { EVENT_CLICK } else { event_type + 1 };
    }

    // Wait for consumer to process all events
    while total_processed.load(Ordering::Relaxed) < TOTAL_EVENTS {
        std::hint::spin_loop();
    }

    let counts = [
        count_click.load(Ordering::Relaxed),
        count_scroll.load(Ordering::Relaxed),
        count_pageview.load(Ordering::Relaxed),
        count_purchase.load(Ordering::Relaxed),
        count_login.load(Ordering::Relaxed),
    ];

    let total: u64 = counts.iter().sum();
    let verified = total == TOTAL_EVENTS && counts.iter().all(|&c| c == EVENTS_PER_TYPE);

    (total, verified)
}

fn benchmark_trace_events(c: &mut Criterion) {
    let mut group = c.benchmark_group("Trace Events (100M)");
    group.throughput(Throughput::Elements(TOTAL_EVENTS));
    group.sample_size(10);

    group.bench_function("kaos", |b| {
        b.iter(|| {
            let (total, verified) = run_kaos_trace_events();
            assert!(verified, "Kaos verification failed! Got {} events", total);
            total
        })
    });

    group.bench_function("disruptor-rs", |b| {
        b.iter(|| {
            let (total, verified) = run_disruptor_trace_events();
            assert!(verified, "disruptor-rs verification failed! Got {} events", total);
            total
        })
    });

    group.finish();
}

criterion_group!(benches, benchmark_trace_events);
criterion_main!(benches);


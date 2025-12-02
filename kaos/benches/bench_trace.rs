//! Real-world benchmark: Mobile User Interaction Tracking
//!
//! Simulates a production observability system tracking user events:
//! - Click, Scroll, PageView, Purchase, Login
//!
//! Parameterized for quick checks (100M) to full stress tests (5B).
//!
//! Run quick:  cargo bench --bench bench_trace -- "100M"
//! Run medium: cargo bench --bench bench_trace -- "500M"
//! Run full:   cargo bench --bench bench_trace -- "1B"

use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::thread;

use kaos::disruptor::{RingBuffer, RingBufferEntry, Slot8, Slot32, Slot64};

const RING_SIZE: usize = 1024 * 1024;
const BATCH_SIZE: usize = 8192;

// Event types (mobile user interactions)
const EVENT_CLICK: u64 = 1;
const EVENT_SCROLL: u64 = 2;
const EVENT_PAGEVIEW: u64 = 3;
const EVENT_PURCHASE: u64 = 4;
const EVENT_LOGIN: u64 = 5;

// Parameterized event counts
const EVENTS_100M: u64 = 100_000_000;   // ~2 sec - quick check
const EVENTS_500M: u64 = 500_000_000;   // ~10 sec - medium
const EVENTS_1B: u64 = 1_000_000_000;   // ~20 sec - full test

/// Run observability benchmark with specified slot type and event count
fn run_bench<T: RingBufferEntry + Copy + Default + Send + Sync + 'static>(
    total_events: u64,
    use_mapped: bool,
) -> u64 {
    let events_per_type = total_events / 5;
    
    let ring = if use_mapped {
        Arc::new(RingBuffer::<T>::new_mapped(RING_SIZE).unwrap())
    } else {
        Arc::new(RingBuffer::<T>::new(RING_SIZE).unwrap())
    };
    let producer_cursor = ring.producer_cursor();

    // Consumer counters
    let count_click = Arc::new(AtomicU64::new(0));
    let count_scroll = Arc::new(AtomicU64::new(0));
    let count_pageview = Arc::new(AtomicU64::new(0));
    let count_purchase = Arc::new(AtomicU64::new(0));
    let count_login = Arc::new(AtomicU64::new(0));

    // Consumer thread
    let ring_cons = ring.clone();
    let c_click = count_click.clone();
    let c_scroll = count_scroll.clone();
    let c_pageview = count_pageview.clone();
    let c_purchase = count_purchase.clone();
    let c_login = count_login.clone();

    let consumer = thread::spawn(move || {
        let mut local_click = 0u64;
        let mut local_scroll = 0u64;
        let mut local_pageview = 0u64;
        let mut local_purchase = 0u64;
        let mut local_login = 0u64;
        let mut cursor = 0u64;

        while cursor < total_events {
            let prod_seq = producer_cursor.load(Ordering::Acquire);
            let available = prod_seq.saturating_sub(cursor);

            if available > 0 {
                let batch = (available as usize).min(BATCH_SIZE);
                let slots = ring_cons.get_read_batch(cursor, batch);

                for slot in slots {
                    match slot.sequence() {
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

    // Producer thread
    let ring_prod = ring.clone();
    let producer = thread::spawn(move || {
        let mut event_type = EVENT_CLICK;
        let mut cursor = 0u64;
        let mut sent = 0u64;

        while sent < total_events {
            let remaining = (total_events - sent) as usize;
            let batch = remaining.min(BATCH_SIZE);

            if let Some((seq, slots)) = ring_prod.try_claim_slots(batch, cursor) {
                for slot in slots.iter_mut() {
                    slot.set_sequence(event_type);
                    event_type = if event_type >= EVENT_LOGIN { EVENT_CLICK } else { event_type + 1 };
                }

                let next = seq + (slots.len() as u64);
                ring_prod.publish(next);
                cursor = next;
                sent += slots.len() as u64;
            }
        }
    });

    producer.join().unwrap();
    consumer.join().unwrap();

    // Verify
    let counts = [
        count_click.load(Ordering::Relaxed),
        count_scroll.load(Ordering::Relaxed),
        count_pageview.load(Ordering::Relaxed),
        count_purchase.load(Ordering::Relaxed),
        count_login.load(Ordering::Relaxed),
    ];

    let total: u64 = counts.iter().sum();
    let verified = total == total_events && counts.iter().all(|&c| c == events_per_type);
    assert!(verified, "Data integrity failed! Expected {} per type, got {:?}", events_per_type, counts);

    total_events
}

// ============================================================================
// Quick Check: 100M events (~2 seconds)
// ============================================================================

fn benchmark_100m(c: &mut Criterion) {
    let mut group = c.benchmark_group("Trace 100M (quick)");
    group.throughput(Throughput::Elements(EVENTS_100M));
    group.sample_size(10);

    group.bench_function(BenchmarkId::new("heap", "8B"), |b| {
        b.iter(|| run_bench::<Slot8>(EVENTS_100M, false))
    });

    group.bench_function(BenchmarkId::new("heap", "32B"), |b| {
        b.iter(|| run_bench::<Slot32>(EVENTS_100M, false))
    });

    group.finish();
}

// ============================================================================
// Medium: 500M events (~10 seconds)
// ============================================================================

fn benchmark_500m(c: &mut Criterion) {
    let mut group = c.benchmark_group("Trace 500M (medium)");
    group.throughput(Throughput::Elements(EVENTS_500M));
    group.sample_size(10);

    group.bench_function(BenchmarkId::new("heap", "8B"), |b| {
        b.iter(|| run_bench::<Slot8>(EVENTS_500M, false))
    });

    group.bench_function(BenchmarkId::new("mapped", "8B"), |b| {
        b.iter(|| run_bench::<Slot8>(EVENTS_500M, true))
    });

    group.finish();
}

// ============================================================================
// Full: 1B events (~20 seconds)
// ============================================================================

fn benchmark_1b(c: &mut Criterion) {
    let mut group = c.benchmark_group("Trace 1B (full)");
    group.throughput(Throughput::Elements(EVENTS_1B));
    group.sample_size(10);

    group.bench_function(BenchmarkId::new("heap", "8B"), |b| {
        b.iter(|| run_bench::<Slot8>(EVENTS_1B, false))
    });

    group.bench_function(BenchmarkId::new("heap", "32B"), |b| {
        b.iter(|| run_bench::<Slot32>(EVENTS_1B, false))
    });

    group.bench_function(BenchmarkId::new("heap", "64B"), |b| {
        b.iter(|| run_bench::<Slot64>(EVENTS_1B, false))
    });

    group.bench_function(BenchmarkId::new("mapped", "8B"), |b| {
        b.iter(|| run_bench::<Slot8>(EVENTS_1B, true))
    });

    group.bench_function(BenchmarkId::new("mapped", "32B"), |b| {
        b.iter(|| run_bench::<Slot32>(EVENTS_1B, true))
    });

    group.finish();
}

criterion_group!(benches, benchmark_100m, benchmark_500m, benchmark_1b);
criterion_main!(benches);

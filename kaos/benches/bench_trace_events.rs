//! Real-world benchmark: Mobile User Interaction Tracking
//!
//! Simulates a production observability system tracking user events:
//! - Click, Scroll, PageView, Purchase, Login
//!
//! Verifies exact event counts to ensure no data loss.

use criterion::{ criterion_group, criterion_main, BenchmarkId, Criterion, Throughput };
use std::sync::atomic::{ AtomicU64, Ordering };
use std::sync::Arc;
use std::thread;
use std::time::Instant;

use kaos::disruptor::{ RingBuffer, RingBufferEntry, Slot8, Slot32, Slot64, MessageSlot };

const RING_SIZE: usize = 1024 * 1024;
const BATCH_SIZE: usize = 8192;

// Event types (mobile user interactions)
const EVENT_CLICK: u64 = 1;
const EVENT_SCROLL: u64 = 2;
const EVENT_PAGEVIEW: u64 = 3;
const EVENT_PURCHASE: u64 = 4;
const EVENT_LOGIN: u64 = 5;

const EVENTS_PER_TYPE: u64 = 1_000_000_000; // 1B per type = 5B total
const TOTAL_EVENTS: u64 = EVENTS_PER_TYPE * 5;

/// Trace event with timestamp (fits in Slot32)
/// Can be used for custom slot types in production.
#[repr(C)]
#[derive(Clone, Copy, Default)]
#[allow(dead_code)]
struct TraceEvent {
    event_type: u64,
    timestamp_ns: u64,
    user_id: u64,
    _padding: u64,
}

/// Benchmark result with verification
#[allow(dead_code)]
struct BenchResult {
    throughput_m_per_sec: f64,
    duration_secs: f64,
    counts: [u64; 5],
    verified: bool,
}

/// Run observability benchmark with specified slot type
fn run_observability_bench<T: RingBufferEntry + Copy + Default + Send + Sync + 'static>(
    use_mapped: bool
) -> BenchResult {
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

    let start = Instant::now();

    // Consumer thread - processes events and counts by type
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

        while cursor < TOTAL_EVENTS {
            let prod_seq = producer_cursor.load(Ordering::Acquire);
            let available = prod_seq.saturating_sub(cursor);

            if available > 0 {
                let batch = (available as usize).min(BATCH_SIZE);
                let slots = ring_cons.get_read_batch(cursor, batch);

                for slot in slots {
                    // Event type stored in sequence field
                    match slot.sequence() {
                        EVENT_CLICK => {
                            local_click += 1;
                        }
                        EVENT_SCROLL => {
                            local_scroll += 1;
                        }
                        EVENT_PAGEVIEW => {
                            local_pageview += 1;
                        }
                        EVENT_PURCHASE => {
                            local_purchase += 1;
                        }
                        EVENT_LOGIN => {
                            local_login += 1;
                        }
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

    // Producer thread - generates events with timestamps
    let ring_prod = ring.clone();
    let producer = thread::spawn(move || {
        let mut event_type = EVENT_CLICK;
        let mut cursor = 0u64;
        let mut sent = 0u64;

        while sent < TOTAL_EVENTS {
            let remaining = (TOTAL_EVENTS - sent) as usize;
            let batch = remaining.min(BATCH_SIZE);

            if let Some((seq, slots)) = ring_prod.try_claim_slots(batch, cursor) {
                for slot in slots.iter_mut() {
                    // Store event type in sequence
                    slot.set_sequence(event_type);
                    // Cycle through event types
                    event_type = if event_type >= EVENT_LOGIN {
                        EVENT_CLICK
                    } else {
                        event_type + 1
                    };
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

    let duration = start.elapsed().as_secs_f64();
    let counts = [
        count_click.load(Ordering::Relaxed),
        count_scroll.load(Ordering::Relaxed),
        count_pageview.load(Ordering::Relaxed),
        count_purchase.load(Ordering::Relaxed),
        count_login.load(Ordering::Relaxed),
    ];

    let total: u64 = counts.iter().sum();
    let throughput = (total as f64) / duration / 1_000_000.0;
    let verified = total == TOTAL_EVENTS && counts.iter().all(|&c| c == EVENTS_PER_TYPE);

    BenchResult {
        throughput_m_per_sec: throughput,
        duration_secs: duration,
        counts,
        verified,
    }
}

// Criterion benchmark wrappers
fn bench_smallslot_heap() -> u64 {
    let result = run_observability_bench::<Slot8>(false);
    assert!(result.verified, "Data integrity check failed!");
    TOTAL_EVENTS
}

fn bench_slot32_heap() -> u64 {
    let result = run_observability_bench::<Slot32>(false);
    assert!(result.verified, "Data integrity check failed!");
    TOTAL_EVENTS
}

fn bench_slot64_heap() -> u64 {
    let result = run_observability_bench::<Slot64>(false);
    assert!(result.verified, "Data integrity check failed!");
    TOTAL_EVENTS
}

fn bench_smallslot_mapped() -> u64 {
    let result = run_observability_bench::<Slot8>(true);
    assert!(result.verified, "Data integrity check failed!");
    TOTAL_EVENTS
}

fn bench_slot32_mapped() -> u64 {
    let result = run_observability_bench::<Slot32>(true);
    assert!(result.verified, "Data integrity check failed!");
    TOTAL_EVENTS
}

fn benchmark_observability(c: &mut Criterion) {
    let mut group = c.benchmark_group("Observability (5B events, 5 types)");
    group.throughput(Throughput::Elements(TOTAL_EVENTS));
    group.sample_size(10);

    // Heap allocation
    group.bench_function(BenchmarkId::new("heap", "8B"), |b| { b.iter(bench_smallslot_heap) });

    group.bench_function(BenchmarkId::new("heap", "32B"), |b| { b.iter(bench_slot32_heap) });

    group.bench_function(BenchmarkId::new("heap", "64B"), |b| { b.iter(bench_slot64_heap) });

    // Mapped allocation (mlock'd)
    group.bench_function(BenchmarkId::new("mapped", "8B"), |b| { b.iter(bench_smallslot_mapped) });

    group.bench_function(BenchmarkId::new("mapped", "32B"), |b| { b.iter(bench_slot32_mapped) });

    group.finish();
}

criterion_group!(benches, benchmark_observability);
criterion_main!(benches);

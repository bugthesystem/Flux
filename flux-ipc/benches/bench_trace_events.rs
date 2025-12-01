//! Real-world benchmark: Mobile User Interaction Tracking (IPC)
//!
//! Simulates a production observability system tracking user events
//! across process boundaries using shared memory.

use criterion::{ criterion_group, criterion_main, Criterion, Throughput };
use flux_ipc::{ Publisher, Subscriber, SmallSlot };
use std::sync::atomic::{ AtomicBool, AtomicU64, Ordering };
use std::sync::Arc;
use std::thread;
use std::time::Instant;
use std::fs;

const TEST_PATH: &str = "/tmp/flux-ipc-trace-bench";

// Event types (mobile user interactions)
const EVENT_CLICK: u64 = 1;
const EVENT_SCROLL: u64 = 2;
const EVENT_PAGEVIEW: u64 = 3;
const EVENT_PURCHASE: u64 = 4;
const EVENT_LOGIN: u64 = 5;

const EVENTS_PER_TYPE: u64 = 200_000_000; // 200M per type = 1B total
const TOTAL_EVENTS: u64 = EVENTS_PER_TYPE * 5;

fn cleanup() {
    let _ = fs::remove_file(TEST_PATH);
}

fn run_ipc_trace_bench() -> (f64, bool) {
    cleanup();

    let running = Arc::new(AtomicBool::new(true));
    let count_click = Arc::new(AtomicU64::new(0));
    let count_scroll = Arc::new(AtomicU64::new(0));
    let count_pageview = Arc::new(AtomicU64::new(0));
    let count_purchase = Arc::new(AtomicU64::new(0));
    let count_login = Arc::new(AtomicU64::new(0));

    let start = Instant::now();

    // Publisher thread (simulates producer process)
    let running_pub = running.clone();
    let publisher = thread::spawn(move || {
        let mut pub_handle = Publisher::<SmallSlot>::new(TEST_PATH, 64 * 1024).unwrap();
        let mut event_type = EVENT_CLICK;
        let mut sent = 0u64;

        while sent < TOTAL_EVENTS && running_pub.load(Ordering::Relaxed) {
            if pub_handle.send(&event_type.to_le_bytes()).is_ok() {
                sent += 1;
                event_type = if event_type >= EVENT_LOGIN { EVENT_CLICK } else { event_type + 1 };
            }
        }
        sent
    });

    // Give publisher time to create file
    thread::sleep(std::time::Duration::from_millis(10));

    // Subscriber thread (simulates consumer process)
    let running_sub = running.clone();
    let c_click = count_click.clone();
    let c_scroll = count_scroll.clone();
    let c_pageview = count_pageview.clone();
    let c_purchase = count_purchase.clone();
    let c_login = count_login.clone();

    let subscriber = thread::spawn(move || {
        let mut sub_handle = Subscriber::<SmallSlot>::new(TEST_PATH).unwrap();
        let mut local_click = 0u64;
        let mut local_scroll = 0u64;
        let mut local_pageview = 0u64;
        let mut local_purchase = 0u64;
        let mut local_login = 0u64;
        let mut received = 0u64;

        while received < TOTAL_EVENTS && running_sub.load(Ordering::Relaxed) {
            received += sub_handle.receive(|slot| {
                match slot.value {
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
            }) as u64;
        }

        c_click.store(local_click, Ordering::Release);
        c_scroll.store(local_scroll, Ordering::Release);
        c_pageview.store(local_pageview, Ordering::Release);
        c_purchase.store(local_purchase, Ordering::Release);
        c_login.store(local_login, Ordering::Release);
        received
    });

    let sent = publisher.join().unwrap();
    running.store(false, Ordering::SeqCst);
    let received = subscriber.join().unwrap();

    let duration = start.elapsed().as_secs_f64();
    let throughput = (received as f64) / duration / 1_000_000.0;

    let counts = [
        count_click.load(Ordering::Relaxed),
        count_scroll.load(Ordering::Relaxed),
        count_pageview.load(Ordering::Relaxed),
        count_purchase.load(Ordering::Relaxed),
        count_login.load(Ordering::Relaxed),
    ];

    let total: u64 = counts.iter().sum();
    let verified = total == TOTAL_EVENTS && counts.iter().all(|&c| c == EVENTS_PER_TYPE);

    cleanup();
    (throughput, verified)
}

fn benchmark_ipc_trace(c: &mut Criterion) {
    let mut group = c.benchmark_group("IPC Trace Events (1B events)");
    group.throughput(Throughput::Elements(TOTAL_EVENTS));
    group.sample_size(10);

    group.bench_function("shared_memory", |b| {
        b.iter(|| {
            let (throughput, verified) = run_ipc_trace_bench();
            assert!(verified, "Data integrity check failed!");
            TOTAL_EVENTS
        })
    });

    group.finish();
}

criterion_group!(benches, benchmark_ipc_trace);
criterion_main!(benches);

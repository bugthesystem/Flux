//! kaos-ipc benchmarks - IPC throughput tests

use criterion::{black_box, criterion_group, criterion_main, Criterion, Throughput};
use kaos_ipc::{Publisher, Slot8, Subscriber};
use std::fs;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::thread;

const RING_SIZE: usize = 64 * 1024;

// Event types for trace benchmark
const EVENT_CLICK: u64 = 1;
const EVENT_SCROLL: u64 = 2;
const EVENT_PAGEVIEW: u64 = 3;
const EVENT_PURCHASE: u64 = 4;
const EVENT_LOGIN: u64 = 5;

fn cleanup(path: &str) {
    let _ = fs::remove_file(path);
}

fn bench_single_msg(c: &mut Criterion) {
    let path = "/tmp/kaos-ipc-bench-single";
    cleanup(path);

    let mut group = c.benchmark_group("IPC Single");
    group.throughput(Throughput::Elements(1));

    group.bench_function("send_8B", |b| {
        let mut publisher = Publisher::<Slot8>::new(path, RING_SIZE).unwrap();
        let mut subscriber = Subscriber::<Slot8>::new(path).unwrap();
        b.iter(|| {
            let _ = publisher.send(black_box(&[1u8; 8]));
            subscriber.receive(|_| {});
        });
    });

    group.finish();
    cleanup(path);
}

fn bench_sustained(c: &mut Criterion) {
    let path = "/tmp/kaos-ipc-bench-sustained";
    cleanup(path);

    let mut group = c.benchmark_group("IPC Sustained");
    const EVENTS: usize = 100_000;
    group.throughput(Throughput::Elements(EVENTS as u64));
    group.sample_size(20);

    group.bench_function("100K_msgs", |b| {
        let mut publisher = Publisher::<Slot8>::new(path, RING_SIZE).unwrap();
        let mut subscriber = Subscriber::<Slot8>::new(path).unwrap();
        b.iter(|| {
            let mut sent = 0;
            let mut received = 0;
            while received < EVENTS {
                while sent < EVENTS && sent - received < RING_SIZE - 1024 {
                    if publisher.send(black_box(&[1u8; 8])).is_ok() {
                        sent += 1;
                    }
                }
                received += subscriber.receive(|m| {
                    black_box(m);
                });
            }
        });
    });

    group.finish();
    cleanup(path);
}

fn bench_trace_events(c: &mut Criterion) {
    let path = "/tmp/kaos-ipc-trace-bench";
    cleanup(path);

    const EVENTS_PER_TYPE: u64 = 20_000_000; // 20M per type = 100M total (reduced for faster CI)
    const TOTAL_EVENTS: u64 = EVENTS_PER_TYPE * 5;

    let mut group = c.benchmark_group("IPC Trace (100M events)");
    group.throughput(Throughput::Elements(TOTAL_EVENTS));
    group.sample_size(10);

    group.bench_function("shared_memory", |b| {
        b.iter(|| {
            cleanup(path);
            let running = Arc::new(AtomicBool::new(true));
            let counts: [Arc<AtomicU64>; 5] = [
                Arc::new(AtomicU64::new(0)),
                Arc::new(AtomicU64::new(0)),
                Arc::new(AtomicU64::new(0)),
                Arc::new(AtomicU64::new(0)),
                Arc::new(AtomicU64::new(0)),
            ];

            let running_pub = running.clone();
            let publisher = thread::spawn(move || {
                let mut pub_handle = Publisher::<Slot8>::new(path, RING_SIZE).unwrap();
                let mut event_type = EVENT_CLICK;
                let mut sent = 0u64;
                while sent < TOTAL_EVENTS && running_pub.load(Ordering::Relaxed) {
                    if pub_handle.send(&event_type.to_le_bytes()).is_ok() {
                        sent += 1;
                        event_type = if event_type >= EVENT_LOGIN {
                            EVENT_CLICK
                        } else {
                            event_type + 1
                        };
                    }
                }
                sent
            });

            thread::sleep(std::time::Duration::from_millis(10));

            let running_sub = running.clone();
            let c = counts.clone();
            let subscriber = thread::spawn(move || {
                let mut sub_handle = Subscriber::<Slot8>::new(path).unwrap();
                let mut local = [0u64; 5];
                let mut received = 0u64;
                while received < TOTAL_EVENTS && running_sub.load(Ordering::Relaxed) {
                    received += sub_handle.receive(|slot| match slot.value {
                        EVENT_CLICK => {
                            local[0] += 1;
                        }
                        EVENT_SCROLL => {
                            local[1] += 1;
                        }
                        EVENT_PAGEVIEW => {
                            local[2] += 1;
                        }
                        EVENT_PURCHASE => {
                            local[3] += 1;
                        }
                        EVENT_LOGIN => {
                            local[4] += 1;
                        }
                        _ => {}
                    }) as u64;
                }
                for i in 0..5 {
                    c[i].store(local[i], Ordering::Release);
                }
                received
            });

            publisher.join().unwrap();
            running.store(false, Ordering::SeqCst);
            subscriber.join().unwrap();

            let total: u64 = counts.iter().map(|c| c.load(Ordering::Relaxed)).sum();
            assert!(total >= (TOTAL_EVENTS * 95) / 100, "Lost >5%");
            cleanup(path);
            TOTAL_EVENTS
        })
    });

    group.finish();
}

criterion_group!(
    benches,
    bench_single_msg,
    bench_sustained,
    bench_trace_events
);
criterion_main!(benches);

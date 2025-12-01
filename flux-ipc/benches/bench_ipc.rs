//! flux-ipc benchmark
//!
//! Tests throughput using flux's SharedRingBuffer.

use criterion::{ black_box, criterion_group, criterion_main, Criterion, Throughput };
use flux_ipc::{ Publisher, Subscriber, SmallSlot };
use std::fs;

const RING_SIZE: usize = 64 * 1024;

fn bench_throughput(c: &mut Criterion) {
    let path = "/tmp/flux-ipc-bench";
    let _ = fs::remove_file(path);

    let mut group = c.benchmark_group("flux-ipc");
    group.throughput(Throughput::Elements(1));

    group.bench_function("send_8B", |b| {
        let mut publisher = Publisher::<SmallSlot>::new(path, RING_SIZE).unwrap();
        let mut subscriber = Subscriber::<SmallSlot>::new(path).unwrap();

        b.iter(|| {
            let _ = publisher.send(black_box(&[1u8; 8]));
            subscriber.receive(|_| {});
        });
    });

    group.finish();
    let _ = fs::remove_file(path);
}

fn bench_sustained(c: &mut Criterion) {
    let path = "/tmp/flux-ipc-bench-sustained";
    let _ = fs::remove_file(path);

    let mut group = c.benchmark_group("flux-ipc-sustained");

    const EVENTS: usize = 100_000;
    group.throughput(Throughput::Elements(EVENTS as u64));
    group.sample_size(20);

    group.bench_function("100K_msgs", |b| {
        let mut publisher = Publisher::<SmallSlot>::new(path, RING_SIZE).unwrap();
        let mut subscriber = Subscriber::<SmallSlot>::new(path).unwrap();

        b.iter(|| {
            let mut sent = 0;
            let mut received = 0;

            while received < EVENTS {
                // Send batch
                while sent < EVENTS && sent - received < RING_SIZE - 1024 {
                    if publisher.send(black_box(&[1u8; 8])).is_ok() {
                        sent += 1;
                    }
                }

                // Receive batch
                received += subscriber.receive(|m| {
                    black_box(m);
                });
            }
        });
    });

    group.finish();
    let _ = fs::remove_file(path);
}

criterion_group!(benches, bench_throughput, bench_sustained);
criterion_main!(benches);

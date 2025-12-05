//! RUDP benchmark - throughput and integrity tests

use criterion::{criterion_group, criterion_main, Criterion, Throughput};
use kaos_rudp::ReliableUdpRingBufferTransport;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::Instant;

const BATCH_SIZE: usize = 16;

fn run_rudp_bench(total_events: u64) -> (f64, u64) {
    use std::net::UdpSocket;
    let sock1 = UdpSocket::bind("127.0.0.1:0").unwrap();
    let sock2 = UdpSocket::bind("127.0.0.1:0").unwrap();
    let server_addr = sock1.local_addr().unwrap();
    let client_addr = sock2.local_addr().unwrap();
    drop(sock1);
    drop(sock2);
    thread::sleep(std::time::Duration::from_millis(5));

    let received_count = Arc::new(AtomicU64::new(0));
    let recv_cnt = received_count.clone();

    let receiver = thread::spawn(move || {
        let mut transport =
            ReliableUdpRingBufferTransport::new(server_addr, client_addr, 65536).unwrap();
        let mut count = 0u64;
        while count < total_events {
            transport.receive_batch_with(64, |data| {
                if data.len() >= 8 {
                    count += 1;
                }
            });
        }
        recv_cnt.store(count, Ordering::Release);
    });

    thread::sleep(std::time::Duration::from_millis(10));
    let start = Instant::now();

    let mut transport =
        ReliableUdpRingBufferTransport::new(client_addr, server_addr, 65536).unwrap();
    let mut batch_data: [[u8; 8]; 16] = [[0u8; 8]; 16];
    let mut sent = 0u64;

    while sent < total_events {
        let batch_count = ((total_events - sent) as usize).min(BATCH_SIZE);
        for i in 0..batch_count {
            batch_data[i] = (((sent + i as u64) % 5) + 1).to_le_bytes();
        }
        let refs: [&[u8]; 16] = [
            &batch_data[0],
            &batch_data[1],
            &batch_data[2],
            &batch_data[3],
            &batch_data[4],
            &batch_data[5],
            &batch_data[6],
            &batch_data[7],
            &batch_data[8],
            &batch_data[9],
            &batch_data[10],
            &batch_data[11],
            &batch_data[12],
            &batch_data[13],
            &batch_data[14],
            &batch_data[15],
        ];
        match transport.send_batch(&refs[..batch_count]) {
            Ok(n) => sent += n as u64,
            Err(_) => {
                transport.process_acks();
                std::hint::spin_loop();
            }
        }
        if sent % 10000 == 0 {
            transport.process_acks();
        }
    }

    let timeout = std::time::Duration::from_secs(2);
    let wait_start = Instant::now();
    while received_count.load(Ordering::Acquire) < total_events && wait_start.elapsed() < timeout {
        transport.process_acks();
        std::hint::spin_loop();
    }

    receiver.join().unwrap();
    let duration = start.elapsed().as_secs_f64();
    let received = received_count.load(Ordering::Relaxed);
    let throughput = (received as f64) / duration / 1_000_000.0;
    (throughput, received)
}

fn benchmark_rudp_100k(c: &mut Criterion) {
    const EVENTS: u64 = 100_000;
    let mut group = c.benchmark_group("RUDP (100K events)");
    group.throughput(Throughput::Elements(EVENTS));
    group.sample_size(10);

    group.bench_function("localhost", |b| {
        b.iter(|| {
            let (throughput, received) = run_rudp_bench(EVENTS);
            assert!(received >= EVENTS * 99 / 100, "Lost >1%");
            assert!(throughput > 0.5, "Throughput too low");
            EVENTS
        })
    });
    group.finish();
}

fn benchmark_rudp_500k(c: &mut Criterion) {
    const EVENTS: u64 = 500_000;
    let mut group = c.benchmark_group("RUDP (500K events)");
    group.throughput(Throughput::Elements(EVENTS));
    group.sample_size(10);

    group.bench_function("localhost", |b| {
        b.iter(|| {
            let (throughput, received) = run_rudp_bench(EVENTS);
            assert!(received >= EVENTS * 99 / 100, "Lost >1%");
            assert!(throughput > 1.0, "Throughput too low");
            EVENTS
        })
    });
    group.finish();
}

criterion_group!(benches, benchmark_rudp_100k, benchmark_rudp_500k);
criterion_main!(benches);

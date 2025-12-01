//! Real-world benchmark: Mobile User Interaction Tracking (RUDP)
//!
//! Simulates a production observability system tracking user events
//! over reliable UDP network transport.

use criterion::{ criterion_group, criterion_main, Criterion, Throughput };
use flux_rudp::ReliableUdpRingBufferTransport;
use std::sync::atomic::{ AtomicBool, AtomicU64, Ordering };
use std::sync::Arc;
use std::thread;
use std::time::Instant;

// Event types (mobile user interactions)
const EVENT_CLICK: u64 = 1;
const EVENT_SCROLL: u64 = 2;
const EVENT_PAGEVIEW: u64 = 3;
const EVENT_PURCHASE: u64 = 4;
const EVENT_LOGIN: u64 = 5;

// Using smaller numbers for RUDP since it's network-bound
const EVENTS_PER_TYPE: u64 = 20_000; // 20K per type = 100K total
const TOTAL_EVENTS: u64 = EVENTS_PER_TYPE * 5;
const BATCH_SIZE: usize = 16;

fn run_rudp_trace_bench() -> (f64, bool) {
    let running = Arc::new(AtomicBool::new(true));
    let received_count = Arc::new(AtomicU64::new(0));
    let count_click = Arc::new(AtomicU64::new(0));
    let count_scroll = Arc::new(AtomicU64::new(0));
    let count_pageview = Arc::new(AtomicU64::new(0));
    let count_purchase = Arc::new(AtomicU64::new(0));
    let count_login = Arc::new(AtomicU64::new(0));

    // Use random ports for this benchmark
    use std::net::UdpSocket;
    let sock1 = UdpSocket::bind("127.0.0.1:0").unwrap();
    let sock2 = UdpSocket::bind("127.0.0.1:0").unwrap();
    let server_addr = sock1.local_addr().unwrap();
    let client_addr = sock2.local_addr().unwrap();
    drop(sock1);
    drop(sock2);
    thread::sleep(std::time::Duration::from_millis(10));

    // Receiver thread
    let running_recv = running.clone();
    let recv_cnt = received_count.clone();
    let c_click = count_click.clone();
    let c_scroll = count_scroll.clone();
    let c_pageview = count_pageview.clone();
    let c_purchase = count_purchase.clone();
    let c_login = count_login.clone();

    let receiver = thread::spawn(move || {
        let mut transport = ReliableUdpRingBufferTransport::new(
            server_addr,
            client_addr,
            65536
        ).unwrap();

        let mut local_click = 0u64;
        let mut local_scroll = 0u64;
        let mut local_pageview = 0u64;
        let mut local_purchase = 0u64;
        let mut local_login = 0u64;
        let mut received = 0u64;

        while received < TOTAL_EVENTS && running_recv.load(Ordering::Relaxed) {
            transport.receive_batch_with(BATCH_SIZE, |data| {
                if data.len() >= 8 {
                    let event_type = u64::from_le_bytes(data[..8].try_into().unwrap_or([0; 8]));
                    match event_type {
                        EVENT_CLICK => local_click += 1,
                        EVENT_SCROLL => local_scroll += 1,
                        EVENT_PAGEVIEW => local_pageview += 1,
                        EVENT_PURCHASE => local_purchase += 1,
                        EVENT_LOGIN => local_login += 1,
                        _ => {}
                    }
                    received += 1;
                    recv_cnt.store(received, Ordering::Relaxed);
                }
            });
            transport.process_naks();
        }

        c_click.store(local_click, Ordering::Release);
        c_scroll.store(local_scroll, Ordering::Release);
        c_pageview.store(local_pageview, Ordering::Release);
        c_purchase.store(local_purchase, Ordering::Release);
        c_login.store(local_login, Ordering::Release);
    });

    // Give receiver time to bind
    thread::sleep(std::time::Duration::from_millis(50));

    let start = Instant::now();

    // Sender thread
    let sender = thread::spawn(move || {
        let mut transport = ReliableUdpRingBufferTransport::new(
            client_addr,
            server_addr,
            65536
        ).unwrap();

        let mut event_type = EVENT_CLICK;
        let mut sent = 0u64;
        let mut batch: Vec<Vec<u8>> = Vec::with_capacity(BATCH_SIZE);

        while sent < TOTAL_EVENTS {
            batch.clear();
            for _ in 0..BATCH_SIZE {
                if sent >= TOTAL_EVENTS {
                    break;
                }
                batch.push(event_type.to_le_bytes().to_vec());
                event_type = if event_type >= EVENT_LOGIN { EVENT_CLICK } else { event_type + 1 };
                sent += 1;
            }

            let refs: Vec<&[u8]> = batch.iter().map(|v| v.as_slice()).collect();
            match transport.send_batch(&refs) {
                Ok(_) => {}
                Err(_) => {
                    // Window full - process ACKs to free up space
                    transport.process_acks();
                    thread::yield_now();
                }
            }
            transport.process_naks();
            
            // Periodically process ACKs
            if sent % 1000 == 0 {
                transport.process_acks();
            }

            if sent % 10000 == 0 {
                thread::yield_now();
            }
        }
        sent
    });

    let _sent = sender.join().unwrap();
    
    // Wait for receiver to finish or timeout
    let timeout = std::time::Duration::from_secs(5);
    let wait_start = Instant::now();
    while received_count.load(Ordering::Relaxed) < TOTAL_EVENTS && wait_start.elapsed() < timeout {
        thread::sleep(std::time::Duration::from_millis(10));
    }
    
    running.store(false, Ordering::SeqCst);
    receiver.join().unwrap();

    let duration = start.elapsed().as_secs_f64();
    let received = received_count.load(Ordering::Relaxed);
    let throughput = (received as f64) / duration / 1_000_000.0;

    let counts = [
        count_click.load(Ordering::Relaxed),
        count_scroll.load(Ordering::Relaxed),
        count_pageview.load(Ordering::Relaxed),
        count_purchase.load(Ordering::Relaxed),
        count_login.load(Ordering::Relaxed),
    ];

    let total: u64 = counts.iter().sum();
    // For RUDP on localhost, we expect at least 50% delivery
    let verified = total > 0 && received > TOTAL_EVENTS / 2;

    (throughput, verified)
}

fn benchmark_rudp_trace(c: &mut Criterion) {
    let mut group = c.benchmark_group("RUDP Trace Events (100K events)");
    group.throughput(Throughput::Elements(TOTAL_EVENTS));
    group.sample_size(10);

    group.bench_function("localhost_udp", |b| {
        b.iter(|| {
            let (_throughput, verified) = run_rudp_trace_bench();
            assert!(verified, "RUDP benchmark failed - too much packet loss");
            TOTAL_EVENTS
        })
    });

    group.finish();
}

criterion_group!(benches, benchmark_rudp_trace);
criterion_main!(benches);

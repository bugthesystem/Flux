//! RUDP Stress Tests
//!
//! Long-running stress tests for kaos-rudp.

use kaos_test_support::stress::{StressConfig, StressRunner, StressCounters, print_summary};
use kaos_test_support::verify::SequenceChecker;
use std::net::UdpSocket;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

const MSG_SIZE: usize = 64;

/// Simple UDP stress test (no RUDP, baseline)
#[test]
fn test_udp_baseline_stress() {
    let config = StressConfig::new(5)
        .with_batch_size(100);
    
    let runner = StressRunner::new(config.clone());
    let counters = runner.counters();
    
    let sender_socket = UdpSocket::bind("127.0.0.1:0").unwrap();
    let receiver_socket = UdpSocket::bind("127.0.0.1:0").unwrap();
    receiver_socket.set_nonblocking(true).unwrap();
    let receiver_addr = receiver_socket.local_addr().unwrap();
    
    let metrics = runner.run_with_progress(|counters| {
        let counters_send = counters.clone();
        let counters_recv = counters.clone();
        
        // Sender thread
        let sender = thread::spawn(move || {
            let mut seq = 0u64;
            while counters_send.is_running() {
                let mut msg = vec![0u8; MSG_SIZE];
                msg[..8].copy_from_slice(&seq.to_le_bytes());
                
                if sender_socket.send_to(&msg, receiver_addr).is_ok() {
                    counters_send.record_send(MSG_SIZE);
                    seq += 1;
                }
                
                // Rate limit to avoid overwhelming
                if seq % 10000 == 0 {
                    thread::yield_now();
                }
            }
        });
        
        // Receiver thread
        let receiver = thread::spawn(move || {
            let mut buf = [0u8; 2048];
            while counters_recv.is_running() {
                match receiver_socket.recv(&mut buf) {
                    Ok(len) => counters_recv.record_receive(len),
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                        thread::yield_now();
                    }
                    Err(_) => {}
                }
            }
        });
        
        sender.join().unwrap();
        receiver.join().unwrap();
    });
    
    print_summary(&metrics);
    
    // UDP may lose some packets, but should be mostly reliable on localhost
    let loss = metrics.loss_rate();
    assert!(loss < 0.1, "Too much loss on localhost: {:.2}%", loss * 100.0);
}

/// Stress test with sequence verification
#[test]
fn test_udp_sequence_stress() {
    let running = Arc::new(AtomicBool::new(true));
    let checker = Arc::new(SequenceChecker::new());
    
    let sender_socket = UdpSocket::bind("127.0.0.1:0").unwrap();
    let receiver_socket = UdpSocket::bind("127.0.0.1:0").unwrap();
    receiver_socket.set_read_timeout(Some(Duration::from_millis(100))).unwrap();
    let receiver_addr = receiver_socket.local_addr().unwrap();
    
    let running_send = running.clone();
    let running_recv = running.clone();
    let checker_recv = checker.clone();
    
    // Sender
    let sender = thread::spawn(move || {
        let mut seq = 0u64;
        while running_send.load(Ordering::Relaxed) {
            let mut msg = vec![0u8; MSG_SIZE];
            msg[..8].copy_from_slice(&seq.to_le_bytes());
            
            let _ = sender_socket.send_to(&msg, receiver_addr);
            seq += 1;
            
            if seq % 1000 == 0 {
                thread::yield_now();
            }
        }
        seq
    });
    
    // Receiver
    let receiver = thread::spawn(move || {
        let mut buf = [0u8; 2048];
        let mut count = 0u64;
        
        while running_recv.load(Ordering::Relaxed) {
            match receiver_socket.recv(&mut buf) {
                Ok(len) if len >= 8 => {
                    let seq = u64::from_le_bytes(buf[..8].try_into().unwrap());
                    checker_recv.check(seq);
                    count += 1;
                }
                _ => {}
            }
        }
        count
    });
    
    // Run for duration
    thread::sleep(Duration::from_secs(5));
    running.store(false, Ordering::SeqCst);
    
    let sent = sender.join().unwrap();
    let received = receiver.join().unwrap();
    
    let stats = checker.stats();
    let gaps = checker.gaps();
    
    println!("\n=== UDP Sequence Stress Test ===");
    println!("Sent: {}", sent);
    println!("Received: {}", received);
    println!("Gaps: {} (total missing: {})", stats.gap_count, stats.total_missing);
    println!("Out of order: {}", stats.out_of_order);
    println!("Delivery: {:.2}%", stats.delivery_rate() * 100.0);
    
    if !gaps.is_empty() {
        println!("First 5 gaps: {:?}", &gaps[..gaps.len().min(5)]);
    }
    
    // On localhost, we shouldn't have too many gaps
    // (some are expected due to UDP's unreliability)
    assert!(stats.delivery_rate() > 0.9, 
            "Delivery rate too low: {:.2}%", stats.delivery_rate() * 100.0);
}

/// Burst stress test - send in bursts with pauses
#[test]
fn test_udp_burst_stress() {
    let sender_socket = UdpSocket::bind("127.0.0.1:0").unwrap();
    let receiver_socket = UdpSocket::bind("127.0.0.1:0").unwrap();
    receiver_socket.set_read_timeout(Some(Duration::from_millis(500))).unwrap();
    let receiver_addr = receiver_socket.local_addr().unwrap();
    
    let running = Arc::new(AtomicBool::new(true));
    let running_recv = running.clone();
    
    let received = Arc::new(std::sync::atomic::AtomicU64::new(0));
    let received_clone = received.clone();
    
    // Receiver thread
    let receiver = thread::spawn(move || {
        let mut buf = [0u8; 2048];
        while running_recv.load(Ordering::Relaxed) {
            if receiver_socket.recv(&mut buf).is_ok() {
                received_clone.fetch_add(1, Ordering::Relaxed);
            }
        }
    });
    
    // Send in bursts
    let burst_size = 1000;
    let burst_count = 100;
    let mut total_sent = 0u64;
    
    for burst in 0..burst_count {
        // Send burst
        for i in 0..burst_size {
            let seq = burst * burst_size + i;
            let mut msg = vec![0u8; MSG_SIZE];
            msg[..8].copy_from_slice(&(seq as u64).to_le_bytes());
            let _ = sender_socket.send_to(&msg, receiver_addr);
            total_sent += 1;
        }
        
        // Pause between bursts
        thread::sleep(Duration::from_millis(10));
    }
    
    // Wait for final packets
    thread::sleep(Duration::from_millis(500));
    running.store(false, Ordering::SeqCst);
    
    let total_received = received.load(Ordering::Relaxed);
    receiver.join().unwrap();
    
    println!("\n=== UDP Burst Stress Test ===");
    println!("Bursts: {} x {}", burst_count, burst_size);
    println!("Total sent: {}", total_sent);
    println!("Total received: {}", total_received);
    println!("Delivery: {:.2}%", total_received as f64 / total_sent as f64 * 100.0);
    
    // Should receive most packets
    assert!(total_received as f64 / total_sent as f64 > 0.9,
            "Too much loss in burst mode");
}

/// Memory stress test - verify no leaks over time
#[test]
#[ignore] // Run with: cargo test --release -- --ignored memory
fn test_memory_stress() {
    let sender_socket = UdpSocket::bind("127.0.0.1:0").unwrap();
    let receiver_socket = UdpSocket::bind("127.0.0.1:0").unwrap();
    receiver_socket.set_nonblocking(true).unwrap();
    let receiver_addr = receiver_socket.local_addr().unwrap();
    
    let running = Arc::new(AtomicBool::new(true));
    let running_send = running.clone();
    let running_recv = running.clone();
    
    // Sender
    let sender = thread::spawn(move || {
        let mut seq = 0u64;
        while running_send.load(Ordering::Relaxed) {
            let msg = vec![0u8; MSG_SIZE]; // Allocate each time
            let _ = sender_socket.send_to(&msg, receiver_addr);
            seq += 1;
        }
        seq
    });
    
    // Receiver
    let receiver = thread::spawn(move || {
        let mut count = 0u64;
        while running_recv.load(Ordering::Relaxed) {
            let mut buf = vec![0u8; 2048]; // Allocate each time
            if receiver_socket.recv(&mut buf).is_ok() {
                count += 1;
            }
        }
        count
    });
    
    // Run for extended period
    let duration = Duration::from_secs(30);
    let start = std::time::Instant::now();
    
    while start.elapsed() < duration {
        thread::sleep(Duration::from_secs(5));
        eprintln!("[{:>3}s] running...", start.elapsed().as_secs());
    }
    
    running.store(false, Ordering::SeqCst);
    
    let sent = sender.join().unwrap();
    let received = receiver.join().unwrap();
    
    println!("\n=== Memory Stress Test (30s) ===");
    println!("Sent: {} ({:.2}B)", sent, sent as f64 / 1_000_000_000.0);
    println!("Received: {}", received);
    
    // Note: actual memory leak detection would require valgrind or similar
    // This test mainly verifies the code doesn't crash under load
}


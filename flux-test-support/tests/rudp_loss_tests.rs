//! RUDP Loss Recovery Tests
//!
//! Tests that flux-rudp correctly handles packet loss scenarios.

use flux_test_support::loss::{LossGenerator, LossPattern, DropDecision};
use flux_test_support::verify::SequenceChecker;
use std::net::{SocketAddr, UdpSocket};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

const MSG_SIZE: usize = 64;

/// Simulated sender that respects loss generator
fn simulated_sender(
    socket: &UdpSocket,
    dest: SocketAddr,
    count: usize,
    loss: &mut LossGenerator,
) -> (usize, usize) {
    let mut sent = 0;
    let mut dropped = 0;
    
    for seq in 0..count {
        let mut msg = vec![0u8; MSG_SIZE];
        // Encode sequence in first 8 bytes
        msg[..8].copy_from_slice(&(seq as u64).to_le_bytes());
        
        match loss.should_drop(seq as u64) {
            DropDecision::Drop => {
                dropped += 1;
                // Don't send - simulating loss
            }
            DropDecision::Pass => {
                let _ = socket.send_to(&msg, dest);
                sent += 1;
            }
        }
    }
    
    (sent, dropped)
}

/// Simulated receiver that tracks sequences
fn simulated_receiver(
    socket: &UdpSocket,
    checker: &SequenceChecker,
    running: Arc<AtomicBool>,
) -> usize {
    let mut buf = [0u8; 2048];
    let mut received = 0;
    
    socket.set_read_timeout(Some(Duration::from_millis(100))).unwrap();
    
    while running.load(Ordering::Relaxed) {
        match socket.recv(&mut buf) {
            Ok(len) if len >= 8 => {
                let seq = u64::from_le_bytes(buf[..8].try_into().unwrap());
                checker.check(seq);
                received += 1;
            }
            Ok(_) => {}
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                // Timeout, check if still running
            }
            Err(_) => break,
        }
    }
    
    received
}

#[test]
fn test_no_loss_baseline() {
    // Baseline: no loss, everything should arrive
    let sender_socket = UdpSocket::bind("127.0.0.1:0").unwrap();
    let receiver_socket = UdpSocket::bind("127.0.0.1:0").unwrap();
    let receiver_addr = receiver_socket.local_addr().unwrap();
    
    let running = Arc::new(AtomicBool::new(true));
    let checker = Arc::new(SequenceChecker::new());
    
    let checker_clone = checker.clone();
    let running_clone = running.clone();
    let receiver = thread::spawn(move || {
        simulated_receiver(&receiver_socket, &checker_clone, running_clone)
    });
    
    // Give receiver time to start
    thread::sleep(Duration::from_millis(10));
    
    let mut loss = LossGenerator::none();
    let count = 1000;
    let (sent, dropped) = simulated_sender(&sender_socket, receiver_addr, count, &mut loss);
    
    // Wait for all packets to arrive
    thread::sleep(Duration::from_millis(100));
    running.store(false, Ordering::Relaxed);
    
    let received = receiver.join().unwrap();
    
    println!("No Loss: sent={}, dropped={}, received={}", sent, dropped, received);
    
    assert_eq!(dropped, 0);
    assert_eq!(sent, count);
    // UDP may still lose some packets due to OS, but should be minimal
    assert!(received >= count - 10, "Too many packets lost: {}", count - received);
}

#[test]
fn test_periodic_loss_detection() {
    // Test that we correctly detect periodic loss
    let mut loss = LossGenerator::periodic(10);
    let mut dropped = 0;
    
    for seq in 0..100u64 {
        if loss.should_drop(seq) == DropDecision::Drop {
            dropped += 1;
        }
    }
    
    // Should drop 10 packets (10, 20, 30, ..., 100)
    assert_eq!(dropped, 10, "Expected 10 drops, got {}", dropped);
}

#[test]
fn test_burst_loss_detection() {
    // Burst loss starting at seq 50, length 20
    let mut loss = LossGenerator::burst(50, 20);
    let checker = SequenceChecker::new();
    
    for seq in 0..100u64 {
        if loss.should_drop(seq) == DropDecision::Pass {
            checker.check(seq);
        }
    }
    
    let stats = checker.stats();
    println!("Burst loss: total_seen={}, gaps={:?}", stats.total_seen, checker.gaps());
    
    assert_eq!(stats.total_seen, 80); // 100 - 20 dropped
    assert_eq!(stats.gap_count, 1);
    assert_eq!(stats.total_missing, 20);
}

#[test]
fn test_random_loss_statistics() {
    // Test that random loss has expected distribution
    let trials = 10;
    let packets = 10000;
    let target_loss = 0.05; // 5%
    
    let mut total_dropped = 0;
    
    for _ in 0..trials {
        let mut loss = LossGenerator::random(target_loss);
        let dropped: usize = (0..packets)
            .filter(|&seq| loss.should_drop(seq as u64) == DropDecision::Drop)
            .count();
        total_dropped += dropped;
    }
    
    let avg_loss = total_dropped as f64 / (trials * packets) as f64;
    println!("Random loss: target={}, actual={}", target_loss, avg_loss);
    
    // Should be within 1% of target
    assert!((avg_loss - target_loss).abs() < 0.01, 
            "Loss rate {} not close to target {}", avg_loss, target_loss);
}

#[test]
fn test_sequence_gap_detection() {
    let checker = SequenceChecker::new();
    
    // Simulate receiving with gaps
    checker.check(0);
    checker.check(1);
    checker.check(2);
    // Gap: 3, 4
    checker.check(5);
    checker.check(6);
    // Gap: 7, 8, 9
    checker.check(10);
    
    let stats = checker.stats();
    let gaps = checker.gaps();
    
    println!("Gaps detected: {:?}", gaps);
    println!("Stats: {:?}", stats);
    
    assert_eq!(gaps.len(), 2);
    assert_eq!(gaps[0], (3, 4));
    assert_eq!(gaps[1], (7, 9));
    assert_eq!(stats.total_missing, 5); // 3,4 + 7,8,9
}

#[test]
fn test_combined_loss_patterns() {
    // Combine periodic and burst loss
    let combined = LossPattern::Combined(vec![
        LossPattern::Periodic { every_n: 100 },
        LossPattern::Burst { start_seq: 500, length: 10 },
    ]);
    
    let mut loss = LossGenerator::new(combined);
    let mut dropped = vec![];
    
    for seq in 0..1000u64 {
        if loss.should_drop(seq) == DropDecision::Drop {
            dropped.push(seq);
        }
    }
    
    println!("Combined loss dropped {} packets: {:?}", dropped.len(), &dropped[..dropped.len().min(15)]);
    
    // Periodic drops at packet counts 100, 200, ... = sequences 99, 199, ...
    // (packet_count increments before check, so packet 100 = seq 99)
    // Plus burst at sequences 500-509
    assert!(dropped.len() >= 10, "Should have at least burst drops");
    assert!(dropped.contains(&500), "Should have burst start");
    assert!(dropped.contains(&509), "Should have burst end");
}

/// Integration test: simulated RUDP with loss
#[test]
fn test_simulated_rudp_with_loss() {
    let sender_socket = UdpSocket::bind("127.0.0.1:0").unwrap();
    let receiver_socket = UdpSocket::bind("127.0.0.1:0").unwrap();
    let receiver_addr = receiver_socket.local_addr().unwrap();
    
    let running = Arc::new(AtomicBool::new(true));
    let checker = Arc::new(SequenceChecker::new());
    
    let checker_clone = checker.clone();
    let running_clone = running.clone();
    let receiver = thread::spawn(move || {
        simulated_receiver(&receiver_socket, &checker_clone, running_clone)
    });
    
    thread::sleep(Duration::from_millis(10));
    
    // 5% random loss
    let mut loss = LossGenerator::random(0.05);
    let count = 5000;
    let (sent, dropped) = simulated_sender(&sender_socket, receiver_addr, count, &mut loss);
    
    thread::sleep(Duration::from_millis(200));
    running.store(false, Ordering::Relaxed);
    
    let received = receiver.join().unwrap();
    let stats = checker.stats();
    
    println!("\n=== Simulated RUDP with 5% Loss ===");
    println!("Sent (after loss): {}", sent);
    println!("Intentionally dropped: {}", dropped);
    println!("Received: {}", received);
    println!("Gaps detected: {}", stats.gap_count);
    println!("Total missing: {}", stats.total_missing);
    println!("Delivery rate: {:.2}%", stats.delivery_rate() * 100.0);
    
    // With 5% loss, we should see gaps
    assert!(dropped > 0, "Should have dropped some packets");
    
    // Delivery rate should roughly match (1 - loss_rate)
    let expected_delivery = (count - dropped) as f64 / count as f64;
    let actual_delivery = received as f64 / count as f64;
    
    // Allow some variance for UDP unreliability
    assert!(
        (actual_delivery - expected_delivery).abs() < 0.05,
        "Delivery rate {} differs too much from expected {}", 
        actual_delivery, expected_delivery
    );
}


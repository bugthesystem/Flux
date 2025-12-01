//! IPC Stress Tests
//!
//! Tests flux-ipc under stress conditions with timeouts.

use flux_ipc::{Publisher, Subscriber, SmallSlot};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};
use std::fs;

const TEST_PATH: &str = "/tmp/flux-ipc-stress-test";
const TEST_TIMEOUT: Duration = Duration::from_secs(30);

fn cleanup() {
    let _ = fs::remove_file(TEST_PATH);
}

/// Basic stress test: fast producer, fast consumer
#[test]
fn test_ipc_stress_basic() {
    cleanup();
    
    let mut publisher = Publisher::<SmallSlot>::new(TEST_PATH, 64 * 1024).unwrap();
    let mut subscriber = Subscriber::<SmallSlot>::new(TEST_PATH).unwrap();
    
    let count = 100_000u64; // Reduced for faster tests
    let mut sent = 0u64;
    let mut received = 0u64;
    let mut errors = 0u64;
    
    let start = Instant::now();
    
    while received < count && start.elapsed() < TEST_TIMEOUT {
        // Send batch
        while sent < count && sent - received < 60_000 {
            if publisher.send(&sent.to_le_bytes()).is_ok() {
                sent += 1;
            } else {
                break;
            }
        }
        
        // Receive batch
        received += subscriber.receive(|slot| {
            if slot.value >= count {
                errors += 1;
            }
        }) as u64;
    }
    
    let duration = start.elapsed();
    
    println!("\n=== IPC Stress Test (Basic) ===");
    println!("Messages: {}/{}", received, count);
    println!("Duration: {:.2}s", duration.as_secs_f64());
    println!("Rate: {:.2} M/s", received as f64 / duration.as_secs_f64() / 1_000_000.0);
    println!("Errors: {}", errors);
    
    assert_eq!(errors, 0);
    assert_eq!(received, count, "Timed out before completion");
    
    cleanup();
}

/// Test with slow consumer (backpressure)
#[test]
fn test_ipc_slow_consumer() {
    cleanup();
    
    // Very small ring to force backpressure
    let mut publisher = Publisher::<SmallSlot>::new(TEST_PATH, 64).unwrap();
    let mut subscriber = Subscriber::<SmallSlot>::new(TEST_PATH).unwrap();
    
    let count = 1_000u64;
    let mut sent = 0u64;
    let mut received = 0u64;
    let mut backpressure_events = 0u64;
    
    let start = Instant::now();
    
    while received < count && start.elapsed() < TEST_TIMEOUT {
        // Try to send burst (will hit backpressure with small ring)
        for _ in 0..100 {
            if sent < count {
                match publisher.send(&sent.to_le_bytes()) {
                    Ok(_) => sent += 1,
                    Err(_) => {
                        backpressure_events += 1;
                        break;
                    }
                }
            }
        }
        
        // Consume slowly
        received += subscriber.receive(|_| {}) as u64;
    }
    
    let duration = start.elapsed();
    
    println!("\n=== IPC Slow Consumer Test ===");
    println!("Messages: {}/{}", received, count);
    println!("Backpressure events: {}", backpressure_events);
    println!("Duration: {:.2}s", duration.as_secs_f64());
    
    assert!(backpressure_events > 0, "Should have experienced backpressure");
    assert_eq!(received, count, "Timed out before completion");
    
    cleanup();
}

/// Test data integrity with checksums
#[test]
fn test_ipc_data_integrity() {
    cleanup();
    
    let mut publisher = Publisher::<SmallSlot>::new(TEST_PATH, 64 * 1024).unwrap();
    let mut subscriber = Subscriber::<SmallSlot>::new(TEST_PATH).unwrap();
    
    let count = 100_000u64;
    let mut expected_sum: u64 = 0;
    let mut actual_sum: u64 = 0;
    
    // Calculate expected sum
    for i in 0..count {
        expected_sum = expected_sum.wrapping_add(i);
    }
    
    let mut sent = 0u64;
    let mut received = 0u64;
    
    let start = Instant::now();
    
    while received < count && start.elapsed() < TEST_TIMEOUT {
        // Send
        while sent < count && sent - received < 60_000 {
            if publisher.send(&sent.to_le_bytes()).is_ok() {
                sent += 1;
            } else {
                break;
            }
        }
        
        // Receive and sum
        received += subscriber.receive(|slot| {
            actual_sum = actual_sum.wrapping_add(slot.value);
        }) as u64;
    }
    
    println!("\n=== IPC Data Integrity Test ===");
    println!("Messages: {}/{}", received, count);
    println!("Expected sum: {}", expected_sum);
    println!("Actual sum: {}", actual_sum);
    
    assert_eq!(received, count, "Timed out before completion");
    assert_eq!(actual_sum, expected_sum, "Data corruption detected!");
    
    cleanup();
}

/// Concurrent access test (simulated multi-process)
#[test]
fn test_ipc_concurrent_threads() {
    let path = "/tmp/flux-ipc-concurrent-test";
    let _ = fs::remove_file(path);
    
    let running = Arc::new(AtomicBool::new(true));
    let sent_count = Arc::new(AtomicU64::new(0));
    let recv_count = Arc::new(AtomicU64::new(0));
    let errors = Arc::new(AtomicU64::new(0));
    
    // Spawn producer thread
    let path_clone = path.to_string();
    let running_prod = running.clone();
    let sent_clone = sent_count.clone();
    
    let producer = thread::spawn(move || {
        let mut publisher = Publisher::<SmallSlot>::new(&path_clone, 64 * 1024).unwrap();
        let mut seq = 0u64;
        
        while running_prod.load(Ordering::Relaxed) {
            if publisher.send(&seq.to_le_bytes()).is_ok() {
                seq += 1;
                sent_clone.store(seq, Ordering::Relaxed);
            }
        }
        seq
    });
    
    // Give producer time to create file
    thread::sleep(Duration::from_millis(100));
    
    // Spawn consumer thread
    let path_clone = path.to_string();
    let running_cons = running.clone();
    let recv_clone = recv_count.clone();
    let errors_clone = errors.clone();
    
    let consumer = thread::spawn(move || {
        let mut subscriber = match Subscriber::<SmallSlot>::new(&path_clone) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("Failed to open subscriber: {}", e);
                return 0u64;
            }
        };
        let mut expected = 0u64;
        let mut total = 0u64;
        let start = Instant::now();
        
        while (running_cons.load(Ordering::Relaxed) || subscriber.available() > 0) 
              && start.elapsed() < TEST_TIMEOUT {
            let count = subscriber.receive(|slot| {
                if slot.value != expected {
                    errors_clone.fetch_add(1, Ordering::Relaxed);
                }
                expected += 1;
            });
            total += count as u64;
            recv_clone.store(total, Ordering::Relaxed);
            
            if count == 0 {
                thread::yield_now();
            }
        }
        total
    });
    
    // Run for 2 seconds
    let duration = Duration::from_secs(2);
    thread::sleep(duration);
    
    running.store(false, Ordering::SeqCst);
    
    let total_sent = producer.join().unwrap();
    let total_recv = consumer.join().unwrap();
    let error_count = errors.load(Ordering::Relaxed);
    
    println!("\n=== IPC Concurrent Test ===");
    println!("Sent: {}", total_sent);
    println!("Received: {}", total_recv);
    println!("Errors: {}", error_count);
    println!("Rate: {:.2} M/s", total_sent as f64 / duration.as_secs_f64() / 1_000_000.0);
    
    assert_eq!(error_count, 0, "Ordering errors in concurrent access!");
    assert!(total_recv > 0, "Consumer didn't receive any messages");
    
    let _ = fs::remove_file(path);
}

/// Test ring buffer wraparound in IPC
#[test]
fn test_ipc_wraparound() {
    cleanup();
    
    // Small ring to force many wraparounds
    let mut publisher = Publisher::<SmallSlot>::new(TEST_PATH, 256).unwrap();
    let mut subscriber = Subscriber::<SmallSlot>::new(TEST_PATH).unwrap();
    
    let count = 10_000u64; // Reduced for speed
    let mut sent = 0u64;
    let mut received = 0u64;
    let mut errors = 0u64;
    
    let start = Instant::now();
    
    while received < count && start.elapsed() < TEST_TIMEOUT {
        // Send some
        for _ in 0..10 {
            if sent < count {
                match publisher.send(&sent.to_le_bytes()) {
                    Ok(_) => sent += 1,
                    Err(_) => break,
                }
            }
        }
        
        // Receive some
        let mut expected = received;
        received += subscriber.receive(|slot| {
            if slot.value != expected {
                errors += 1;
            }
            expected += 1;
        }) as u64;
    }
    
    println!("\n=== IPC Wraparound Test ===");
    println!("Ring size: 256");
    println!("Messages: {}/{}", received, count);
    println!("Wraparounds: ~{}", received / 256);
    println!("Errors: {}", errors);
    
    assert_eq!(received, count, "Timed out before completion");
    assert_eq!(errors, 0, "Wraparound caused data corruption!");
    
    cleanup();
}

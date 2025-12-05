//! IPC Stress Tests
use kaos_ipc::{Publisher, Subscriber};
use std::fs;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

const TEST_TIMEOUT: Duration = Duration::from_secs(30);

fn test_path(name: &str) -> String {
    format!("/tmp/kaos-ipc-{}-{}", name, std::process::id())
}

fn cleanup(path: &str) {
    let _ = fs::remove_file(path);
}

#[test]
fn test_ipc_stress_basic() {
    let path = test_path("basic");
    cleanup(&path);

    let mut pub_ = Publisher::create(&path, 64 * 1024).unwrap();
    let mut sub = Subscriber::open(&path).unwrap();

    let count = 100_000u64;
    let (mut sent, mut received, mut errors) = (0u64, 0u64, 0u64);
    let start = Instant::now();

    while received < count && start.elapsed() < TEST_TIMEOUT {
        while sent < count && sent.saturating_sub(received) < 60_000 {
            if pub_.send(sent).is_ok() {
                sent += 1;
            } else {
                break;
            }
        }
        sub.receive(|val| {
            if val >= count {
                errors += 1;
            }
            received += 1;
        });
    }

    assert_eq!(errors, 0);
    assert_eq!(received, count, "Timed out");
    cleanup(&path);
}

#[test]
fn test_ipc_slow_consumer() {
    let path = test_path("slow");
    cleanup(&path);

    let mut pub_ = Publisher::create(&path, 64).unwrap();
    let mut sub = Subscriber::open(&path).unwrap();

    let count = 1_000u64;
    let (mut sent, mut received, mut backpressure) = (0u64, 0u64, 0u64);
    let start = Instant::now();

    while received < count && start.elapsed() < TEST_TIMEOUT {
        for _ in 0..100 {
            if sent < count {
                if pub_.send(sent).is_ok() {
                    sent += 1;
                } else {
                    backpressure += 1;
                    break;
                }
            }
        }
        sub.receive(|_| received += 1);
    }

    assert!(backpressure > 0, "Should have backpressure");
    assert_eq!(received, sent);
    cleanup(&path);
}

#[test]
fn test_ipc_data_integrity() {
    let path = test_path("integrity");
    cleanup(&path);

    let mut pub_ = Publisher::create(&path, 64 * 1024).unwrap();
    let mut sub = Subscriber::open(&path).unwrap();

    let count = 100_000u64;
    let expected_sum: u64 = (0..count).fold(0u64, |a, b| a.wrapping_add(b));
    let (mut sent, mut received, mut actual_sum) = (0u64, 0u64, 0u64);
    let start = Instant::now();

    while received < count && start.elapsed() < TEST_TIMEOUT {
        while sent < count && sent.saturating_sub(received) < 60_000 {
            if pub_.send(sent).is_ok() {
                sent += 1;
            } else {
                break;
            }
        }
        sub.receive(|val| {
            actual_sum = actual_sum.wrapping_add(val);
            received += 1;
        });
    }

    assert_eq!(received, count, "Timed out");
    assert_eq!(actual_sum, expected_sum, "Data corruption!");
    cleanup(&path);
}

#[test]
fn test_ipc_concurrent_threads() {
    let path = "/tmp/kaos-ipc-concurrent-test";
    let _ = fs::remove_file(path);

    let running = Arc::new(AtomicBool::new(true));
    let errors = Arc::new(AtomicU64::new(0));

    let path_clone = path.to_string();
    let running_prod = running.clone();

    let producer = thread::spawn(move || {
        let mut pub_ = Publisher::create(&path_clone, 64 * 1024).unwrap();
        let mut seq = 0u64;
        while running_prod.load(Ordering::Relaxed) {
            if pub_.send(seq).is_ok() {
                seq += 1;
            }
        }
        seq
    });

    thread::sleep(Duration::from_millis(100));

    let path_clone = path.to_string();
    let running_cons = running.clone();
    let errors_clone = errors.clone();

    let consumer = thread::spawn(move || {
        let mut sub = Subscriber::open(&path_clone).unwrap();
        let (mut expected, mut total) = (0u64, 0u64);
        let start = Instant::now();

        while (running_cons.load(Ordering::Relaxed) || sub.available() > 0)
            && start.elapsed() < TEST_TIMEOUT
        {
            let n = sub.receive(|val| {
                if val != expected {
                    errors_clone.fetch_add(1, Ordering::Relaxed);
                }
                expected += 1;
            });
            total += n as u64;
            if n == 0 {
                thread::yield_now();
            }
        }
        total
    });

    thread::sleep(Duration::from_secs(2));
    running.store(false, Ordering::SeqCst);

    let total_sent = producer.join().unwrap();
    let total_recv = consumer.join().unwrap();
    let error_count = errors.load(Ordering::Relaxed);

    assert_eq!(error_count, 0, "Ordering errors!");
    assert!(total_recv > 0, "No messages received");
    println!("Concurrent: sent={} recv={}", total_sent, total_recv);

    let _ = fs::remove_file(path);
}

#[test]
fn test_ipc_wraparound() {
    let path = test_path("wrap");
    cleanup(&path);

    let mut pub_ = Publisher::create(&path, 256).unwrap();
    let mut sub = Subscriber::open(&path).unwrap();

    let count = 10_000u64;
    let (mut sent, mut received, mut errors, mut expected) = (0u64, 0u64, 0u64, 0u64);
    let start = Instant::now();

    while received < count && start.elapsed() < TEST_TIMEOUT {
        for _ in 0..10 {
            if sent < count {
                if pub_.send(sent).is_ok() {
                    sent += 1;
                } else {
                    break;
                }
            }
        }
        sub.receive(|val| {
            if val != expected {
                errors += 1;
            }
            expected += 1;
            received += 1;
        });
    }

    assert_eq!(received, count, "Timed out");
    assert_eq!(errors, 0, "Wraparound corruption!");
    cleanup(&path);
}

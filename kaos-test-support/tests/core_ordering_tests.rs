//! Memory Ordering Tests for Kaos Core Ring Buffer
//!
//! These tests verify that the ring buffer correctly handles
//! concurrent access with proper memory ordering.

use kaos::disruptor::{RingBuffer, Slot8};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

const TEST_DURATION_SECS: u64 = 2;

/// Test SPSC ordering with Slot8 (8 bytes)
#[test]
fn test_spsc_ordering_small_slot() {
    let ring = Arc::new(RingBuffer::<Slot8>::new(64 * 1024).unwrap());
    let running = Arc::new(AtomicBool::new(true));
    let errors = Arc::new(AtomicU64::new(0));
    let producer_seq = Arc::new(AtomicU64::new(0));

    let ring_prod = ring.clone();
    let running_prod = running.clone();
    let producer_seq_w = producer_seq.clone();

    let ring_cons = ring.clone();
    let running_cons = running.clone();
    let errors_cons = errors.clone();
    let producer_seq_r = producer_seq.clone();

    // Producer: write incrementing values
    let producer = thread::spawn(move || {
        let mut cursor = 0u64;
        let mut written = 0u64;

        while running_prod.load(Ordering::Relaxed) {
            if let Some((seq, slots)) = ring_prod.try_claim_slots(1, cursor) {
                slots[0].value = written;
                ring_prod.publish(seq);
                // Publish sequence for consumer
                producer_seq_w.store(seq + 1, Ordering::Release);
                cursor = seq + 1;
                written += 1;
            } else {
                std::hint::spin_loop();
            }
        }
        written
    });

    // Consumer: verify incrementing values
    let consumer = thread::spawn(move || {
        let mut expected = 0u64;
        let mut read = 0u64;
        let mut cons_cursor = 0u64;

        while running_cons.load(Ordering::Relaxed) {
            // Only read up to what producer has published
            let prod_cur = producer_seq_r.load(Ordering::Acquire);
            let available = prod_cur.saturating_sub(cons_cursor);

            if available > 0 {
                let to_read = available.min(1024) as usize;
                let slots = ring_cons.get_read_batch(cons_cursor, to_read);

                for slot in slots {
                    if slot.value != expected {
                        errors_cons.fetch_add(1, Ordering::Relaxed);
                        eprintln!(
                            "ORDERING ERROR: at seq {}, expected={}, got={}",
                            cons_cursor + read,
                            expected,
                            slot.value
                        );
                        if errors_cons.load(Ordering::Relaxed) > 10 {
                            return read;
                        }
                    }
                    expected += 1;
                    read += 1;
                }
                cons_cursor += slots.len() as u64;
                ring_cons.update_consumer(cons_cursor - 1);
            } else {
                std::hint::spin_loop();
            }
        }

        // Drain remaining after producer stops
        let prod_cur = producer_seq_r.load(Ordering::Acquire);
        while cons_cursor < prod_cur {
            let available = prod_cur - cons_cursor;
            let to_read = available.min(1024) as usize;
            let slots = ring_cons.get_read_batch(cons_cursor, to_read);

            for slot in slots {
                if slot.value != expected {
                    errors_cons.fetch_add(1, Ordering::Relaxed);
                }
                expected += 1;
                read += 1;
            }
            cons_cursor += slots.len() as u64;
            ring_cons.update_consumer(cons_cursor - 1);
        }

        read
    });

    // Run for duration
    thread::sleep(Duration::from_secs(TEST_DURATION_SECS));
    running.store(false, Ordering::SeqCst);

    let written = producer.join().unwrap();
    let read = consumer.join().unwrap();
    let error_count = errors.load(Ordering::Relaxed);

    println!("\n=== SPSC Ordering Test (Slot8) ===");
    println!("Written: {}", written);
    println!("Read: {}", read);
    println!("Errors: {}", error_count);
    println!(
        "Rate: {:.2} M/s",
        written as f64 / TEST_DURATION_SECS as f64 / 1_000_000.0
    );

    assert_eq!(error_count, 0, "Memory ordering errors detected!");
    assert!(read > 0, "Consumer didn't read any data");
    // Allow off-by-one due to timing (producer may write one more after stop signal)
    assert!(
        written - read <= 1,
        "Producer/consumer mismatch: {} written, {} read",
        written,
        read
    );
}

/// Stress test with high contention (small ring)
#[test]
fn test_high_contention_ordering() {
    let ring = Arc::new(RingBuffer::<Slot8>::new(1024).unwrap());
    let running = Arc::new(AtomicBool::new(true));
    let errors = Arc::new(AtomicU64::new(0));
    let producer_seq = Arc::new(AtomicU64::new(0));

    let ring_prod = ring.clone();
    let running_prod = running.clone();
    let producer_seq_w = producer_seq.clone();

    let ring_cons = ring.clone();
    let running_cons = running.clone();
    let errors_cons = errors.clone();
    let producer_seq_r = producer_seq.clone();

    // Fast producer
    let producer = thread::spawn(move || {
        let mut cursor = 0u64;
        let mut written = 0u64;

        while running_prod.load(Ordering::Relaxed) {
            if let Some((seq, slots)) = ring_prod.try_claim_slots(16, cursor) {
                for (i, slot) in slots.iter_mut().enumerate() {
                    slot.value = written + i as u64;
                }
                let count = slots.len();
                ring_prod.publish(seq + count as u64 - 1);
                producer_seq_w.store(seq + count as u64, Ordering::Release);
                cursor = seq + count as u64;
                written += count as u64;
            }
        }
        written
    });

    // Slow consumer
    let consumer = thread::spawn(move || {
        let mut expected = 0u64;
        let mut read = 0u64;
        let mut cons_cursor = 0u64;

        while running_cons.load(Ordering::Relaxed) {
            let prod_cur = producer_seq_r.load(Ordering::Acquire);
            let available = prod_cur.saturating_sub(cons_cursor);

            if available > 0 {
                let to_read = available.min(8) as usize;
                let slots = ring_cons.get_read_batch(cons_cursor, to_read);

                for slot in slots {
                    if slot.value != expected {
                        errors_cons.fetch_add(1, Ordering::Relaxed);
                    }
                    expected += 1;
                    read += 1;
                }
                cons_cursor += slots.len() as u64;
                ring_cons.update_consumer(cons_cursor - 1);

                // Simulate slow consumer
                std::thread::sleep(Duration::from_micros(1));
            }
        }
        read
    });

    thread::sleep(Duration::from_secs(TEST_DURATION_SECS));
    running.store(false, Ordering::SeqCst);

    let written = producer.join().unwrap();
    let read = consumer.join().unwrap();
    let error_count = errors.load(Ordering::Relaxed);

    println!("\n=== High Contention Ordering Test ===");
    println!("Written: {}", written);
    println!("Read: {}", read);
    println!("Errors: {}", error_count);

    assert_eq!(error_count, 0, "Memory ordering errors under contention!");
}

/// Test sequence wraparound
#[test]
fn test_sequence_wraparound() {
    let ring = Arc::new(RingBuffer::<Slot8>::new(256).unwrap());

    let iterations = 2000;
    let mut cursor = 0u64;
    let mut cons_cursor = 0u64;
    let mut values_written = 0;
    let mut values_read = 0;

    for i in 0..iterations {
        // Try to write
        if let Some((seq, slots)) = ring.try_claim_slots(1, cursor) {
            slots[0].value = i as u64;
            ring.publish(seq);
            cursor = seq + 1;
            values_written += 1;
        }

        // Read some to make space
        if i % 50 == 49 {
            let available = cursor.saturating_sub(cons_cursor);
            if available > 0 {
                let to_read = available.min(50);
                let slots = ring.get_read_batch(cons_cursor, to_read as usize);
                values_read += slots.len();
                cons_cursor += slots.len() as u64;
                ring.update_consumer(cons_cursor - 1);
            }
        }
    }

    // Drain remaining
    while cons_cursor < cursor {
        let available = cursor - cons_cursor;
        let to_read = available.min(50);
        let slots = ring.get_read_batch(cons_cursor, to_read as usize);
        values_read += slots.len();
        cons_cursor += slots.len() as u64;
        ring.update_consumer(cons_cursor - 1);
    }

    println!("\n=== Sequence Wraparound Test ===");
    println!("Iterations: {}", iterations);
    println!("Written: {}", values_written);
    println!("Read: {}", values_read);

    assert_eq!(values_written, values_read, "Write/read mismatch");
}

//! Average Calculator - SPMC (1 Producer, 4 Consumers)
//!
//! Demonstrates the read-then-commit pattern for safe multi-consumer access.
//! Each consumer uses RAII guards that automatically commit reads, ensuring
//! no data loss even on early exit (e.g., seeing a sentinel value).

use kaos::disruptor::{SpmcRingBuffer, Slot8};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::thread;
use std::time::Instant;

const RING_SIZE: usize = 1024 * 1024;
const MAX_NUMBER: u64 = 1_000_000;
const NUM_CONSUMERS: usize = 4;

fn main() {
    println!("\n╔════════════════════════════════════════════════════════╗");
    println!("║  Average Calculator - SPMC (4 Consumers)              ║");
    println!("║  Using Read-Then-Commit Pattern                       ║");
    println!("╚════════════════════════════════════════════════════════╝\n");

    println!("Task: Calculate average of numbers 1 to {}", MAX_NUMBER);
    println!("Strategy: 1 producer, 4 consumers with RAII guards\n");

    let ring_buffer = Arc::new(SpmcRingBuffer::<Slot8>::new(RING_SIZE).unwrap());

    let start = Instant::now();

    let total_sum = Arc::new(AtomicU64::new(0));
    let total_count = Arc::new(AtomicU64::new(0));

    // Producer thread
    let ring_buffer_producer = ring_buffer.clone();
    let producer_thread = thread::spawn(move || {
        let ring_buffer = ring_buffer_producer;
        let mut producer_cursor = 0u64;
        let mut sent = 0u64;
        
        println!("Producer starting...");
        
        // Send numbers 1 to MAX_NUMBER
        for number in 1..=MAX_NUMBER {
            loop {
                if let Some(next) = ring_buffer.try_claim(1, producer_cursor) {
                    unsafe {
                        ring_buffer.write_slot(producer_cursor, Slot8 { value: number });
                    }
                    ring_buffer.publish(next);
                    producer_cursor = next;
                    sent += 1;
                    break;
                }
                std::thread::yield_now();
            }
        }

        // Send sentinels (one per consumer)
        for _ in 0..NUM_CONSUMERS {
            loop {
                if let Some(next) = ring_buffer.try_claim(1, producer_cursor) {
                    unsafe {
                        ring_buffer.write_slot(producer_cursor, Slot8 { value: 0 });
                    }
                    ring_buffer.publish(next);
                    producer_cursor = next;
                    break;
                }
                std::thread::yield_now();
            }
        }

        println!("Producer: Sent {} numbers", sent);
        sent
    });

    // Consumer threads - using the safe read-then-commit API
    let mut consumer_threads = vec![];
    for consumer_id in 0..NUM_CONSUMERS {
        let ring_buffer_clone = ring_buffer.clone();
        let total_sum_clone = total_sum.clone();
        let total_count_clone = total_count.clone();
        
        let handle = thread::spawn(move || {
            let mut local_sum = 0u64;
            let mut local_count = 0u64;

            println!("Consumer {} starting...", consumer_id);
            
            loop {
                // Using try_read() returns a ReadGuard that automatically
                // commits the slot when dropped - even on early exit!
                if let Some(guard) = ring_buffer_clone.try_read() {
                    let slot = guard.get();
                    
                    if slot.value == 0 {
                        // Sentinel received - guard drops and commits automatically!
                        // No data loss even though we're breaking early.
                        break;
                    }
                    
                    local_sum += slot.value;
                    local_count += 1;
                    // guard drops here, slot committed
                } else {
                    std::hint::spin_loop();
                }
            }
            
            total_sum_clone.fetch_add(local_sum, Ordering::Relaxed);
            total_count_clone.fetch_add(local_count, Ordering::Relaxed);
            
            println!("Consumer {}: Processed {} numbers", consumer_id, local_count);
            local_count
        });
        consumer_threads.push(handle);
    }

    let sent = producer_thread.join().unwrap();

    let mut total_consumed = 0;
    for handle in consumer_threads {
        total_consumed += handle.join().unwrap();
    }

    let duration = start.elapsed();

    let sum = total_sum.load(Ordering::Relaxed);
    let count = total_count.load(Ordering::Relaxed);
    let average = sum as f64 / count as f64;

    let expected_sum: u64 = (MAX_NUMBER * (MAX_NUMBER + 1)) / 2;
    let expected_avg = expected_sum as f64 / MAX_NUMBER as f64;

    println!("\n╔════════════════════════════════════════════════════════╗");
    println!("║  RESULTS                                                ║");
    println!("╚════════════════════════════════════════════════════════╝");
    println!("  Numbers sent:         {}", sent);
    println!("  Numbers processed:    {}", count);
    println!("  Sum (calculated):     {}", sum);
    println!("  Sum (expected):       {}", expected_sum);
    println!("  Average (calculated): {:.1}", average);
    println!("  Average (expected):   {:.1}", expected_avg);
    println!("  Time taken:           {:.3}s", duration.as_secs_f64());
    println!();

    if sum == expected_sum && count == MAX_NUMBER {
        println!("  ✅ VERIFICATION PASSED!");
        println!("  ✨ All {} numbers transmitted correctly", MAX_NUMBER);
        println!("  ✨ Sum matches expected value");
        println!("  ✨ Average = {:.1} (exactly as expected)", average);
        println!();
        println!("  🚀 SPMC - 4 consumers with read-then-commit pattern!");
        println!("  🚀 RAII guards ensure no data loss on early exit!");
    } else {
        println!("  ❌ VERIFICATION FAILED!");
        println!("  Expected sum: {}, got: {}", expected_sum, sum);
        println!("  Expected count: {}, got: {}", MAX_NUMBER, count);
    }

    let throughput = sent as f64 / duration.as_secs_f64();
    println!("\n  Performance: {:.2}M numbers/sec", throughput / 1_000_000.0);
    println!("  Per consumer: {:.2}M numbers/sec\n", throughput / NUM_CONSUMERS as f64 / 1_000_000.0);
}

//! Average Calculator - MPMC (4 Producers, 4 Consumers)
//!
//! Demonstrates the read-then-commit pattern with multiple producers and consumers.
//! Both sides use atomic CAS operations, with RAII guards ensuring no data loss.

use kaos::disruptor::{MpmcRingBuffer, Slot8};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::thread;
use std::time::Instant;

const RING_SIZE: usize = 1024 * 1024;
const MESSAGES_PER_PRODUCER: u64 = 250_000;
const NUM_PRODUCERS: usize = 4;
const NUM_CONSUMERS: usize = 4;
const MAX_NUMBER: u64 = 1_000_000;

fn main() {
    println!("\n╔════════════════════════════════════════════════════════╗");
    println!("║  Average Calculator - MPMC (4P/4C)                     ║");
    println!("║  Using Read-Then-Commit Pattern                        ║");
    println!("╚════════════════════════════════════════════════════════╝\n");

    println!("Task: Calculate average of numbers 1 to {}", MAX_NUMBER);
    println!("Strategy: 4 producers, 4 consumers with RAII guards\n");

    let ring_buffer = Arc::new(MpmcRingBuffer::<Slot8>::new(RING_SIZE).unwrap());

    let start = Instant::now();

    let total_sum = Arc::new(AtomicU64::new(0));
    let total_count = Arc::new(AtomicU64::new(0));
    let total_sent = Arc::new(AtomicU64::new(0));

    // Producer threads
    let mut producer_threads = vec![];
    for producer_id in 0..NUM_PRODUCERS {
        let ring_buffer_clone = ring_buffer.clone();
        let total_sent_clone = total_sent.clone();
        
        let handle = thread::spawn(move || {
            let start_num = producer_id as u64 * MESSAGES_PER_PRODUCER + 1;
            let end_num = start_num + MESSAGES_PER_PRODUCER;
            
            let mut sent = 0u64;

            for number in start_num..end_num {
                loop {
                    if let Some(sequence) = ring_buffer_clone.try_claim(1) {
                        unsafe {
                            ring_buffer_clone.write_slot(sequence, Slot8 { value: number });
                        }
                        ring_buffer_clone.publish(sequence);
                        sent += 1;
                        break;
                    }
                    std::hint::spin_loop();
                }
            }

            total_sent_clone.fetch_add(sent, Ordering::Relaxed);
            println!("Producer {}: Sent {} numbers (range {} to {})", 
                     producer_id, sent, start_num, end_num - 1);
            sent
        });
        producer_threads.push(handle);
    }

    // Consumer threads - using the safe read-then-commit API
    let mut consumer_threads = vec![];
    for consumer_id in 0..NUM_CONSUMERS {
        let ring_buffer_clone = ring_buffer.clone();
        let total_sum_clone = total_sum.clone();
        let total_count_clone = total_count.clone();
        
        let handle = thread::spawn(move || {
            let mut local_sum = 0u64;
            let mut local_count = 0u64;

            loop {
                // Using try_read() returns a ReadGuard that automatically
                // commits the slot when dropped - even on early exit!
                if let Some(guard) = ring_buffer_clone.try_read() {
                    let slot = guard.get();
                    
                    if slot.value == 0 {
                        // Sentinel received - guard drops and commits automatically!
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

    // Wait for producers to finish
    let mut total_produced = 0;
    for handle in producer_threads {
        total_produced += handle.join().unwrap();
    }

    // Send sentinels (one per consumer)
    for _ in 0..NUM_CONSUMERS {
        loop {
            if let Some(sequence) = ring_buffer.try_claim(1) {
                unsafe {
                    ring_buffer.write_slot(sequence, Slot8 { value: 0 });
                }
                ring_buffer.publish(sequence);
                break;
            }
            std::hint::spin_loop();
        }
    }

    // Wait for consumers to finish
    let mut total_consumed = 0;
    for handle in consumer_threads {
        total_consumed += handle.join().unwrap();
    }

    let duration = start.elapsed();

    let sum = total_sum.load(Ordering::Relaxed);
    let count = total_count.load(Ordering::Relaxed);
    let sent = total_sent.load(Ordering::Relaxed);
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
        println!("  🚀 MPMC - Full multi-producer/multi-consumer!");
        println!("  🚀 Read-then-commit with RAII ensures no data loss!");
    } else {
        println!("  ❌ VERIFICATION FAILED!");
        println!("  Expected sum: {}, got: {}", expected_sum, sum);
        println!("  Expected count: {}, got: {}", MAX_NUMBER, count);
    }

    let throughput = sent as f64 / duration.as_secs_f64();
    println!("\n  Performance: {:.2}M numbers/sec", throughput / 1_000_000.0);
    println!("  Per producer: {:.2}M numbers/sec\n", throughput / NUM_PRODUCERS as f64 / 1_000_000.0);
}

//! Average Calculator - Clean API Example
//!
//! Demonstrates the high-level Producer/Consumer API (LMAX Disruptor style)
//! Calculate average of 1 to 1,000,000 with beautiful, safe code!

use flux::disruptor::{ConsumerBuilder, ProducerBuilder, MessageRingBuffer, RingBufferConfig, WaitStrategyType, EventHandler, MessageSlot};
use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use std::thread;
use std::time::Instant;

const RING_SIZE: usize = 1024 * 1024;
const BATCH_SIZE: usize = 8192;
const MAX_NUMBER: u64 = 1_000_000;

fn main() {
    println!("\n╔════════════════════════════════════════════════════════╗");
    println!("║  Average Calculator - Clean API (LMAX Style)          ║");
    println!("╚════════════════════════════════════════════════════════╝\n");

    println!("Task: Calculate average of numbers 1 to {}\n", MAX_NUMBER);

    // Create ring buffer with clean config
    let config = RingBufferConfig {
        size: RING_SIZE,
        num_consumers: 1,
        wait_strategy: WaitStrategyType::BusySpin,
        optimal_batch_size: BATCH_SIZE,
        enable_cache_prefetch: true,
        enable_simd: false,
        strict_message_ordering: true,
        block_on_full: false,
    };

    let ring_buffer = Arc::new(MessageRingBuffer::new(config).unwrap());
    let stop = Arc::new(AtomicBool::new(false));

    // Producer with clean API
    let mut producer = ProducerBuilder::new()
        .with_ring_buffer(ring_buffer.clone())
        .build()
        .unwrap();

    let consumer = ConsumerBuilder::new()
        .with_ring_buffer(ring_buffer.clone())
        .with_consumer_id(0)
        .with_batch_size(BATCH_SIZE)
        .build()
        .unwrap();

    let stop_clone = stop.clone();
    let start = Instant::now();

    // Producer thread - Beautiful API with batching!
    let producer_thread = thread::spawn(move || {
        let mut sent = 0u64;
        let mut number = 1u64;
        
        while number <= MAX_NUMBER {
            let remaining = MAX_NUMBER - number + 1;
            let to_send = remaining.min(BATCH_SIZE as u64) as usize;
            
            let batch: Vec<u64> = (number..number + to_send as u64).collect();
            
            match producer.publish_batch(&batch, |event, _seq, &value| {
                event.sequence = value;
                event.data[0..8].copy_from_slice(&value.to_le_bytes());
            }) {
                Ok(count) => {
                    sent += count as u64;
                    number += count as u64;
                }
                Err(_) => std::thread::yield_now(),
            }
        }
        
        println!("Producer: Sent {} numbers", sent);
        sent
    });

    // Consumer thread - Clean event handler!
    struct AverageHandler {
        sum: u64,
        count: u64,
        first_value: Option<u64>,
        last_value: Option<u64>,
    }

    impl EventHandler for AverageHandler {
        fn on_event(&mut self, event: &MessageSlot, _seq: u64, _end_of_batch: bool) {
            let value = event.sequence;
            if self.first_value.is_none() {
                self.first_value = Some(value);
            }
            self.last_value = Some(value);
            self.sum += value;
            self.count += 1;
        }
    }

    let mut handler = AverageHandler { 
        sum: 0, 
        count: 0,
        first_value: None,
        last_value: None,
    };
    
    let consumer_thread = thread::spawn(move || {
        // Run until we've processed all numbers
        while handler.count < MAX_NUMBER {
            consumer.process_events(&mut handler);
            std::hint::spin_loop();
        }
        
        println!("Consumer: Received {} numbers (first={:?}, last={:?})", 
                 handler.count, handler.first_value, handler.last_value);
        (handler.sum, handler.count)
    });

    // Wait for completion
    let sent = producer_thread.join().unwrap();
    let (sum, count) = consumer_thread.join().unwrap();
    let duration = start.elapsed();

    // Verify results
    let calculated_average = sum as f64 / count as f64;
    let expected_average = (MAX_NUMBER as f64 + 1.0) / 2.0;
    let expected_sum = (MAX_NUMBER * (MAX_NUMBER + 1)) / 2;

    println!("\n╔════════════════════════════════════════════════════════╗");
    println!("  ║  RESULTS                                               ║");
    println!("  ╚════════════════════════════════════════════════════════╝");
    println!("  Numbers processed:    {}", count);
    println!("  Sum (calculated):     {}", sum);
    println!("  Sum (expected):       {}", expected_sum);
    println!("  Average (calculated): {:.1}", calculated_average);
    println!("  Average (expected):   {:.1}", expected_average);
    println!("  Time taken:           {:.3}s", duration.as_secs_f64());
    println!();

    if count == MAX_NUMBER && sum == expected_sum {
        println!("  ✅ VERIFICATION PASSED!");
        println!("  ✨ All {} numbers transmitted correctly", count);
        println!("  ✨ Sum matches expected value");
        println!("  ✨ Average = {:.1} (exactly as expected)", calculated_average);
        println!();
        println!("  🎯 CLEAN API - No unsafe code needed!");
        println!("  🎯 LMAX Disruptor style - Beautiful DevX!");
    } else {
        println!("  ❌ VERIFICATION FAILED!");
        if count != sent {
            println!("  ⚠️  Count mismatch: got {}, expected {}", count, sent);
        }
        if sum != expected_sum {
            println!("  ⚠️  Sum mismatch: got {}, expected {}", sum, expected_sum);
        }
    }

    let throughput = count as f64 / duration.as_secs_f64();
    println!("\n  Performance: {:.2}M numbers/sec\n", throughput / 1_000_000.0);
}

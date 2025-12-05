//! Average Calculator - MPSC (4 Producers, 1 Consumer)
//!
//! Each producer sends 250k numbers, consumer calculates average of all 1M

use kaos::disruptor::{
    MpscConsumerBuilder, MpscEventHandler, MpscProducerBuilder, MpscRingBuffer, Slot8,
};
use std::sync::Arc;
use std::thread;
use std::time::Instant;

const RING_SIZE: usize = 1024 * 1024;
const BATCH_SIZE: usize = 8192;
const MESSAGES_PER_PRODUCER: u64 = 250_000;
const NUM_PRODUCERS: usize = 4;
const MAX_NUMBER: u64 = 1_000_000;

fn main() {
    println!("\n╔════════════════════════════════════════════════════════╗");
    println!("  ║  Average Calculator - MPSC (4 Producers)               ║");
    println!("  ╚════════════════════════════════════════════════════════╝\n");

    println!("Task: Calculate average of numbers 1 to {}", MAX_NUMBER);
    println!("Strategy: 4 producers each send 250k sequential numbers\n");

    let ring_buffer = Arc::new(MpscRingBuffer::<Slot8>::new(RING_SIZE).unwrap());

    let start = Instant::now();

    // Spawn 4 producer threads - each sends sequential chunk
    let mut producer_threads = vec![];
    for producer_id in 0..NUM_PRODUCERS {
        let ring_buffer_clone = ring_buffer.clone();

        let handle = thread::spawn(move || {
            let producer = MpscProducerBuilder::new()
                .with_ring_buffer(ring_buffer_clone)
                .build()
                .unwrap();

            // Each producer sends a sequential range
            let start_num = producer_id as u64 * MESSAGES_PER_PRODUCER + 1;
            let end_num = start_num + MESSAGES_PER_PRODUCER;

            let mut sent = 0u64;
            let mut number = start_num;

            while number < end_num {
                let remaining = end_num - number;
                let to_send = remaining.min(BATCH_SIZE as u64) as usize;

                // Capture number before the batch
                let start_of_batch = number;

                // Try to publish batch
                match producer.publish_batch(to_send, |i, slot| {
                    slot.value = start_of_batch + i as u64;
                }) {
                    Ok(count) => {
                        number += count as u64;
                        sent += count as u64;
                    }
                    Err(_) => {
                        std::thread::yield_now();
                    }
                }
            }

            println!(
                "Producer {}: Sent {} numbers (range {} to {})",
                producer_id,
                sent,
                start_num,
                end_num - 1
            );
            sent
        });

        producer_threads.push(handle);
    }

    // Consumer thread - calculates sum and average
    let consumer_thread = thread::spawn(move || {
        let mut consumer = MpscConsumerBuilder::new()
            .with_ring_buffer(ring_buffer)
            .with_batch_size(BATCH_SIZE)
            .build()
            .unwrap();

        struct AverageHandler {
            sum: u64,
            count: u64,
            first_value: Option<u64>,
            last_value: Option<u64>,
        }

        impl MpscEventHandler<Slot8> for AverageHandler {
            fn on_event(&mut self, event: &Slot8, _seq: u64, _end_of_batch: bool) {
                if self.first_value.is_none() {
                    self.first_value = Some(event.value);
                }
                self.last_value = Some(event.value);
                self.sum += event.value;
                self.count += 1;
            }
        }

        let mut handler = AverageHandler {
            sum: 0,
            count: 0,
            first_value: None,
            last_value: None,
        };

        while handler.count < MAX_NUMBER {
            consumer.process_events(&mut handler);
            std::hint::spin_loop();
        }

        println!(
            "Consumer: Received {} numbers (first={:?}, last={:?})",
            handler.count, handler.first_value, handler.last_value
        );
        (handler.sum, handler.count)
    });

    // Wait for completion
    let mut total_sent = 0;
    for handle in producer_threads {
        total_sent += handle.join().unwrap();
    }

    let (sum, count) = consumer_thread.join().unwrap();
    let duration = start.elapsed();

    // Verify results
    let calculated_average = sum as f64 / count as f64;
    let expected_average = (MAX_NUMBER as f64 + 1.0) / 2.0;
    let expected_sum = (MAX_NUMBER * (MAX_NUMBER + 1)) / 2;

    println!("\n╔════════════════════════════════════════════════════════╗");
    println!("║  RESULTS                                                ║");
    println!("╚════════════════════════════════════════════════════════╝");
    println!("  Numbers sent:         {}", total_sent);
    println!("  Numbers processed:    {}", count);
    println!("  Sum (calculated):     {}", sum);
    println!("  Sum (expected):       {}", expected_sum);
    println!("  Average (calculated): {:.1}", calculated_average);
    println!("  Average (expected):   {:.1}", expected_average);
    println!("  Time taken:           {:.3}s", duration.as_secs_f64());
    println!();

    if count == total_sent && sum == expected_sum {
        println!("  ✅ VERIFICATION PASSED!");
        println!("  ✨ All {} numbers transmitted correctly", count);
        println!("  ✨ Sum matches expected value");
        println!(
            "  ✨ Average = {:.1} (exactly as expected)",
            calculated_average
        );
        println!();
        println!("  🚀 MPSC - 4 producers coordinated via CAS!");
        println!("  🚀 MPSC pattern");
    } else {
        println!("  ❌ VERIFICATION FAILED!");
        if count != total_sent {
            println!(
                "  ⚠️  Count mismatch: got {}, expected {}",
                count, total_sent
            );
        }
        if sum != expected_sum {
            println!("  ⚠️  Sum mismatch: got {}, expected {}", sum, expected_sum);
        }
    }

    let throughput = count as f64 / duration.as_secs_f64();
    println!(
        "\n  Performance: {:.2}M numbers/sec",
        throughput / 1_000_000.0
    );
    println!(
        "  Per producer: {:.2}M numbers/sec\n",
        throughput / 1_000_000.0 / NUM_PRODUCERS as f64
    );
}

//! Simple SPSC Example
//!
//! Demonstrates the basic RingBuffer<T> API for single-producer single-consumer.

use kaos::disruptor::{RingBuffer, RingBufferEntry, Slot8};
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::thread;

const RING_SIZE: usize = 1024 * 64;
const MESSAGE_COUNT: u64 = 1_000_000;
const BATCH_SIZE: usize = 1024;

fn main() {
    println!("\n=== Simple SPSC Example ===\n");

    // Create ring buffer (heap allocation)
    let ring = Arc::new(RingBuffer::<Slot8>::new(RING_SIZE).unwrap());

    // Or use memory-mapped for better performance:
    // let ring = Arc::new(RingBuffer::<Slot8>::new_mapped(RING_SIZE).unwrap());

    let producer_cursor = ring.producer_cursor();

    // Producer thread
    let ring_prod = ring.clone();
    let producer = thread::spawn(move || {
        let mut cursor = 0u64;
        let mut sent = 0u64;

        while sent < MESSAGE_COUNT {
            let remaining = (MESSAGE_COUNT - sent) as usize;
            let batch = remaining.min(BATCH_SIZE);

            // Claim slots and write
            if let Some((seq, slots)) = ring_prod.try_claim_slots(batch, cursor) {
                for (i, slot) in slots.iter_mut().enumerate() {
                    slot.set_sequence(sent + i as u64 + 1);
                }
                let next = seq + slots.len() as u64;
                ring_prod.publish(next);
                cursor = next;
                sent += slots.len() as u64;
            }
        }

        println!("Producer: sent {} messages", sent);
    });

    // Consumer thread
    let ring_cons = ring.clone();
    let consumer = thread::spawn(move || {
        let mut cursor = 0u64;
        let mut received = 0u64;
        let mut sum = 0u64;

        while received < MESSAGE_COUNT {
            let prod_seq = producer_cursor.load(Ordering::Acquire);
            let available = prod_seq.saturating_sub(cursor);

            if available > 0 {
                let to_read = (available as usize).min(BATCH_SIZE);
                let slots = ring_cons.get_read_batch(cursor, to_read);

                for slot in slots {
                    sum += slot.sequence();
                    received += 1;
                }

                cursor += slots.len() as u64;
                ring_cons.update_consumer(cursor);
            }
        }

        println!("Consumer: received {} messages, sum = {}", received, sum);
        sum
    });

    let start = std::time::Instant::now();
    producer.join().unwrap();
    let sum = consumer.join().unwrap();
    let duration = start.elapsed();

    // Verify
    let expected = (MESSAGE_COUNT * (MESSAGE_COUNT + 1)) / 2;
    assert_eq!(sum, expected, "Sum mismatch!");

    let throughput = MESSAGE_COUNT as f64 / duration.as_secs_f64() / 1_000_000.0;
    println!("\nThroughput: {:.2}M msgs/sec", throughput);
    println!("Verified: sum = {} (expected {})", sum, expected);
}

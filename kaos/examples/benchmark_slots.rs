//! Slot Sizes Example
//!
//! Demonstrates different slot sizes and their performance characteristics.

use kaos::disruptor::{RingBuffer, RingBufferEntry, Slot8, Slot16, Slot32, Slot64};
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::thread;
use std::time::Instant;

const RING_SIZE: usize = 1024 * 1024;
const MESSAGE_COUNT: u64 = 10_000_000;
const BATCH_SIZE: usize = 8192;

fn benchmark<T: RingBufferEntry + Copy + Default + Send + Sync + 'static>(name: &str) -> f64 {
    let ring = Arc::new(RingBuffer::<T>::new(RING_SIZE).unwrap());
    let producer_cursor = ring.producer_cursor();

    let ring_prod = ring.clone();
    let producer = thread::spawn(move || {
        let mut cursor = 0u64;
        let mut sent = 0u64;

        while sent < MESSAGE_COUNT {
            let remaining = (MESSAGE_COUNT - sent) as usize;
            let batch = remaining.min(BATCH_SIZE);

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
    });

    let ring_cons = ring.clone();
    let consumer = thread::spawn(move || {
        let mut cursor = 0u64;
        let mut received = 0u64;

        while received < MESSAGE_COUNT {
            let prod_seq = producer_cursor.load(Ordering::Acquire);
            let available = prod_seq.saturating_sub(cursor);

            if available > 0 {
                let to_read = (available as usize).min(BATCH_SIZE);
                let slots = ring_cons.get_read_batch(cursor, to_read);
                received += slots.len() as u64;
                cursor += slots.len() as u64;
                ring_cons.update_consumer(cursor);
            }
        }
    });

    let start = Instant::now();
    producer.join().unwrap();
    consumer.join().unwrap();
    let duration = start.elapsed();

    let throughput = MESSAGE_COUNT as f64 / duration.as_secs_f64() / 1_000_000.0;
    println!("  {:<12} {:>8.2}M msgs/sec", name, throughput);
    throughput
}

fn main() {
    println!("\n=== Slot Sizes Comparison ===\n");
    println!("Ring size: {}M slots", RING_SIZE / 1_000_000);
    println!("Messages:  {}M\n", MESSAGE_COUNT / 1_000_000);

    println!("Results:");
    let t8 = benchmark::<Slot8>("Slot8(8B)");
    let t16 = benchmark::<Slot16>("Slot16(16B)");
    let t32 = benchmark::<Slot32>("Slot32(32B)");
    let t64 = benchmark::<Slot64>("Slot64(64B)");

    println!("\nSummary:");
    println!("  Slot32 benefits from cache alignment");
    println!("  Slot8: {:.0}% of Slot32", t8 / t32 * 100.0);
    println!("  Slot16:    {:.0}% of Slot32", t16 / t32 * 100.0);
    println!("  Slot64:    {:.0}% of Slot32", t64 / t32 * 100.0);
}


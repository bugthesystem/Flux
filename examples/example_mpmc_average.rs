use flux::disruptor::{ RingBuffer, RingBufferConfig };
use flux::disruptor::RingBufferEntry;
use std::thread;
use std::sync::{ Arc, Mutex };
use std::sync::atomic::{ AtomicU64, Ordering };

const NUM_PRODUCERS: usize = 4;
const NUM_CONSUMERS: usize = 4;
const NUM_MESSAGES_PER_PRODUCER: u64 = 2_500_000;
const NUM_MESSAGES: u64 = (NUM_PRODUCERS as u64) * NUM_MESSAGES_PER_PRODUCER;
const RING_BUFFER_SIZE: usize = 65536;

fn main() {
    let config = RingBufferConfig::new(RING_BUFFER_SIZE).unwrap();
    let ring_buffer = Arc::new(Mutex::new(RingBuffer::new(config).unwrap()));
    let total_sum = Arc::new(AtomicU64::new(0));
    let total_count = Arc::new(AtomicU64::new(0));

    // Producers
    let mut producer_handles = Vec::new();
    for p in 0..NUM_PRODUCERS {
        let ring_buffer = Arc::clone(&ring_buffer);
        let start = (p as u64) * NUM_MESSAGES_PER_PRODUCER + 1;
        let end = start + NUM_MESSAGES_PER_PRODUCER;
        producer_handles.push(
            thread::spawn(move || {
                for i in start..end {
                    let data = i.to_le_bytes();
                    loop {
                        let mut buffer = ring_buffer.lock().unwrap();
                        if let Some((seq, slots)) = buffer.try_claim_slots(1) {
                            slots[0].set_sequence(seq);
                            slots[0].set_data(&data);
                            buffer.publish_batch(seq, 1);
                            break;
                        }
                        drop(buffer);
                        thread::yield_now();
                    }
                }
            })
        );
    }

    // Consumers
    let mut consumer_handles = Vec::new();
    for _ in 0..NUM_CONSUMERS {
        let ring_buffer = Arc::clone(&ring_buffer);
        let total_sum = Arc::clone(&total_sum);
        let total_count = Arc::clone(&total_count);
        consumer_handles.push(
            thread::spawn(move || {
                loop {
                    let values: Vec<u64> = {
                        let buffer = ring_buffer.lock().unwrap();
                        buffer
                            .try_consume_batch(0, 8)
                            .iter()
                            .filter_map(|m| {
                                if m.data().len() == std::mem::size_of::<u64>() {
                                    Some(u64::from_le_bytes(m.data().try_into().unwrap()))
                                } else {
                                    None
                                }
                            })
                            .collect()
                    };
                    if !values.is_empty() {
                        for value in values {
                            total_sum.fetch_add(value, Ordering::Relaxed);
                            total_count.fetch_add(1, Ordering::Relaxed);
                        }
                    } else {
                        if total_count.load(Ordering::Relaxed) >= NUM_MESSAGES {
                            break;
                        }
                        thread::yield_now();
                    }
                }
            })
        );
    }

    for handle in producer_handles {
        handle.join().unwrap();
    }
    for handle in consumer_handles {
        handle.join().unwrap();
    }

    let sum = total_sum.load(Ordering::Relaxed);
    let count = total_count.load(Ordering::Relaxed);
    let expected_sum: u64 = (1..=NUM_MESSAGES).sum();
    let average = (sum as f64) / (count as f64);
    let expected_average = (expected_sum as f64) / (NUM_MESSAGES as f64);

    println!("--- MPMC Average Calculation Test ---");
    println!("Producers: {}", NUM_PRODUCERS);
    println!("Consumers: {}", NUM_CONSUMERS);
    println!("Messages processed: {}", count);
    println!("Calculated average: {}", average);
    println!("Expected average:   {}", expected_average);

    assert_eq!(sum, expected_sum, "Data integrity check failed: The sum is incorrect.");
    println!("\nTest passed: Data integrity verified.");
}

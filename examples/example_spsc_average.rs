use flux::disruptor::{ RingBuffer, RingBufferConfig };
use flux::disruptor::RingBufferEntry;
use std::thread;
use std::sync::{ Arc, Mutex };

const NUM_MESSAGES: u64 = 1_000_000;
const RING_BUFFER_SIZE: usize = 65536;

fn main() {
    let config = RingBufferConfig::new(RING_BUFFER_SIZE).unwrap();
    let ring_buffer = Arc::new(Mutex::new(RingBuffer::new(config).unwrap()));

    let producer_handle = {
        let ring_buffer = Arc::clone(&ring_buffer);
        thread::spawn(move || {
            for i in 1..=NUM_MESSAGES {
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
    };

    let consumer_handle = {
        let ring_buffer = Arc::clone(&ring_buffer);
        thread::spawn(move || {
            let mut sum = 0;
            let mut count = 0;
            while count < NUM_MESSAGES {
                let values: Vec<u64> = {
                    let buffer = ring_buffer.lock().unwrap();
                    buffer
                        .try_consume_batch(0, 1)
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
                        sum += value;
                        count += 1;
                    }
                } else {
                    thread::yield_now();
                }
            }
            sum
        })
    };

    producer_handle.join().unwrap();
    let total_sum = consumer_handle.join().unwrap();

    let expected_sum: u64 = (1..=NUM_MESSAGES).sum();
    let average = (total_sum as f64) / (NUM_MESSAGES as f64);
    let expected_average = (expected_sum as f64) / (NUM_MESSAGES as f64);

    println!("--- SPSC Average Calculation Test ---");
    println!("Messages processed: {}", NUM_MESSAGES);
    println!("Calculated average: {}", average);
    println!("Expected average:   {}", expected_average);

    assert_eq!(total_sum, expected_sum, "Data integrity check failed: The sum is incorrect.");
    println!("\nTest passed: Data integrity verified.");
}

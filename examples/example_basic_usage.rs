use flux::{ RingBuffer, FluxError };
use flux::disruptor::{ WaitStrategyType, RingBufferConfig, RingBufferEntry };
use std::thread;
use std::time::{ Duration, Instant };

fn main() -> Result<(), FluxError> {
    println!("Flux Basic Usage Example");
    println!("============================");

    // Create a ring buffer with 1024 slots
    let config = RingBufferConfig::new(1024).unwrap();
    let mut buffer = RingBuffer::new(config)?;

    println!("Created ring buffer with 1024 slots");

    // Example 1: Single message operations
    println!("\nExample 1: Single Message Operations");
    single_message_example(&mut buffer)?;

    // Example 2: Batch operations
    println!("\nExample 2: Batch Operations");
    batch_operations_example(&mut buffer)?;

    // Example 3: Producer-consumer pattern
    println!("\nExample 3: Producer-Consumer Pattern");
    producer_consumer_example()?;

    // Example 4: Different wait strategies
    println!("\nExample 4: Wait Strategies");
    wait_strategies_example()?;

    println!("\nAll examples completed successfully!");
    Ok(())
}

fn single_message_example(buffer: &mut RingBuffer) -> Result<(), FluxError> {
    println!("  Sending single messages...");

    // Send messages one by one
    for i in 0..5 {
        let message = format!("Hello, Flux! Message {}", i);

        // Claim a slot and write the message
        if let Some((seq, slots)) = buffer.try_claim_slots(1) {
            slots[0].set_data(message.as_bytes());
            buffer.publish_batch(seq, 1);
            println!("  Sent: {}", message);
        } else {
            println!("  Failed to claim slot for message {}", i);
        }
    }

    println!("  Reading messages...");

    // Read messages one by one
    let mut received_count = 0;
    while let Ok(message) = buffer.try_consume(0) {
        let content = String::from_utf8_lossy(message.data());
        println!("  Received: {}", content);
        received_count += 1;

        if received_count >= 5 {
            break;
        }
    }

    println!("  Sent: 5, Received: {}", received_count);
    Ok(())
}

fn batch_operations_example(buffer: &mut RingBuffer) -> Result<(), FluxError> {
    println!("  Sending batch of messages...");

    const BATCH_SIZE: usize = 10;
    let start_time = Instant::now();

    // Send messages in batches
    if let Some((seq, slots)) = buffer.try_claim_slots(BATCH_SIZE) {
        for (i, slot) in slots.iter_mut().enumerate() {
            let message = format!("Batch message {}", i);
            slot.set_data(message.as_bytes());
        }
        buffer.publish_batch(seq, BATCH_SIZE);
        println!("  Sent batch of {} messages", BATCH_SIZE);
    } else {
        println!("  Failed to claim batch of slots");
    }

    // Read messages in batches
    let messages = buffer.try_consume_batch(0, BATCH_SIZE);
    for (i, message) in messages.iter().enumerate() {
        let content = String::from_utf8_lossy(message.data());
        println!("  Batch[{}]: {}", i, content);
    }

    let duration = start_time.elapsed();
    println!("  Batch processing took: {:?}", duration);
    Ok(())
}

fn producer_consumer_example() -> Result<(), FluxError> {
    println!("  Starting producer-consumer threads...");

    let config = RingBufferConfig::new(4096).unwrap();
    let buffer = RingBuffer::new(config)?;
    let buffer = std::sync::Arc::new(std::sync::Mutex::new(buffer));

    let messages_to_send = 1000;

    // Producer thread
    let producer_buffer = buffer.clone();
    let producer_handle = thread::spawn(move || {
        let start_time = Instant::now();

        for i in 0..messages_to_send {
            let message = format!("Message {}", i);

            // Retry until we can send
            loop {
                let mut buf = producer_buffer.lock().unwrap();
                if let Some((seq, slots)) = buf.try_claim_slots(1) {
                    slots[0].set_sequence(seq);
                    slots[0].set_data(message.as_bytes());
                    buf.publish_batch(seq, 1);
                    break;
                }
                drop(buf); // Release lock before yielding
                thread::yield_now();
            }
        }

        let duration = start_time.elapsed();
        let throughput = (messages_to_send as f64) / duration.as_secs_f64();
        println!("  Producer finished: {:.0} messages/sec", throughput);
    });

    // Consumer thread
    let consumer_buffer = buffer.clone();
    let consumer_handle = thread::spawn(move || {
        let mut received_count = 0;
        let start_time = Instant::now();
        let mut consecutive_empty = 0;

        while received_count < messages_to_send {
            let mut buf = consumer_buffer.lock().unwrap();
            let messages = buf.try_consume_batch(0, 10);
            if !messages.is_empty() {
                received_count += messages.len();
                consecutive_empty = 0;
            } else {
                consecutive_empty += 1;
                drop(buf); // Release lock before sleeping/yielding
                // If we've been empty for too long, yield more aggressively
                if consecutive_empty > 100 {
                    thread::sleep(Duration::from_micros(1));
                } else {
                    thread::yield_now();
                }
            }
        }

        let duration = start_time.elapsed();
        let throughput = (received_count as f64) / duration.as_secs_f64();
        println!("  Consumer finished: {:.0} messages/sec", throughput);
    });

    // Wait for both threads to complete
    producer_handle.join().unwrap();
    consumer_handle.join().unwrap();

    println!("  Producer-consumer example completed");
    Ok(())
}

fn wait_strategies_example() -> Result<(), FluxError> {
    println!("  Testing different wait strategies...");

    let strategies = vec![
        ("BusySpin", WaitStrategyType::BusySpin),
        ("Blocking", WaitStrategyType::Blocking),
        ("Sleeping", WaitStrategyType::Sleeping)
    ];

    for (name, strategy) in strategies {
        println!("  Testing {} strategy...", name);

        let config = RingBufferConfig::new(1024).unwrap().with_wait_strategy(strategy);
        let mut buffer = RingBuffer::new(config)?;

        let start_time = Instant::now();
        let test_messages = 100;

        // Send messages
        for i in 0..test_messages {
            let message = format!("Test message {}", i);

            if let Some((seq, slots)) = buffer.try_claim_slots(1) {
                slots[0].set_sequence(seq);
                slots[0].set_data(message.as_bytes());
                buffer.publish_batch(seq, 1);
            }
        }

        // Receive messages
        let mut received = 0;
        let mut consecutive_empty = 0;
        while received < test_messages {
            let messages = buffer.try_consume_batch(0, 10);
            if !messages.is_empty() {
                received += messages.len();
                consecutive_empty = 0;
            } else {
                consecutive_empty += 1;
                // If we've been empty for too long, yield more aggressively
                if consecutive_empty > 50 {
                    thread::sleep(Duration::from_micros(1));
                } else {
                    thread::yield_now();
                }
            }
        }

        let duration = start_time.elapsed();
        let throughput = (test_messages as f64) / duration.as_secs_f64();

        println!("    {}: {:.0} messages/sec ({:?})", name, throughput, duration);
    }

    Ok(())
}

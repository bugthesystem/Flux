use flux::{ RingBuffer, WaitStrategy, FluxError };
use std::thread;
use std::time::{ Duration, Instant };

fn main() -> Result<(), FluxError> {
    println!("🚀 Flux Basic Usage Example");
    println!("============================");

    // Create a ring buffer with 1024 slots
    let buffer = RingBuffer::new(1024, WaitStrategy::BusySpin);

    // Create producer and consumer
    let mut producer = buffer.create_producer();
    let mut consumer = buffer.create_consumer();

    println!("✅ Created ring buffer with 1024 slots");

    // Example 1: Single message operations
    println!("\n📝 Example 1: Single Message Operations");
    single_message_example(&mut producer, &mut consumer)?;

    // Example 2: Batch operations
    println!("\n📦 Example 2: Batch Operations");
    batch_operations_example(&mut producer, &mut consumer)?;

    // Example 3: Producer-consumer pattern
    println!("\n🔄 Example 3: Producer-Consumer Pattern");
    producer_consumer_example()?;

    // Example 4: Different wait strategies
    println!("\n⏱️ Example 4: Wait Strategies");
    wait_strategies_example()?;

    println!("\n🎉 All examples completed successfully!");
    Ok(())
}

fn single_message_example(
    producer: &mut impl Producer,
    consumer: &mut impl Consumer
) -> Result<(), FluxError> {
    println!("  Sending single messages...");

    // Send messages one by one
    for i in 0..5 {
        let message = format!("Hello, Flux! Message {}", i);

        // Claim a slot and write the message
        if let Ok(slot) = producer.try_claim_slot() {
            slot.write_message(message.as_bytes());
            producer.publish_single();
            println!("  ✅ Sent: {}", message);
        } else {
            println!("  ❌ Failed to claim slot for message {}", i);
        }
    }

    println!("  Reading messages...");

    // Read messages one by one
    let mut received_count = 0;
    while let Some(message) = consumer.try_consume_single() {
        let content = String::from_utf8_lossy(message.data());
        println!("  📨 Received: {}", content);
        received_count += 1;
    }

    println!("  📊 Sent: 5, Received: {}", received_count);
    Ok(())
}

fn batch_operations_example(
    producer: &mut impl Producer,
    consumer: &mut impl Consumer
) -> Result<(), FluxError> {
    println!("  Sending batch of messages...");

    const BATCH_SIZE: usize = 10;
    let start_time = Instant::now();

    // Send messages in batches
    if let Ok(slots) = producer.try_claim_slots(BATCH_SIZE) {
        for (i, slot) in slots.iter().enumerate() {
            let message = format!("Batch message {}", i);
            slot.write_message(message.as_bytes());
        }
        producer.publish_batch(BATCH_SIZE);
        println!("  ✅ Sent batch of {} messages", BATCH_SIZE);
    } else {
        println!("  ❌ Failed to claim batch of slots");
    }

    // Read messages in batches
    if let Some(batch) = consumer.try_consume_batch(BATCH_SIZE) {
        for (i, message) in batch.iter().enumerate() {
            let content = String::from_utf8_lossy(message.data());
            println!("  📨 Batch[{}]: {}", i, content);
        }
    }

    let duration = start_time.elapsed();
    println!("  📊 Batch processing took: {:?}", duration);
    Ok(())
}

fn producer_consumer_example() -> Result<(), FluxError> {
    println!("  Starting producer-consumer threads...");

    let buffer = RingBuffer::new(4096, WaitStrategy::BusySpin);
    let producer = buffer.create_producer();
    let consumer = buffer.create_consumer();

    let messages_to_send = 1000;

    // Producer thread
    let producer_handle = thread::spawn(move || {
        let mut producer = producer;
        let start_time = Instant::now();

        for i in 0..messages_to_send {
            let message = format!("Message {}", i);

            // Retry until we can send
            loop {
                if let Ok(slot) = producer.try_claim_slot() {
                    slot.write_message(message.as_bytes());
                    producer.publish_single();
                    break;
                }
                // Brief yield to avoid busy waiting
                thread::yield_now();
            }
        }

        let duration = start_time.elapsed();
        let throughput = (messages_to_send as f64) / duration.as_secs_f64();
        println!("  📤 Producer finished: {:.0} messages/sec", throughput);
    });

    // Consumer thread
    let consumer_handle = thread::spawn(move || {
        let mut consumer = consumer;
        let mut received_count = 0;
        let start_time = Instant::now();

        while received_count < messages_to_send {
            if let Some(message) = consumer.try_consume_single() {
                let _content = String::from_utf8_lossy(message.data());
                received_count += 1;
            } else {
                // Brief yield to avoid busy waiting
                thread::yield_now();
            }
        }

        let duration = start_time.elapsed();
        let throughput = (received_count as f64) / duration.as_secs_f64();
        println!("  📥 Consumer finished: {:.0} messages/sec", throughput);
    });

    // Wait for both threads to complete
    producer_handle.join().unwrap();
    consumer_handle.join().unwrap();

    println!("  ✅ Producer-consumer example completed");
    Ok(())
}

fn wait_strategies_example() -> Result<(), FluxError> {
    println!("  Testing different wait strategies...");

    let strategies = vec![
        ("BusySpin", WaitStrategy::BusySpin),
        ("Yielding", WaitStrategy::Yielding),
        ("Sleeping", WaitStrategy::Sleeping),
        ("Blocking", WaitStrategy::Blocking),
        (
            "Timeout",
            WaitStrategy::Timeout {
                timeout: Duration::from_millis(1),
            },
        )
    ];

    for (name, strategy) in strategies {
        println!("  🔄 Testing {} strategy...", name);

        let buffer = RingBuffer::new(1024, strategy);
        let mut producer = buffer.create_producer();
        let mut consumer = buffer.create_consumer();

        let start_time = Instant::now();
        let test_messages = 100;

        // Send messages
        for i in 0..test_messages {
            let message = format!("Test message {}", i);

            if let Ok(slot) = producer.try_claim_slot() {
                slot.write_message(message.as_bytes());
                producer.publish_single();
            }
        }

        // Receive messages
        let mut received = 0;
        while received < test_messages {
            if let Some(_message) = consumer.try_consume_single() {
                received += 1;
            } else if matches!(strategy, WaitStrategy::BusySpin) {
                // For BusySpin, we might need to yield occasionally
                thread::yield_now();
            }
        }

        let duration = start_time.elapsed();
        let throughput = (test_messages as f64) / duration.as_secs_f64();

        println!("    ✅ {}: {:.0} messages/sec ({:?})", name, throughput, duration);
    }

    Ok(())
}

// Mock traits for the example (these would be provided by Flux)
trait Producer {
    fn try_claim_slot(&mut self) -> Result<MockSlot, FluxError>;
    fn try_claim_slots(&mut self, count: usize) -> Result<Vec<MockSlot>, FluxError>;
    fn publish_single(&mut self);
    fn publish_batch(&mut self, count: usize);
}

trait Consumer {
    fn try_consume_single(&mut self) -> Option<MockMessage>;
    fn try_consume_batch(&mut self, count: usize) -> Option<Vec<MockMessage>>;
}

// Mock implementations (these would be provided by Flux)
struct MockSlot {
    data: Vec<u8>,
}

impl MockSlot {
    fn write_message(&mut self, data: &[u8]) {
        self.data = data.to_vec();
    }
}

struct MockMessage {
    data: Vec<u8>,
}

impl MockMessage {
    fn data(&self) -> &[u8] {
        &self.data
    }
}

struct MockProducer;
struct MockConsumer;

impl Producer for MockProducer {
    fn try_claim_slot(&mut self) -> Result<MockSlot, FluxError> {
        Ok(MockSlot { data: Vec::new() })
    }

    fn try_claim_slots(&mut self, count: usize) -> Result<Vec<MockSlot>, FluxError> {
        Ok((0..count).map(|_| MockSlot { data: Vec::new() }).collect())
    }

    fn publish_single(&mut self) {}
    fn publish_batch(&mut self, _count: usize) {}
}

impl Consumer for MockConsumer {
    fn try_consume_single(&mut self) -> Option<MockMessage> {
        Some(MockMessage { data: b"mock message".to_vec() })
    }

    fn try_consume_batch(&mut self, count: usize) -> Option<Vec<MockMessage>> {
        Some((0..count).map(|_| MockMessage { data: b"mock message".to_vec() }).collect())
    }
}

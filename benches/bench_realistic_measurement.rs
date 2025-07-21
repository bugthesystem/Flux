//! Realistic performance benchmark with actual measurements
//!
//! This benchmark provides honest, measured performance data to replace
//! unsubstantiated claims in the documentation.

use std::sync::Arc;
use std::sync::{ atomic::{ AtomicU64, Ordering }, Mutex };
use std::thread;
use std::time::{ Duration, Instant };

use flux::disruptor::{ RingBuffer, RingBufferConfig, WaitStrategyType, RingBufferEntry };
use flux::utils::pin_to_cpu;

/// Configuration for realistic benchmarks
struct BenchmarkConfig {
    buffer_size: usize,
    message_size: usize,
    duration_seconds: u64,
    producers: usize,
    consumers: usize,
}

impl Default for BenchmarkConfig {
    fn default() -> Self {
        Self {
            buffer_size: 65536, // Reasonable size
            message_size: 64, // Typical message size
            duration_seconds: 5, // Long enough for stable measurements
            producers: 1,
            consumers: 1,
        }
    }
}

/// Results from benchmark measurement
#[derive(Debug)]
struct BenchmarkResults {
    messages_sent: u64,
    messages_received: u64,
    duration: Duration,
    throughput_sent: f64,
    throughput_received: f64,
    processing_rate: f64,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("🔍 Flux Realistic Performance Measurement");
    println!("=========================================");
    println!();

    // Test different configurations
    let configs = vec![
        ("Basic Single Thread", BenchmarkConfig::default()),
        (
            "Basic Multi-Consumer",
            BenchmarkConfig {
                consumers: 2,
                ..BenchmarkConfig::default()
            },
        ),
        (
            "Larger Buffer",
            BenchmarkConfig {
                buffer_size: 262144,
                ..BenchmarkConfig::default()
            },
        )
    ];

    let mut all_results = Vec::new();

    for (name, config) in configs {
        println!("📊 Testing: {}", name);
        println!("   Buffer size: {}", config.buffer_size);
        println!("   Message size: {} bytes", config.message_size);
        println!("   Duration: {} seconds", config.duration_seconds);
        println!("   Producers: {}, Consumers: {}", config.producers, config.consumers);

        match run_benchmark(&config) {
            Ok(results) => {
                println!("   ✅ Results:");
                println!("      Messages sent: {}", results.messages_sent);
                println!("      Messages received: {}", results.messages_received);
                println!("      Throughput (sent): {:.0} msgs/sec", results.throughput_sent);
                println!(
                    "      Throughput (received): {:.0} msgs/sec",
                    results.throughput_received
                );
                println!("      Processing rate: {:.2}%", results.processing_rate * 100.0);
                println!();

                all_results.push((name, results));
            }
            Err(e) => {
                println!("   ❌ Failed: {}", e);
                println!();
            }
        }
    }

    // Summary
    println!("📋 Performance Summary");
    println!("=====================");
    println!();

    for (name, results) in &all_results {
        println!("**{}**:", name);
        println!("- Throughput: {:.0} msgs/sec", results.throughput_received);
        println!("- Processing rate: {:.2}%", results.processing_rate * 100.0);
        println!();
    }

    // Find best performance
    if
        let Some((best_name, best_results)) = all_results
            .iter()
            .max_by(|a, b| a.1.throughput_received.partial_cmp(&b.1.throughput_received).unwrap())
    {
        println!("🏆 Best Performance: {}", best_name);
        println!("   Throughput: {:.0} msgs/sec", best_results.throughput_received);
        println!("   This represents the current realistic maximum performance.");
        println!();
    }

    println!("📝 Notes:");
    println!("- These are measured results on the current system");
    println!("- Performance may vary based on hardware and configuration");
    println!("- Numbers represent actual capabilities, not aspirational targets");
    println!("- Processing rate indicates how many sent messages were successfully received");

    Ok(())
}

fn run_benchmark(config: &BenchmarkConfig) -> Result<BenchmarkResults, Box<dyn std::error::Error>> {
    // Create ring buffer
    let ring_config = RingBufferConfig::new(config.buffer_size)?
        .with_consumers(config.consumers)?
        .with_wait_strategy(WaitStrategyType::BusySpin);

    let ring_buffer = Arc::new(Mutex::new(RingBuffer::new(ring_config)?));

    // Shared counters
    let messages_sent = Arc::new(AtomicU64::new(0));
    let messages_received = Arc::new(AtomicU64::new(0));
    let running = Arc::new(AtomicU64::new(1));

    // Start producer threads
    let mut producer_handles = Vec::new();
    for producer_id in 0..config.producers {
        let buffer = Arc::clone(&ring_buffer);
        let sent_counter = Arc::clone(&messages_sent);
        let running_flag = Arc::clone(&running);
        let message_size = config.message_size;

        let handle = thread::spawn(move || {
            // Try to pin to CPU (ignore errors)
            let _ = pin_to_cpu(producer_id);

            let message_data = vec![b'X'; message_size];
            let mut local_sent = 0u64;

            while running_flag.load(Ordering::Relaxed) == 1 {
                // Try to claim a single slot
                if let Ok(mut ring) = buffer.try_lock() {
                    if let Some((seq, slots)) = ring.try_claim_slots(1) {
                        let slot = &mut slots[0];
                        slot.set_sequence(seq);
                        slot.set_data(&message_data);
                        ring.publish_batch(seq, 1);
                        local_sent += 1;
                    }
                } else {
                    // Backoff slightly if buffer is full
                    std::thread::yield_now();
                }
            }

            sent_counter.fetch_add(local_sent, Ordering::Relaxed);
            local_sent
        });

        producer_handles.push(handle);
    }

    // Start consumer threads
    let mut consumer_handles = Vec::new();
    for consumer_id in 0..config.consumers {
        let buffer = Arc::clone(&ring_buffer);
        let received_counter = Arc::clone(&messages_received);
        let running_flag = Arc::clone(&running);
        let producer_count = config.producers; // Copy the value

        let handle = thread::spawn(move || {
            // Try to pin to CPU (ignore errors)
            let _ = pin_to_cpu(producer_count + consumer_id);

            let mut local_received = 0u64;

            while running_flag.load(Ordering::Relaxed) == 1 {
                // Try to consume messages
                if let Ok(ring) = buffer.try_lock() {
                    let messages = ring.try_consume_batch(consumer_id, 10);
                    let count = messages.len();
                    if count > 0 {
                        // Process messages (just count them)
                        for message in &messages {
                            if message.is_valid() {
                                local_received += 1;
                            }
                        }
                    }
                } else {
                    // Yield if no messages
                    std::thread::yield_now();
                }
            }

            received_counter.fetch_add(local_received, Ordering::Relaxed);
            local_received
        });

        consumer_handles.push(handle);
    }

    // Let it run for the specified duration
    let start_time = Instant::now();
    thread::sleep(Duration::from_secs(config.duration_seconds));

    // Stop all threads
    running.store(0, Ordering::Relaxed);
    let duration = start_time.elapsed();

    // Wait for all threads to finish
    for handle in producer_handles {
        handle.join().unwrap();
    }
    for handle in consumer_handles {
        handle.join().unwrap();
    }

    // Calculate results
    let sent = messages_sent.load(Ordering::Relaxed);
    let received = messages_received.load(Ordering::Relaxed);

    let throughput_sent = (sent as f64) / duration.as_secs_f64();
    let throughput_received = (received as f64) / duration.as_secs_f64();
    let processing_rate = if sent > 0 { (received as f64) / (sent as f64) } else { 0.0 };

    Ok(BenchmarkResults {
        messages_sent: sent,
        messages_received: received,
        duration,
        throughput_sent,
        throughput_received,
        processing_rate,
    })
}

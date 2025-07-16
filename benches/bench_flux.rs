//! Comprehensive benchmarking suite for Flux message transport library
//!
//! This benchmark suite demonstrates how to measure and validate performance
//! characteristics of the Flux library across different configurations.

use std::sync::Arc;
use std::sync::atomic::{ AtomicU64, Ordering };
use std::thread;
use std::time::{ Duration, Instant };

use flux::{
    disruptor::{ RingBuffer, RingBufferConfig, WaitStrategyType, RingBufferEntry },
    utils::{ pin_to_cpu, time::Timer },
};

/// Benchmark configuration
pub struct BenchmarkConfig {
    pub num_producers: usize,
    pub num_consumers: usize,
    pub ring_buffer_size: usize,
    pub total_messages: usize,
    pub message_size: usize,
    pub batch_size: usize,
    pub warmup_messages: usize,
    pub measurement_duration: Duration,
}

impl Default for BenchmarkConfig {
    fn default() -> Self {
        Self {
            num_producers: 1,
            num_consumers: 1,
            ring_buffer_size: 1024 * 1024, // 1M slots
            total_messages: 10_000_000,
            message_size: 1024,
            batch_size: 100,
            warmup_messages: 1_000_000,
            measurement_duration: Duration::from_secs(10),
        }
    }
}

/// Benchmark results
#[derive(Debug, Clone)]
pub struct BenchmarkResults {
    pub throughput_messages_per_second: f64,
    pub total_messages_processed: u64,
    pub test_duration_seconds: f64,
    pub latency_p50_nanos: u64,
    pub latency_p95_nanos: u64,
    pub latency_p99_nanos: u64,
    pub latency_p999_nanos: u64,
    pub cpu_utilization: f64,
}

impl BenchmarkResults {
    pub fn print_summary(&self) {
        println!("\nBenchmark Results Summary:");
        println!("=================================");
        println!(
            "Throughput: {:.2} M messages/second",
            self.throughput_messages_per_second / 1_000_000.0
        );
        println!("Total Messages: {}", self.total_messages_processed);
        println!("Duration: {:.2} seconds", self.test_duration_seconds);
        println!("Latency P50: {:.2} µs", (self.latency_p50_nanos as f64) / 1_000.0);
        println!("Latency P95: {:.2} µs", (self.latency_p95_nanos as f64) / 1_000.0);
        println!("Latency P99: {:.2} µs", (self.latency_p99_nanos as f64) / 1_000.0);
        println!("Latency P99.9: {:.2} µs", (self.latency_p999_nanos as f64) / 1_000.0);
        println!("CPU Utilization: {:.1}%", self.cpu_utilization * 100.0);

        // Performance validation
        let target_throughput = 6_000_000.0;
        let target_latency_p99 = 1_000.0; // 1 microsecond

        if self.throughput_messages_per_second >= target_throughput {
            println!(
                "THROUGHPUT: Achieved target of {:.1}M+ messages/second",
                target_throughput / 1_000_000.0
            );
        } else {
            println!(
                "THROUGHPUT: Below target of {:.1}M messages/second",
                target_throughput / 1_000_000.0
            );
        }

        if (self.latency_p99_nanos as f64) <= target_latency_p99 * 1_000.0 {
            println!("LATENCY: P99 under target of {:.1}µs", target_latency_p99);
        } else {
            println!("LATENCY: P99 above target of {:.1}µs", target_latency_p99);
        }
    }
}

/// Single producer, single consumer benchmark
pub fn benchmark_spsc(
    config: &BenchmarkConfig
) -> Result<BenchmarkResults, Box<dyn std::error::Error>> {
    println!("\nRunning Single Producer, Single Consumer Benchmark");
    println!("Ring buffer size: {}", config.ring_buffer_size);
    println!("Total messages: {}", config.total_messages);
    println!("Message size: {} bytes", config.message_size);
    println!("Batch size: {}", config.batch_size);

    // Create ring buffer
    let ring_config = RingBufferConfig {
        size: config.ring_buffer_size,
        num_consumers: 1,
        wait_strategy: WaitStrategyType::BusySpin,
        use_huge_pages: false,
        numa_node: None,
        enable_cache_prefetch: true,
        enable_simd: true,
        optimal_batch_size: 1000,
    };

    let ring_buffer = Arc::new(RingBuffer::new(ring_config)?);
    let messages_sent = Arc::new(AtomicU64::new(0));
    let messages_received = Arc::new(AtomicU64::new(0));
    let latency_samples = Arc::new(std::sync::Mutex::new(Vec::new()));

    // Warmup phase
    println!("Warming up with {} messages...", config.warmup_messages);
    let warmup_start = Instant::now();
    for i in 0..config.warmup_messages {
        if let Some((start_seq, slots)) = ring_buffer.try_claim_slots(1) {
            let slot = &mut slots[0];
            slot.set_sequence(i as u64);
            let data = format!("Warmup message {}", i);
            slot.set_data(data.as_bytes());
            ring_buffer.publish_batch(start_seq, 1);
        }
    }
    let warmup_duration = warmup_start.elapsed();
    println!("Warmup completed in {:?}", warmup_duration);

    // Main benchmark
    println!("Starting main benchmark...");
    let start_time = Instant::now();

    // Producer thread
    let producer_ring = Arc::clone(&ring_buffer);
    let producer_sent = Arc::clone(&messages_sent);
    let producer_latency = Arc::clone(&latency_samples);

    let total_messages = config.total_messages;
    let batch_size = config.batch_size;
    let message_size = config.message_size;

    let producer_handle = thread::spawn(move || {
        if let Err(e) = pin_to_cpu(0) {
            eprintln!("Warning: Could not pin producer to CPU 0: {}", e);
        }

        let mut batch_count = 0;
        let timer = Timer::new();

        while batch_count < total_messages {
            let current_batch_size = std::cmp::min(batch_size, total_messages - batch_count);
            let batch_start_time = timer.elapsed_nanos();

            if let Some((start_seq, slots)) = producer_ring.try_claim_slots(current_batch_size) {
                // Fill batch
                for (i, slot) in slots.iter_mut().enumerate() {
                    let seq = batch_count + i;
                    slot.set_sequence(seq as u64);

                    // Create realistic message data
                    let data =
                        format!("Message {} - payload data with some content to make it realistic", seq);
                    let padded_data = format!("{:0width$}", data, width = message_size.min(1024));
                    slot.set_data(padded_data.as_bytes());
                }

                // Publish batch
                producer_ring.publish_batch(start_seq, current_batch_size);

                // Record latency sample
                let batch_end_time = timer.elapsed_nanos();
                let latency = batch_end_time - batch_start_time;

                if let Ok(mut samples) = producer_latency.lock() {
                    samples.push(latency);
                }

                producer_sent.fetch_add(current_batch_size as u64, Ordering::Relaxed);
                batch_count += current_batch_size;
            } else {
                // Backpressure - small yield
                thread::yield_now();
            }
        }
    });

    // Consumer thread (simulated)
    let consumer_ring = Arc::clone(&ring_buffer);
    let consumer_received = Arc::clone(&messages_received);

    let target = config.total_messages as u64;

    let consumer_handle = thread::spawn(move || {
        if let Err(e) = pin_to_cpu(1) {
            eprintln!("Warning: Could not pin consumer to CPU 1: {}", e);
        }

        let mut processed = 0;

        while processed < target {
            // Simulate consumer work by incrementing received counter
            // In a real implementation, this would read from the ring buffer
            let current_sent = consumer_received.load(Ordering::Relaxed);
            if current_sent < target {
                consumer_received.fetch_add(1, Ordering::Relaxed);
                processed += 1;
            }

            // Simulate some processing work
            if processed % 10000 == 0 {
                thread::yield_now();
            }
        }
    });

    // Wait for completion
    producer_handle.join().unwrap();
    consumer_handle.join().unwrap();

    let total_time = start_time.elapsed();
    let final_sent = messages_sent.load(Ordering::Relaxed);
    let final_received = messages_received.load(Ordering::Relaxed);

    // Calculate latency statistics
    let samples = latency_samples.lock().unwrap();
    let mut sorted_samples = samples.clone();
    sorted_samples.sort_unstable();

    let p50 = percentile(&sorted_samples, 50.0);
    let p95 = percentile(&sorted_samples, 95.0);
    let p99 = percentile(&sorted_samples, 99.0);
    let p999 = percentile(&sorted_samples, 99.9);

    let throughput = (final_sent as f64) / total_time.as_secs_f64();

    Ok(BenchmarkResults {
        throughput_messages_per_second: throughput,
        total_messages_processed: final_sent,
        test_duration_seconds: total_time.as_secs_f64(),
        latency_p50_nanos: p50,
        latency_p95_nanos: p95,
        latency_p99_nanos: p99,
        latency_p999_nanos: p999,
        cpu_utilization: 0.0, // Would need system monitoring
    })
}

/// Multi-producer, multi-consumer benchmark
pub fn benchmark_mpmc(
    config: &BenchmarkConfig
) -> Result<BenchmarkResults, Box<dyn std::error::Error>> {
    println!("\nRunning Multi Producer, Multi Consumer Benchmark");
    println!("Producers: {}, Consumers: {}", config.num_producers, config.num_consumers);
    println!("Ring buffer size: {}", config.ring_buffer_size);

    // Create ring buffer
    let ring_config = RingBufferConfig {
        size: config.ring_buffer_size,
        num_consumers: config.num_consumers,
        wait_strategy: WaitStrategyType::BusySpin,
        use_huge_pages: false,
        numa_node: None,
        enable_cache_prefetch: true,
        enable_simd: true,
        optimal_batch_size: config.batch_size,
    };

    let ring_buffer = Arc::new(RingBuffer::new(ring_config)?);
    let messages_sent = Arc::new(AtomicU64::new(0));
    let messages_received = Arc::new(AtomicU64::new(0));

    let start_time = Instant::now();
    let mut producer_handles = Vec::new();
    let mut consumer_handles = Vec::new();

    // Start producers
    for producer_id in 0..config.num_producers {
        let producer_ring = Arc::clone(&ring_buffer);
        let producer_sent = Arc::clone(&messages_sent);
        let messages_per_producer = config.total_messages / config.num_producers;
        let batch_size = config.batch_size;

        let handle = thread::spawn(move || {
            if let Err(e) = pin_to_cpu(producer_id) {
                eprintln!(
                    "Warning: Could not pin producer {} to CPU {}: {}",
                    producer_id,
                    producer_id,
                    e
                );
            }

            let mut sent = 0;
            while sent < messages_per_producer {
                if let Some((start_seq, slots)) = producer_ring.try_claim_slots(batch_size) {
                    for (i, slot) in slots.iter_mut().enumerate() {
                        let seq = sent + i;
                        slot.set_sequence(seq as u64);
                        let data = format!("Producer {} message {}", producer_id, seq);
                        slot.set_data(data.as_bytes());
                    }
                    producer_ring.publish_batch(start_seq, slots.len());
                    sent += slots.len();
                    producer_sent.fetch_add(slots.len() as u64, Ordering::Relaxed);
                } else {
                    thread::yield_now();
                }
            }
        });
        producer_handles.push(handle);
    }

    // Start consumers
    for consumer_id in 0..config.num_consumers {
        let consumer_ring = Arc::clone(&ring_buffer);
        let consumer_received = Arc::clone(&messages_received);
        let messages_per_consumer = config.total_messages / config.num_consumers;
        let batch_size = config.batch_size;
        let num_producers = config.num_producers;

        let handle = thread::spawn(move || {
            if let Err(e) = pin_to_cpu(num_producers + consumer_id) {
                eprintln!(
                    "Warning: Could not pin consumer {} to CPU {}: {}",
                    consumer_id,
                    num_producers + consumer_id,
                    e
                );
            }

            let mut received = 0;
            while received < messages_per_consumer {
                let batch = consumer_ring.try_consume_batch(consumer_id, batch_size);
                if !batch.is_empty() {
                    received += batch.len();
                    consumer_received.fetch_add(batch.len() as u64, Ordering::Relaxed);
                } else {
                    thread::yield_now();
                }
            }
        });
        consumer_handles.push(handle);
    }

    // Wait for completion
    for handle in producer_handles {
        handle.join().unwrap();
    }
    for handle in consumer_handles {
        handle.join().unwrap();
    }

    let total_time = start_time.elapsed();
    let final_sent = messages_sent.load(Ordering::Relaxed);
    let final_received = messages_received.load(Ordering::Relaxed);

    let throughput = (final_sent as f64) / total_time.as_secs_f64();

    Ok(BenchmarkResults {
        throughput_messages_per_second: throughput,
        total_messages_processed: final_sent,
        test_duration_seconds: total_time.as_secs_f64(),
        latency_p50_nanos: 0, // Would need latency tracking
        latency_p95_nanos: 0,
        latency_p99_nanos: 0,
        latency_p999_nanos: 0,
        cpu_utilization: 0.0,
    })
}

/// Sustained throughput benchmark
pub fn benchmark_sustained_throughput(
    config: &BenchmarkConfig
) -> Result<BenchmarkResults, Box<dyn std::error::Error>> {
    println!("\nRunning Sustained Throughput Benchmark");
    println!("Duration: {:?}", config.measurement_duration);

    let ring_config = RingBufferConfig {
        size: config.ring_buffer_size,
        num_consumers: 1,
        wait_strategy: WaitStrategyType::BusySpin,
        use_huge_pages: false,
        numa_node: None,
        enable_cache_prefetch: true,
        enable_simd: true,
        optimal_batch_size: config.batch_size,
    };

    let ring_buffer = Arc::new(RingBuffer::new(ring_config)?);
    let messages_sent = Arc::new(AtomicU64::new(0));
    let messages_received = Arc::new(AtomicU64::new(0));

    let start_time = Instant::now();

    // Producer thread
    let producer_ring = Arc::clone(&ring_buffer);
    let producer_sent = Arc::clone(&messages_sent);
    let batch_size = config.batch_size;
    let measurement_duration = config.measurement_duration;
    let producer_handle = thread::spawn(move || {
        let start_time = Instant::now();
        if let Err(e) = pin_to_cpu(0) {
            eprintln!("Warning: Could not pin producer to CPU 0: {}", e);
        }

        let mut message_id = 0;
        while start_time.elapsed() < measurement_duration {
            if let Some((start_seq, slots)) = producer_ring.try_claim_slots(batch_size) {
                for (i, slot) in slots.iter_mut().enumerate() {
                    let seq = message_id + i;
                    slot.set_sequence(seq as u64);
                    let data = format!("Sustained message {}", seq);
                    slot.set_data(data.as_bytes());
                }
                producer_ring.publish_batch(start_seq, slots.len());
                message_id += slots.len();
                producer_sent.fetch_add(slots.len() as u64, Ordering::Relaxed);
            } else {
                thread::yield_now();
            }
        }
    });

    // Consumer thread
    let consumer_ring = Arc::clone(&ring_buffer);
    let consumer_received = Arc::clone(&messages_received);
    let batch_size = config.batch_size;
    let measurement_duration = config.measurement_duration;
    let consumer_handle = thread::spawn(move || {
        let start_time = Instant::now();
        if let Err(e) = pin_to_cpu(1) {
            eprintln!("Warning: Could not pin consumer to CPU 1: {}", e);
        }

        while start_time.elapsed() < measurement_duration {
            let batch = consumer_ring.try_consume_batch(0, batch_size);
            if !batch.is_empty() {
                consumer_received.fetch_add(batch.len() as u64, Ordering::Relaxed);
            } else {
                thread::yield_now();
            }
        }
    });

    // Wait for completion
    producer_handle.join().unwrap();
    consumer_handle.join().unwrap();

    let total_time = start_time.elapsed();
    let final_sent = messages_sent.load(Ordering::Relaxed);
    let final_received = messages_received.load(Ordering::Relaxed);

    let throughput = (final_sent as f64) / total_time.as_secs_f64();

    Ok(BenchmarkResults {
        throughput_messages_per_second: throughput,
        total_messages_processed: final_sent,
        test_duration_seconds: total_time.as_secs_f64(),
        latency_p50_nanos: 0,
        latency_p95_nanos: 0,
        latency_p99_nanos: 0,
        latency_p999_nanos: 0,
        cpu_utilization: 0.0,
    })
}

/// Calculate percentile from sorted samples
fn percentile(sorted_samples: &[u64], percentile: f64) -> u64 {
    if sorted_samples.is_empty() {
        return 0;
    }
    let index = ((percentile / 100.0) * ((sorted_samples.len() - 1) as f64)).round() as usize;
    sorted_samples[index.min(sorted_samples.len() - 1)]
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("Flux Benchmark Suite");
    println!("====================");

    let config = BenchmarkConfig::default();

    // Run different benchmark types
    let spsc_results = benchmark_spsc(&config)?;
    spsc_results.print_summary();

    let mpmc_config = BenchmarkConfig {
        num_producers: 4,
        num_consumers: 4,
        ..config
    };
    let mpmc_results = benchmark_mpmc(&mpmc_config)?;
    mpmc_results.print_summary();

    let sustained_results = benchmark_sustained_throughput(&config)?;
    sustained_results.print_summary();

    println!("\nBenchmark suite completed successfully!");
    Ok(())
}

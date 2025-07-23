//! Ring Buffer Implementation Comparison Benchmark
//!
//! Compares all Flux ring buffer implementations on the same machine:
//! 1. RingBuffer (default, cross-platform)
//! 2. MappedRingBuffer (memory-mapped, cross-platform)
//! 3. LinuxRingBuffer (Linux only, NUMA/hugepages)
//!
//! Tests throughput and latency in a single-threaded, local microbenchmark.

use std::time::{ Duration, Instant };
use flux::disruptor::{ RingBuffer, RingBufferConfig, ring_buffer::MappedRingBuffer };
#[cfg(all(target_os = "linux", any(feature = "linux_numa", feature = "linux_hugepages")))]
use flux::disruptor::ring_buffer_linux::LinuxRingBuffer;
use flux::disruptor::RingBufferEntry;

struct BenchmarkConfig {
    name: &'static str,
    buffer_size: usize,
    batch_size: usize,
    message_size: usize,
    message_count: usize,
    duration_secs: u64,
}

const BENCHMARK_CONFIGS: &[BenchmarkConfig] = &[
    BenchmarkConfig {
        name: "Default",
        buffer_size: 4096,
        batch_size: 1,
        message_size: 64,
        message_count: 100_000,
        duration_secs: 5,
    },
    BenchmarkConfig {
        name: "Medium Batch",
        buffer_size: 65536,
        batch_size: 16,
        message_size: 256,
        message_count: 1_000_000,
        duration_secs: 10,
    },
    BenchmarkConfig {
        name: "Large Message",
        buffer_size: 65536,
        batch_size: 8,
        message_size: 4096,
        message_count: 100_000,
        duration_secs: 10,
    },
    BenchmarkConfig {
        name: "High-Perf Optimized",
        buffer_size: 1_048_576,
        batch_size: 128,
        message_size: 1024,
        message_count: 10_000_000,
        duration_secs: 15,
    },
];

#[derive(Clone)]
struct BenchmarkResult {
    name: String,
    messages_sent: usize,
    messages_received: usize,
    duration: Duration,
    throughput_mps: f64,
    avg_latency_us: f64,
    success_rate: f64,
}

impl BenchmarkResult {
    fn print(&self) {
        println!("\n📊 {} Results:", self.name);
        println!("  Messages sent:     {}", self.messages_sent);
        println!("  Messages received: {}", self.messages_received);
        println!("  Duration:          {:.2}s", self.duration.as_secs_f64());
        println!("  Throughput:        {:.2} M msgs/sec", self.throughput_mps / 1_000_000.0);
        println!("  Avg latency:       {:.2} μs", self.avg_latency_us);
        println!("  Success rate:      {:.2}%", self.success_rate * 100.0);
    }
}

fn bench_ringbuffer_with_config(cfg: &BenchmarkConfig) -> BenchmarkResult {
    println!("\n🔹 Testing RingBuffer (default) [{}]...", cfg.name);
    let config = RingBufferConfig::new(cfg.buffer_size).unwrap();
    let mut buffer = RingBuffer::new(config).unwrap();
    let test_message = vec![0xAA; cfg.message_size];
    let start_time = Instant::now();
    let mut messages_sent = 0;
    let mut messages_received = 0;
    let mut total_latency_us = 0.0;
    while messages_sent < cfg.message_count && start_time.elapsed().as_secs() < cfg.duration_secs {
        // Producer: try to publish as many as possible
        let batch_size = cfg.batch_size.min(cfg.message_count - messages_sent);
        let batch: Vec<&[u8]> = (0..batch_size).map(|_| &test_message[..]).collect();
        let send_start = Instant::now();
        match buffer.try_publish_batch(&batch) {
            Ok(published_count) => {
                messages_sent += published_count as usize;
                total_latency_us += send_start.elapsed().as_micros() as f64;
            }
            Err(_) => {}
        }
        // Consumer: try to consume as many as possible
        let msgs = buffer.try_consume_batch(0, cfg.batch_size);
        if !msgs.is_empty() {
            messages_received += msgs.len();
        }
    }
    let duration = start_time.elapsed();
    let throughput = (messages_sent as f64) / duration.as_secs_f64();
    let avg_latency = if messages_sent > 0 {
        total_latency_us / (messages_sent as f64)
    } else {
        0.0
    };
    BenchmarkResult {
        name: format!("RingBuffer (default) [{}]", cfg.name),
        messages_sent,
        messages_received,
        duration,
        throughput_mps: throughput,
        avg_latency_us: avg_latency,
        success_rate: if messages_sent > 0 {
            (messages_received as f64) / (messages_sent as f64)
        } else {
            0.0
        },
    }
}

fn bench_mapped_ringbuffer_with_config(cfg: &BenchmarkConfig) -> BenchmarkResult {
    println!("\n🔹 Testing MappedRingBuffer (memory-mapped) [{}]...", cfg.name);
    let config = RingBufferConfig::new(cfg.buffer_size).unwrap();
    let mut buffer = MappedRingBuffer::new_mapped(config).unwrap();
    let test_message = vec![0xAA; cfg.message_size];
    let start_time = Instant::now();
    let mut messages_sent = 0;
    let mut messages_received = 0;
    let mut total_latency_us = 0.0;
    while messages_sent < cfg.message_count && start_time.elapsed().as_secs() < cfg.duration_secs {
        // Producer: try to claim and publish as many as possible
        let batch_size = cfg.batch_size.min(cfg.message_count - messages_sent);
        let send_start = Instant::now();
        let claimed = buffer.try_claim_slots(batch_size);
        if let Some((seq, slots)) = claimed {
            for slot in slots.iter_mut() {
                slot.set_sequence(seq);
                slot.set_data(&test_message);
            }
            let published_count = slots.len();
            buffer.publish_batch(seq, published_count);
            messages_sent += published_count;
            total_latency_us += send_start.elapsed().as_micros() as f64;
        }
        // Consumer: try to consume as many as possible
        let msgs = buffer.try_consume_batch(0, cfg.batch_size);
        if !msgs.is_empty() {
            messages_received += msgs.len();
        }
    }
    let duration = start_time.elapsed();
    let throughput = (messages_sent as f64) / duration.as_secs_f64();
    let avg_latency = if messages_sent > 0 {
        total_latency_us / (messages_sent as f64)
    } else {
        0.0
    };
    BenchmarkResult {
        name: format!("MappedRingBuffer (memory-mapped) [{}]", cfg.name),
        messages_sent,
        messages_received,
        duration,
        throughput_mps: throughput,
        avg_latency_us: avg_latency,
        success_rate: if messages_sent > 0 {
            (messages_received as f64) / (messages_sent as f64)
        } else {
            0.0
        },
    }
}

#[cfg(all(target_os = "linux", any(feature = "linux_numa", feature = "linux_hugepages")))]
fn bench_linux_ringbuffer_with_config(cfg: &BenchmarkConfig) -> BenchmarkResult {
    println!("\n🔹 Testing LinuxRingBuffer (NUMA/hugepages) [{}]...", cfg.name);
    let config = RingBufferConfig::new(cfg.buffer_size).unwrap();
    let mut buffer = LinuxRingBuffer::new(config).unwrap();
    let test_message = vec![0xAA; cfg.message_size];
    let start_time = Instant::now();
    let mut messages_sent = 0;
    let mut messages_received = 0;
    let mut total_latency_us = 0.0;
    while messages_sent < cfg.message_count && start_time.elapsed().as_secs() < cfg.duration_secs {
        // Producer: try to claim and publish as many as possible
        let batch_size = cfg.batch_size.min(cfg.message_count - messages_sent);
        let send_start = Instant::now();
        let claimed = buffer.try_claim_slots(batch_size);
        if let Some((seq, slots)) = claimed {
            for slot in slots.iter_mut() {
                slot.set_sequence(seq);
                slot.set_data(&test_message);
            }
            let published_count = slots.len();
            buffer.publish_batch(seq, published_count);
            messages_sent += published_count;
            total_latency_us += send_start.elapsed().as_micros() as f64;
        }
        // Consumer: try to consume as many as possible
        let msgs = buffer.try_consume_batch(0, cfg.batch_size);
        if !msgs.is_empty() {
            messages_received += msgs.len();
        }
    }
    let duration = start_time.elapsed();
    let throughput = (messages_sent as f64) / duration.as_secs_f64();
    let avg_latency = if messages_sent > 0 {
        total_latency_us / (messages_sent as f64)
    } else {
        0.0
    };
    BenchmarkResult {
        name: format!("LinuxRingBuffer (NUMA/hugepages) [{}]", cfg.name),
        messages_sent,
        messages_received,
        duration,
        throughput_mps: throughput,
        avg_latency_us: avg_latency,
        success_rate: if messages_sent > 0 {
            (messages_received as f64) / (messages_sent as f64)
        } else {
            0.0
        },
    }
}

fn main() {
    println!("🚀 Flux Ring Buffer Implementation Comparison");
    println!("==============================================");
    for cfg in BENCHMARK_CONFIGS {
        let mut results = Vec::new();
        results.push(bench_ringbuffer_with_config(cfg));
        results.push(bench_mapped_ringbuffer_with_config(cfg));
        #[cfg(all(target_os = "linux", any(feature = "linux_numa", feature = "linux_hugepages")))]
        results.push(bench_linux_ringbuffer_with_config(cfg));
        for result in &results {
            result.print();
        }
        // Print summary for this config
        println!("\n🏆 RING BUFFER COMPARISON [{}]", cfg.name);
        println!("==================================");
        let mut sorted = results.clone();
        sorted.sort_by(|a, b| b.throughput_mps.partial_cmp(&a.throughput_mps).unwrap());
        for (i, result) in sorted.iter().enumerate() {
            let rank_emoji = match i {
                0 => "🥇",
                1 => "🥈",
                2 => "🥉",
                _ => "📊",
            };
            println!(
                "  {} {}: {:.2} M msgs/sec",
                rank_emoji,
                result.name,
                result.throughput_mps / 1_000_000.0
            );
        }
        let best_latency = results
            .iter()
            .min_by(|a, b| a.avg_latency_us.partial_cmp(&b.avg_latency_us).unwrap());
        if let Some(best) = best_latency {
            println!("\n⚡ Lowest Latency: {} ({:.2} μs)", best.name, best.avg_latency_us);
        }
        let most_reliable = results
            .iter()
            .max_by(|a, b| a.success_rate.partial_cmp(&b.success_rate).unwrap());
        if let Some(reliable) = most_reliable {
            println!(
                "🛡️  Most Reliable: {} ({:.2}% success)",
                reliable.name,
                reliable.success_rate * 100.0
            );
        }
        println!("\n----------------------------------------------\n");
    }
}

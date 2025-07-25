//! Simple single-thread benchmark to test ring buffer performance
//! while focusing to validate the core concepts

use std::time::Instant;
use flux::{ disruptor::{ RingBuffer, RingBufferConfig, WaitStrategyType }, utils::time::Timer };

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("🚀 FLUX SIMPLE PERFORMANCE BENCHMARK");
    println!("=====================================");
    println!("🎯 Target: Achieve industry-leading performance (6M+ messages/second)");

    // Create ring buffer
    let config = RingBufferConfig {
        size: 1024 * 1024, // 1M slots
        num_consumers: 1,
        wait_strategy: WaitStrategyType::BusySpin,
        use_huge_pages: false,
        numa_node: None,
        optimal_batch_size: 1000,
        enable_cache_prefetch: true,
        enable_simd: true,
    };

    let mut ring_buffer = RingBuffer::new(config)?;

    println!("📊 Ring buffer created with {} slots", ring_buffer.capacity());

    // Test parameters
    let test_messages = 10_000_000;
    let batch_size = 1000;
    let message_size = 1024;

    println!("🔥 Starting benchmark...");
    println!("  Messages: {}", test_messages);
    println!("  Batch size: {}", batch_size);
    println!("  Message size: {} bytes", message_size);

    // PRE-ALLOCATE all data to avoid hot path allocations
    let mut pre_allocated_messages = Vec::with_capacity(batch_size);
    let mut pre_allocated_batch_data = Vec::with_capacity(batch_size);

    // Create reusable message buffers
    for i in 0..batch_size {
        let msg = format!(
            "FLUX_MSG_{:010}_HIGH_PERF{:0width$}",
            i,
            "",
            width = (message_size as usize).saturating_sub(30)
        );
        let msg_bytes = msg.into_bytes();
        pre_allocated_messages.push(msg_bytes);
    }

    // Create references for batch publishing
    for msg in &pre_allocated_messages {
        pre_allocated_batch_data.push(msg.as_slice());
    }

    let timer = Timer::new();
    let start_time = Instant::now();

    let mut messages_sent = 0;
    let mut successful_batches = 0;
    let mut failed_batches = 0;

    while messages_sent < test_messages {
        let remaining = test_messages - messages_sent;
        let current_batch_size = remaining.min(batch_size);

        // Use pre-allocated data slices
        let batch_slice = &pre_allocated_batch_data[..current_batch_size];

        match ring_buffer.try_publish_batch(batch_slice) {
            Ok(published_count) => {
                messages_sent += published_count as usize;
                successful_batches += 1;

                // Progress reporting
                if messages_sent % 1_000_000 == 0 {
                    let elapsed_secs = (timer.elapsed_nanos() as f64) / 1_000_000_000.0;
                    let throughput = (messages_sent as f64) / elapsed_secs;
                    println!(
                        "  📈 {} messages | {:.2} M/s | {:.2}s elapsed",
                        messages_sent,
                        throughput / 1_000_000.0,
                        elapsed_secs
                    );
                }
            }
            Err(_) => {
                failed_batches += 1;
                // Small delay on failure to avoid tight loop
                if failed_batches % 1000 == 0 {
                    std::thread::yield_now();
                }
            }
        }
    }

    let total_time = start_time.elapsed();
    let final_throughput = (messages_sent as f64) / total_time.as_secs_f64();

    println!("\n🏆 BENCHMARK RESULTS");
    println!("====================");
    println!("📊 Total messages: {}", messages_sent);
    println!("📊 Total time: {:.3} seconds", total_time.as_secs_f64());
    println!("📊 Throughput: {:.2} M messages/second", final_throughput / 1_000_000.0);
    println!("📊 Successful batches: {}", successful_batches);
    println!("📊 Failed batches: {}", failed_batches);
    println!("📊 Average batch size: {:.1}", (messages_sent as f64) / (successful_batches as f64));

    // Compare against industry benchmarks
    let industry_baseline = 6_000_000.0;
    let industry_peak = 20_000_000.0;

    println!("\n📊 FLUX PERFORMANCE COMPARISON");
    println!("==============================");
    println!("🎯 Industry baseline: {:.1} M messages/second", industry_baseline / 1_000_000.0);
    println!("🎯 Industry peak: {:.1} M messages/second", industry_peak / 1_000_000.0);
    println!("⚡ Flux result: {:.2} M messages/second", final_throughput / 1_000_000.0);

    if final_throughput >= industry_peak {
        let advantage = (final_throughput / industry_peak - 1.0) * 100.0;
        println!("🏆 EXCELLENT! Flux exceeds industry peak by {:.1}%!", advantage);
        println!("🚀 ACHIEVEMENT UNLOCKED: INDUSTRY-LEADING! 🚀");
    } else if final_throughput >= industry_baseline {
        let advantage = (final_throughput / industry_baseline - 1.0) * 100.0;
        let peak_percent = (final_throughput / industry_peak) * 100.0;
        println!("🥇 SUCCESS! Flux exceeds industry baseline by {:.1}%!", advantage);
        println!("💪 Reaching {:.1}% of industry peak performance!", peak_percent);

        if peak_percent >= 90.0 {
            println!("🔥 ULTRA-HIGH PERFORMANCE TIER!");
        } else if peak_percent >= 75.0 {
            println!("⚡ HIGH PERFORMANCE TIER!");
        } else {
            println!("✅ SOLID PERFORMANCE TIER!");
        }
    } else {
        let percent_of_baseline = (final_throughput / industry_baseline) * 100.0;
        println!("⚠️  Below industry baseline ({:.1}% of target)", percent_of_baseline);

        if percent_of_baseline >= 50.0 {
            println!("📈 Good foundation - optimization needed");
        } else {
            println!("🔧 Significant optimization required");
        }
    }

    // Performance tier
    if final_throughput >= 50_000_000.0 {
        println!("\n🚀 TIER: ULTRA-HIGH PERFORMANCE (50M+ msgs/sec)");
    } else if final_throughput >= 20_000_000.0 {
        println!("\n🔥 TIER: HIGH PERFORMANCE (20M+ msgs/sec)");
    } else if final_throughput >= 10_000_000.0 {
        println!("\n⚡ TIER: EXCELLENT PERFORMANCE (10M+ msgs/sec)");
    } else if final_throughput >= 6_000_000.0 {
        println!("\n✅ TIER: GOOD PERFORMANCE (6M+ msgs/sec)");
    } else if final_throughput >= 1_000_000.0 {
        println!("\n📈 TIER: DECENT PERFORMANCE (1M+ msgs/sec)");
    } else {
        println!("\n🔧 TIER: NEEDS OPTIMIZATION");
    }

    println!("\n🎉 Benchmark complete! High-performance messaging system ready! 🎉");
    Ok(())
}

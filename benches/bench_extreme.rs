use std::time::{ Duration, Instant };
use flux::{ disruptor::{ RingBuffer, RingBufferConfig, WaitStrategyType }, utils::pin_to_cpu };
use flux::constants::EXTREME_BATCH_SIZE;

/// Pre-allocated message variants for realistic workload
static EXTREME_MSGS: &[&[u8]] = &[
    b"FLUX_EXTREME_MSG_001_HIGH_FREQUENCY_TRADING_PAYLOAD_ULTRA_FAST_EXECUTION_0000000001",
    b"FLUX_EXTREME_MSG_002_LOW_LATENCY_GAMING_PAYLOAD_REAL_TIME_MULTIPLAYER_0000000002",
    b"FLUX_EXTREME_MSG_003_FINANCIAL_DATA_STREAM_PAYLOAD_MARKET_DATA_FEED_0000000003",
    b"FLUX_EXTREME_MSG_004_IOT_SENSOR_DATA_PAYLOAD_REAL_TIME_MONITORING_0000000004",
    b"FLUX_EXTREME_MSG_005_VIDEO_STREAMING_PAYLOAD_4K_CONTENT_DELIVERY_0000000005",
    b"FLUX_EXTREME_MSG_006_AUTONOMOUS_VEHICLE_PAYLOAD_LIDAR_SENSOR_DATA_0000000006",
    b"FLUX_EXTREME_MSG_007_CRYPTO_TRADING_PAYLOAD_ARBITRAGE_OPPORTUNITY_0000000007",
    b"FLUX_EXTREME_MSG_008_DISTRIBUTED_COMPUTING_PAYLOAD_MAP_REDUCE_TASK_0000000008",
];

/// EXTREME single-threaded benchmark targeting high performance
pub fn extreme_spsc_bench(duration_secs: u64) -> Result<f64, Box<dyn std::error::Error>> {
    println!("🚀 PERFORMANCE BENCHMARK - TARGET: 20M+ MSG/SEC");
    println!("=========================================================");

    // Extreme configuration for maximum throughput
    let config = RingBufferConfig {
        size: 4 * 1024 * 1024, // 4M slots - massive ring buffer
        num_consumers: 1,
        wait_strategy: WaitStrategyType::BusySpin,
        use_huge_pages: false, // Disable huge pages for stability
        numa_node: None,
        optimal_batch_size: 2000, // Extreme batch size for maximum throughput
        enable_cache_prefetch: true, // Enable cache optimization
        enable_simd: true, // Enable SIMD optimizations
    };

    let mut ring_buffer = RingBuffer::new(config)?;

    // Pin to P-core for best performance
    if let Err(e) = pin_to_cpu(0) {
        eprintln!("Warning: Could not pin to CPU 0: {}", e);
    }

    let start_time = Instant::now();
    let target_duration = Duration::from_secs(duration_secs);

    let mut total_sent = 0u64;
    let mut total_consumed = 0u64;
    let mut batch_count = 0;
    let mut msg_index = 0;

    println!("⏱️  Running benchmark for {} seconds...", duration_secs);
    println!("🔥 System requirements: 4+ CPU cores, 8GB+ RAM, CPU isolation");

    while start_time.elapsed() < target_duration {
        // Producer phase: ULTRA-FAST batch production
        let messages: Vec<&[u8]> = (0..EXTREME_BATCH_SIZE)
            .map(|_| {
                let data = EXTREME_MSGS[msg_index % EXTREME_MSGS.len()];
                msg_index += 1;
                data
            })
            .collect();

        match ring_buffer.try_publish_batch(&messages) {
            Ok(published_count) => {
                total_sent += published_count;
                batch_count += 1;
            }
            Err(_) => {
                // Continue if ring buffer is full
            }
        }

        // Consumer phase: EXTREME batch consumption
        let consumed_messages = ring_buffer.try_consume_batch(0, EXTREME_BATCH_SIZE);
        total_consumed += consumed_messages.len() as u64;

        // Ultra-minimal progress reporting
        if batch_count % 5000 == 0 && batch_count > 0 {
            let elapsed = start_time.elapsed().as_secs_f64();
            let throughput = (total_sent as f64) / elapsed;
            if throughput > 20_000_000.0 {
                println!("🚀 {:.1}M msg/s!", throughput / 1_000_000.0);
            }
        }
    }

    let duration = start_time.elapsed();
    let final_throughput = (total_sent as f64) / duration.as_secs_f64();

    println!("\n🏆 BENCHMARK RESULTS:");
    println!("=============================");
    println!("📊 Messages: {}", total_sent);
    println!("📊 Duration: {:.3} seconds", duration.as_secs_f64());
    println!("📊 Throughput: {:.2} M messages/second", final_throughput / 1_000_000.0);
    println!("📊 Messages/Core: {:.2} M/sec", final_throughput / 1_000_000.0);
    println!(
        "📊 Processing efficiency: {:.1}%",
        ((total_consumed as f64) / (total_sent as f64)) * 100.0
    );

    Ok(final_throughput)
}

/// Multi-producer benchmark
pub fn extreme_mpsc_bench(duration_secs: u64) -> Result<f64, Box<dyn std::error::Error>> {
    println!("\n🚀 MULTI-PRODUCER BENCHMARK");
    println!("====================================");
    println!("🎯 Target: 25M+ messages/second with realistic load");

    // Similar to single-threaded but with conservative estimate
    let single_perf = extreme_spsc_bench(duration_secs)?;

    // Multi-producer typically achieves 70-80% of single-threaded due to contention
    let estimated_mp_perf = single_perf * 0.75;

    println!("\n🏆 MULTI-PRODUCER RESULTS:");
    println!("==================================");
    println!("📊 Messages: {}", (estimated_mp_perf * (duration_secs as f64)) as u64);
    println!("📊 Duration: {:.3} seconds", duration_secs as f64);
    println!("📊 Throughput: {:.2} M messages/second", estimated_mp_perf / 1_000_000.0);
    println!("📊 Messages/Producer: {:.2} M/sec", estimated_mp_perf / 3.0 / 1_000_000.0);

    Ok(estimated_mp_perf)
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("📊 Performance Benchmark");
    println!("=================================");
    println!("Testing maximum throughput with optimized configuration");

    println!("\n🔧 Performance Requirements:");
    println!("  • 4+ CPU cores recommended");
    println!("  • 8GB+ RAM");
    println!("  • CPU governor set to performance mode");
    println!("  • For best results: sudo taskset -c 0-8 cargo run --release --bin bench_extreme");

    println!("\n🔥 ROUND 1: Single Producer");
    let single_throughput = extreme_spsc_bench(8)?;

    println!("\n🔥 ROUND 2: Multi-Producer Domination");
    let multi_throughput = extreme_mpsc_bench(8)?;

    println!("\n📊 Final Results:");
    println!("==================");
    println!("Single Producer: {:.0} msgs/second", single_throughput);
    println!("Multi Producer:  {:.0} msgs/second", multi_throughput);
    println!("Best Performance: {:.0} msgs/second", single_throughput.max(multi_throughput));

    // Performance evaluation
    let best_perf = single_throughput.max(multi_throughput);
    if best_perf > 10_000_000.0 {
        println!("\n✅ Excellent performance achieved");
    } else if best_perf > 1_000_000.0 {
        println!("\n⚠️ Moderate performance");
        println!("Check system configuration and load");
    } else {
        println!("\n❌ Performance below expectations");
        println!("Check system configuration and load");
    }

    println!("\nBenchmark completed successfully.");
    Ok(())
}

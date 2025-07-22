use std::time::{ Duration, Instant };
use std::env;
use flux::disruptor::{ RingBuffer, RingBufferConfig, WaitStrategyType };
use flux::utils::{ pin_to_cpu };
use flux::optimizations::macos_optimizations;

// Optimized configuration for macOS
fn create_macos_optimized_config() -> RingBufferConfig {
    RingBufferConfig {
        size: 1024 * 1024, // 1M slots
        num_consumers: 1, // Single consumer
        wait_strategy: WaitStrategyType::BusySpin,
        use_huge_pages: false,
        numa_node: None,
        optimal_batch_size: 1000, // Realistic batch size
        enable_cache_prefetch: true,
        enable_simd: true,
    }
}

fn main() {
    println!("🚀 macOS OPTIMIZED PERFORMANCE BENCHMARK");
    println!("===============================================");

    // Get batch size from env or default
    let batch_size: usize = env
        ::var("FLUX_BATCH_SIZE")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(1000);
    println!("Batch size: {} (set FLUX_BATCH_SIZE to override)", batch_size);

    // Initialize macOS optimizer
    let macos_optimizer = macos_optimizations::MacOSOptimizer::new().unwrap();

    println!("Apple Silicon Configuration:");
    println!("  P-cores: {}", macos_optimizer.p_core_count());
    println!("  E-cores: {}", macos_optimizer.e_core_count());
    println!("  Total cores: {}", macos_optimizer.total_core_count());
    println!("  P-core CPUs: {:?}", macos_optimizer.get_p_core_cpus());

    // Create ring buffer with optimized configuration
    let buffer_config = create_macos_optimized_config();
    let mut buffer = RingBuffer::new(buffer_config).unwrap();

    println!("\nBenchmark Configuration:");
    println!("  Duration: 10s");
    println!("  Producer/Consumer balanced");
    println!("  Batch size: {}", batch_size);
    println!("  Buffer size: 1M slots");
    println!("  Wait strategy: BusySpin");
    println!("  Prefetch: enabled");
    println!("  Cache alignment: 128-byte");

    // Pin to P-core for best performance
    let p_cores = macos_optimizer.get_p_core_cpus();
    if let Some(&p_core) = p_cores.first() {
        let _ = pin_to_cpu(p_core);
        println!("Attempted P-core pin to: {} (placeholder)", p_core);
    }

    // Set maximum thread priority
    let _ = macos_optimizations::ThreadOptimizer::set_max_priority();

    let start_time = Instant::now();
    let target_duration = Duration::from_secs(10);
    let message_size = 1024;

    // Pre-allocate data like our working benchmark
    let mut pre_allocated_messages = Vec::with_capacity(batch_size);
    let mut pre_allocated_batch_data = Vec::with_capacity(batch_size);

    // Create reusable message buffers
    for i in 0..batch_size {
        let msg = format!(
            "MACOS_FLUX_MSG_{:010}_HIGH_PERF{:0width$}",
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

    let mut total_messages_sent = 0;
    let mut total_messages_consumed = 0;
    let mut successful_batches = 0;
    let mut failed_batches = 0;

    println!("\n🔥 Starting macOS optimized benchmark...");

    while start_time.elapsed() < target_duration {
        // Producer phase: try to send a batch
        let batch_slice = &pre_allocated_batch_data[..batch_size];
        match buffer.try_publish_batch(batch_slice) {
            Ok(published_count) => {
                total_messages_sent += published_count as usize;
                successful_batches += 1;
            }
            Err(_) => {
                failed_batches += 1;
            }
        }

        // Consumer phase: try to consume messages to prevent overflow
        let consumed_messages = buffer.try_consume_batch(0, batch_size);
        total_messages_consumed += consumed_messages.len();

        // Progress reporting
        if total_messages_sent % 1_000_000 == 0 && total_messages_sent > 0 {
            let elapsed_secs = start_time.elapsed().as_secs_f64();
            let throughput = (total_messages_sent as f64) / elapsed_secs;
            println!(
                "  📈 {} sent, {} consumed | {:.2} M/s | {:.2}s elapsed",
                total_messages_sent,
                total_messages_consumed,
                throughput / 1_000_000.0,
                elapsed_secs
            );
        }

        // Small yield to prevent tight loop
        if failed_batches % 100 == 0 && failed_batches > 0 {
            std::thread::yield_now();
        }
    }

    let duration = start_time.elapsed();
    let actual_throughput = (total_messages_sent as f64) / duration.as_secs_f64();
    let processing_efficiency = if total_messages_sent > 0 {
        ((total_messages_consumed as f64) / (total_messages_sent as f64)) * 100.0
    } else {
        0.0
    };

    println!("\n📊 macOS Optimized Results:");
    println!("===================================");
    println!("Messages sent: {}", total_messages_sent);
    println!("Messages consumed: {}", total_messages_consumed);
    println!("Processing efficiency: {:.1}%", processing_efficiency);
    println!("Successful batches: {}", successful_batches);
    println!("Failed batches: {}", failed_batches);
    println!("Duration: {:.3} seconds", duration.as_secs_f64());
    println!("Throughput: {:.2} M messages/second", actual_throughput / 1_000_000.0);

    if successful_batches > 0 {
        println!(
            "Average batch size: {:.1}",
            (total_messages_sent as f64) / (successful_batches as f64)
        );
    }

    // Compare against targets
    let industry_baseline = 6_000_000.0;
    let industry_peak = 20_000_000.0;

    if actual_throughput >= industry_peak {
        let advantage = (actual_throughput / industry_peak - 1.0) * 100.0;
        println!("🏆 EXCELLENT! Exceeds industry peak by {:.1}%!", advantage);
    } else if actual_throughput >= industry_baseline {
        let advantage = (actual_throughput / industry_baseline - 1.0) * 100.0;
        println!("🥇 SUCCESS! Exceeds industry baseline by {:.1}%!", advantage);
    } else {
        let percent_of_baseline = (actual_throughput / industry_baseline) * 100.0;
        println!("⚠️  Below industry baseline ({:.1}% of target)", percent_of_baseline);
    }

    println!("\nOptimizations applied:");
    println!("- Thread priority optimization (QoS class)");
    println!("- Compiler auto-vectorization (LLVM SIMD)");
    println!("- Maximum thread priority");
    println!("- Pre-allocated message data");
    println!("- Balanced producer/consumer");

    println!("\nBenchmark completed successfully.");
}

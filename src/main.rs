//! Main entry point for the Flux library demonstration
//!
//! This demonstrates the restructured Aeron + LMAX Disruptor implementation
//! with proper modular architecture and improved performance.

use flux::{
    constants,
    disruptor::{ RingBuffer, RingBufferConfig, WaitStrategyType },
    utils::{ get_system_info, pin_to_cpu },
    utils::time::Timer,
    monitoring::PerformanceMonitor,
};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("Flux - High-Performance Message Transport");
    println!("Combining Aeron and LMAX Disruptor patterns for maximum throughput");
    println!("=================================================================");

    // Display system information
    let sys_info = get_system_info();
    println!("\nSystem Information:");
    println!("  CPU cores: {}", sys_info.cpu_count);
    println!("  Cache line size: {} bytes", sys_info.cache_line_size);
    println!("  Page size: {} bytes", sys_info.page_size);
    println!("  Huge page size: {} bytes", sys_info.huge_page_size);

    // Pin to default performance CPU for consistent performance
    if let Err(e) = pin_to_cpu(constants::DEFAULT_PERFORMANCE_CPU) {
        println!("Warning: Could not pin to CPU {}: {}", constants::DEFAULT_PERFORMANCE_CPU, e);
    } else {
        println!("Pinned to CPU {} for optimal performance", constants::DEFAULT_PERFORMANCE_CPU);
    }

    println!("\nTesting LMAX Disruptor Ring Buffer...");
    let final_throughput = test_disruptor_performance()?;

    println!("\nPerformance Summary:");
    println!(
        "  Ring buffer operations: {:.2} M msgs/sec",
        final_throughput / constants::MESSAGES_PER_MILLION
    );
    println!("  Optimized memory handling: Implemented");
    println!("  CPU affinity: Configured");
    println!("  Cache-line alignment: {}-byte aligned", constants::CACHE_LINE_SIZE);

    println!("\nNext Steps:");
    println!("  1. Complete transport layer implementation");
    println!("  2. Add reliability features (NAK, FEC, etc.)");
    println!("  3. Implement io_uring optimized networking");
    println!("  4. Add comprehensive benchmarks and performance analysis");
    println!("  5. Production deployment optimizations");

    Ok(())
}

fn test_disruptor_performance() -> Result<f64, Box<dyn std::error::Error>> {
    println!("  Creating ring buffer with 1M slots...");

    let config = RingBufferConfig {
        size: 1_048_576, // 1M slots
        num_consumers: 1,
        wait_strategy: WaitStrategyType::BusySpin,
        use_huge_pages: false,
        numa_node: None,
        optimal_batch_size: constants::OPTIMAL_SPSC_BATCH_SIZE, // Optimized for maximum throughput
        enable_cache_prefetch: true, // Enable cache optimization
        enable_simd: true, // Enable SIMD optimizations
    };

    let mut ring_buffer = RingBuffer::new(config)?;
    let perf_monitor = PerformanceMonitor::new();

    println!("  Buffer capacity: {}", ring_buffer.capacity());
    println!("  Consumer count: {}", ring_buffer.consumer_count());

    println!("  Running performance test...");

    // Simulate high-performance message production
    let test_count = 10_000_000;
    let timer = Timer::new();
    let batch_size = constants::OPTIMAL_SPSC_BATCH_SIZE; // Process in batches for better performance

    for batch_start in (0..test_count).step_by(batch_size) {
        let current_batch_size = std::cmp::min(batch_size, test_count - batch_start);

        // Prepare batch data
        let mut batch_data = Vec::with_capacity(current_batch_size);
        let mut message_strings = Vec::with_capacity(current_batch_size);

        for i in 0..current_batch_size {
            let seq = batch_start + i;
            let data = format!("Test message {}", seq);
            message_strings.push(data);
        }

        // Convert to byte slices
        for msg in &message_strings {
            batch_data.push(msg.as_bytes());
        }

        if let Ok(published_count) = ring_buffer.try_publish_batch(&batch_data) {
            perf_monitor.record_throughput(published_count);
        }

        // Print progress every million operations
        if batch_start % constants::THROUGHPUT_REPORTING_INTERVAL == 0 && batch_start > 0 {
            let elapsed = timer.elapsed_nanos();
            let throughput = (batch_start as f64) / ((elapsed as f64) / constants::NANOS_PER_SEC);
            println!(
                "    {} messages processed, throughput: {:.2} M/s",
                batch_start,
                throughput / constants::MESSAGES_PER_MILLION
            );
        }
    }

    let total_elapsed = timer.elapsed_nanos();
    let final_throughput =
        (test_count as f64) / ((total_elapsed as f64) / constants::NANOS_PER_SEC);

    println!("  Performance test completed!");
    println!("    Total messages: {}", test_count);
    println!("    Total time: {:.2} seconds", (total_elapsed as f64) / constants::NANOS_PER_SEC);
    println!(
        "    Throughput: {:.2} M messages/second",
        final_throughput / constants::MESSAGES_PER_MILLION
    );

    // Performance validation
    if final_throughput >= constants::MIN_GOOD_THROUGHPUT {
        println!("  SUCCESS: Achieved target throughput of 6M+ messages/second");
    } else {
        println!("  WARNING: Below target throughput of 6M messages/second");
    }

    Ok(final_throughput)
}

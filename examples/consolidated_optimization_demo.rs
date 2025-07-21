//! Demonstration of consolidated optimization modules
//! Shows how SIMD optimizations and zero-copy transport work together

use flux::{
    optimizations::{ OptimizationConfig, OptimizationManager },
    transport::optimized_copy::{ OptimizedCopyUdpTransport, example_optimized_copy },
    utils::cpu::{ get_cpu_info, get_simd_level },
    error::Result,
};

fn main() -> Result<()> {
    println!("🚀 Flux Consolidated Optimization Demo");
    println!("=====================================");

    // Show CPU capabilities
    let cpu_info = get_cpu_info();
    let simd_level = get_simd_level();

    println!("\n🖥️  System Information:");
    println!("SIMD Level: {:?}", simd_level);
    println!("Cache Line Size: {} bytes", cpu_info.cache_line_size);
    println!("L1 Cache: {} KB", cpu_info.l1_cache_size / 1024);
    println!("L2 Cache: {} KB", cpu_info.l2_cache_size / 1024);

    // Initialize optimizations
    let config = OptimizationConfig {
        enable_simd: true,
        enable_pooling: true,
        enable_zero_copy: true,
        enable_cpu_optimizations: true,
        enable_numa: true,
        enable_hardware_acceleration: true,
    };

    let optimization_manager = OptimizationManager::new(config);

    println!("\n🔧 Optimization Manager:");
    println!("SIMD Enabled: {}", optimization_manager.is_simd_enabled());
    println!("Pooling Enabled: {}", optimization_manager.is_pooling_enabled());
    println!("Zero-Copy Enabled: {}", optimization_manager.is_zero_copy_enabled());

    // Test SIMD optimizer directly
    println!("\n⚡ SIMD Optimizer Test:");
    if let Some(simd_optimizer) = optimization_manager.simd_optimizer() {
        println!("AVX2 Available: {}", simd_optimizer.avx2_available());
        println!("AVX-512 Available: {}", simd_optimizer.avx512_available());
        println!("Optimal Batch Size: {}", simd_optimizer.optimal_batch_size());

        // Test SIMD copy performance
        let test_data = vec![0xAAu8; 1024];
        let mut dst = vec![0u8; 1024];

        unsafe {
            let copied = simd_optimizer.simd_copy(&mut dst, &test_data);
            println!("SIMD Copy: {} bytes copied successfully", copied);

            let checksum = simd_optimizer.simd_checksum(&test_data);
            println!("SIMD Checksum: 0x{:08x}", checksum);
        }
    }

    // Test optimized copy transport
    println!("\n🚀 Optimized Copy Transport Test:");
    example_optimized_copy()?;

    // Create advanced transport
    let transport = OptimizedCopyUdpTransport::new(1000, 4096, 64)?;
    let stats = transport.pool_stats();

    println!("Transport Pool Statistics:");
    println!("  Pool Utilization: {:.1}%", transport.pool_utilization() * 100.0);
    println!("  Pool Hits: {}", stats.pool_hits.load(std::sync::atomic::Ordering::Relaxed));

    // Run benchmarks
    println!("\n📊 Running Benchmarks:");
    optimization_manager.run_benchmarks();

    println!("\n✅ Consolidated optimization demo completed successfully!");

    Ok(())
}

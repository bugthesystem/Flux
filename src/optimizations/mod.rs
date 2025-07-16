//! Advanced performance optimizations for maximum throughput
//! Target: Defeat Aeron and LMAX Disruptor performance

pub mod advanced_simd;
pub mod memory_pool;

pub use advanced_simd::{ SimdOptimizer, SimdMemoryOps, benchmark_simd_optimizations };
pub use memory_pool::{
    MessagePool,
    PooledMessageSlot,
    ZeroCopyBatch,
    PooledMessageProcessor,
    benchmark_memory_pool,
};

/// Performance optimization configuration
#[derive(Debug, Clone)]
pub struct OptimizationConfig {
    /// Enable SIMD optimizations
    pub enable_simd: bool,
    /// Enable memory pooling
    pub enable_pooling: bool,
    /// Enable zero-copy operations
    pub enable_zero_copy: bool,
    /// Enable advanced CPU optimizations
    pub enable_cpu_optimizations: bool,
    /// Enable NUMA optimizations
    pub enable_numa: bool,
    /// Enable hardware acceleration
    pub enable_hardware_acceleration: bool,
}

impl Default for OptimizationConfig {
    fn default() -> Self {
        Self {
            enable_simd: true,
            enable_pooling: true,
            enable_zero_copy: true,
            enable_cpu_optimizations: true,
            enable_numa: false, // Requires NUMA-aware system
            enable_hardware_acceleration: false, // Requires specific hardware
        }
    }
}

/// Performance optimization manager
pub struct OptimizationManager {
    /// Configuration
    config: OptimizationConfig,
    /// SIMD optimizer
    simd_optimizer: Option<SimdOptimizer>,
    /// Memory pool
    memory_pool: Option<MessagePool>,
}

impl OptimizationManager {
    /// Create new optimization manager
    pub fn new(config: OptimizationConfig) -> Self {
        let simd_optimizer = if config.enable_simd { Some(SimdOptimizer::new()) } else { None };

        let memory_pool = if config.enable_pooling {
            Some(MessagePool::new(10000)) // 10K slots
        } else {
            None
        };

        Self {
            config,
            simd_optimizer,
            memory_pool,
        }
    }

    /// Get SIMD optimizer
    pub fn simd_optimizer(&self) -> Option<&SimdOptimizer> {
        self.simd_optimizer.as_ref()
    }

    /// Get memory pool
    pub fn memory_pool(&self) -> Option<&MessagePool> {
        self.memory_pool.as_ref()
    }

    /// Check if optimizations are enabled
    pub fn is_simd_enabled(&self) -> bool {
        self.config.enable_simd
    }

    pub fn is_pooling_enabled(&self) -> bool {
        self.config.enable_pooling
    }

    pub fn is_zero_copy_enabled(&self) -> bool {
        self.config.enable_zero_copy
    }

    /// Run all optimization benchmarks
    pub fn run_benchmarks(&self) {
        println!("🚀 Running Performance Optimization Benchmarks");
        println!("=============================================");

        if self.config.enable_simd {
            println!("\n📊 SIMD Optimization Benchmark:");
            benchmark_simd_optimizations();
        }

        if self.config.enable_pooling {
            println!("\n📊 Memory Pool Benchmark:");
            benchmark_memory_pool();
        }

        println!("\n🎯 Optimization Summary:");
        println!("SIMD: {}", if self.config.enable_simd { "✅ Enabled" } else { "❌ Disabled" });
        println!("Memory Pooling: {}", if self.config.enable_pooling {
            "✅ Enabled"
        } else {
            "❌ Disabled"
        });
        println!("Zero-Copy: {}", if self.config.enable_zero_copy {
            "✅ Enabled"
        } else {
            "❌ Disabled"
        });
        println!("CPU Optimizations: {}", if self.config.enable_cpu_optimizations {
            "✅ Enabled"
        } else {
            "❌ Disabled"
        });
        println!("NUMA: {}", if self.config.enable_numa { "✅ Enabled" } else { "❌ Disabled" });
        println!("Hardware Acceleration: {}", if self.config.enable_hardware_acceleration {
            "✅ Enabled"
        } else {
            "❌ Disabled"
        });
    }
}

/// Performance optimization utilities
pub mod utils {
    use super::*;

    /// Apply all optimizations to a configuration
    pub fn apply_optimizations(
        config: crate::disruptor::RingBufferConfig
    ) -> crate::disruptor::RingBufferConfig {
        config
            .with_cache_prefetch(true)
            .with_simd_optimizations(true)
            .with_wait_strategy(crate::disruptor::WaitStrategyType::BusySpin)
    }

    /// Get optimal batch size based on CPU features
    pub fn get_optimal_batch_size() -> usize {
        let optimizer = SimdOptimizer::new();
        optimizer.optimal_batch_size()
    }

    /// Get optimal ring buffer size
    pub fn get_optimal_buffer_size() -> usize {
        1024 * 1024 // 1M slots
    }
}

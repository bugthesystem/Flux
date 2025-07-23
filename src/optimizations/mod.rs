//! Advanced performance optimizations for maximum throughput
//! Target: High-performance messaging comparable to industry standards

pub mod advanced_simd;

pub use advanced_simd::{ SimdOptimizer, SimdMemoryOps };

#[cfg(target_os = "macos")]
pub mod macos_optimizations;

pub mod linux_optimizations;

pub use self::linux_optimizations::{
    linux_lock_memory,
    linux_allocate_huge_pages,
    linux_pin_to_cpu,
    linux_set_max_priority,
};

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

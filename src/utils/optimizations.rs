//! General optimization helpers for Flux

use crate::platform::crossx::simd::SimdOptimizer;

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

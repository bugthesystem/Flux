//! Flux Performance Constants and Configuration
//!
//! This module contains performance tuning constants and configuration values
//! used throughout the Flux library.

/// Default ring buffer size (must be power of 2 for efficient modulo operations)
pub const DEFAULT_RING_BUFFER_SIZE: usize = 1024 * 1024; // 1M slots

/// Maximum ring buffer size for extreme performance scenarios
pub const MAX_RING_BUFFER_SIZE: usize = 4 * 1024 * 1024; // 4M slots

/// Cache line size for alignment optimizations (64 bytes on most modern CPUs)
pub const CACHE_LINE_SIZE: usize = 64;

/// Maximum transmission unit for UDP packets (Ethernet MTU - headers)
pub const MAX_UDP_PAYLOAD: usize = 1500;

/// Maximum batch size for optimal performance
pub const MAX_BATCH_SIZE: usize = 64;

/// Optimal batch size for single-producer scenarios
pub const OPTIMAL_SPSC_BATCH_SIZE: usize = 1000;

/// Optimal batch size for multi-producer scenarios
pub const OPTIMAL_MPMC_BATCH_SIZE: usize = 1000;

/// Extreme batch size for maximum throughput scenarios
pub const EXTREME_BATCH_SIZE: usize = 2000;

/// Maximum data size for a single message slot (1KB)
pub const MAX_MESSAGE_DATA_SIZE: usize = 1024;

/// Message slot alignment for cache efficiency
pub const MESSAGE_SLOT_ALIGNMENT: usize = 64;

/// Size of message slot header (sequence, timestamp, session_id, etc.)
pub const MESSAGE_SLOT_HEADER_SIZE: usize = 32;

/// Total size of a message slot (header + data)
pub const MESSAGE_SLOT_SIZE: usize = MESSAGE_SLOT_HEADER_SIZE + MAX_MESSAGE_DATA_SIZE;

/// Number of blocks in the memory pool
pub const POOL_BLOCK_COUNT: usize = 10000;

/// Number of cache lines to prefetch for optimal performance
pub const CACHE_PREFETCH_LINES: usize = 4;

/// Memory alignment for optimal CPU access patterns
pub const MEMORY_ALIGNMENT: usize = 64;

/// Page size for memory allocation optimizations
pub const PAGE_SIZE: usize = 4096;

/// Huge page size (2MB on most systems)
pub const HUGE_PAGE_SIZE: usize = 2 * 1024 * 1024;

/// Default UDP socket buffer size (8MB for high-throughput scenarios)
pub const DEFAULT_SOCKET_BUFFER_SIZE: usize = 8 * 1024 * 1024;

/// Maximum number of concurrent connections
pub const MAX_CONNECTIONS: usize = 10000;

/// Default heartbeat interval in nanoseconds (1 second)
pub const DEFAULT_HEARTBEAT_INTERVAL_NS: u64 = 1_000_000_000;

/// Default connection timeout in nanoseconds (30 seconds)
pub const DEFAULT_CONNECTION_TIMEOUT_NS: u64 = 30_000_000_000;

/// Maximum number of retransmission attempts
pub const MAX_RETRANSMISSION_ATTEMPTS: usize = 3;

/// Default NAK timeout in nanoseconds (100ms)
pub const DEFAULT_NAK_TIMEOUT_NS: u64 = 100_000_000;

/// Forward Error Correction data shards (default: 8)
pub const DEFAULT_FEC_DATA_SHARDS: usize = 8;

/// Forward Error Correction parity shards (default: 2)
pub const DEFAULT_FEC_PARITY_SHARDS: usize = 2;

/// Default benchmark duration in seconds
pub const DEFAULT_BENCHMARK_DURATION_SECS: u64 = 5;

/// Default warmup messages for benchmarks
pub const DEFAULT_WARMUP_MESSAGES: usize = 1_000_000;

/// Default test messages for benchmarks
pub const DEFAULT_TEST_MESSAGES: usize = 10_000_000;

/// Maximum number of CPU cores to use for benchmarks
pub const MAX_BENCHMARK_CPU_CORES: usize = 8;

/// Default CPU affinity for producer threads
pub const DEFAULT_PRODUCER_CPU_CORE: usize = 0;

/// Default CPU affinity for consumer threads
pub const DEFAULT_CONSUMER_CPU_CORE: usize = 1;

/// Minimum data size for SIMD optimization (32 bytes for AVX2)
pub const SIMD_MIN_DATA_SIZE: usize = 32;

/// Word size for optimized memory operations (8 bytes for u64)
pub const OPTIMIZED_WORD_SIZE: usize = 8;

/// Number of words to process in SIMD operations
pub const SIMD_WORDS_PER_OPERATION: usize = 4;

/// Maximum sequence number before wrapping
pub const MAX_SEQUENCE_NUMBER: u64 = u64::MAX;

/// Maximum session ID value
pub const MAX_SESSION_ID: u32 = u32::MAX;

/// Maximum timestamp value (nanoseconds since epoch)
pub const MAX_TIMESTAMP: u64 = u64::MAX;

/// Default CPU for performance-critical threads
/// - CPU 0 is typically reserved for system tasks on some systems
/// - Use with caution and proper capability detection
pub const DEFAULT_PERFORMANCE_CPU: usize = 0;

/// Nanoseconds per second for time calculations
/// - Used in throughput calculations: msgs/sec = count / (nanos / NANOS_PER_SEC)
pub const NANOS_PER_SEC: f64 = 1_000_000_000.0;

/// Messages per million for throughput display
/// - Display convenience: 22_464_890 msgs/sec → 22.46 M msgs/sec
pub const MESSAGES_PER_MILLION: f64 = 1_000_000.0;

/// Throughput reporting interval
/// - Report progress every 1M messages processed
/// - Provides user feedback without overwhelming console output
pub const THROUGHPUT_REPORTING_INTERVAL: usize = 1_000_000;

/// Minimum throughput for success classification
/// - 6M msgs/sec is considered "good" performance baseline
/// - Based on industry standards (LMAX Disruptor, Aeron benchmarks)
pub const MIN_GOOD_THROUGHPUT: f64 = 6_000_000.0;

/// NAK (Negative Acknowledgment) timeout for reliable UDP
/// - 50ms timeout before requesting retransmission
/// - Tuned for high-frequency networks (1-10ms RTT)
/// - Shorter timeouts improve reliability, longer timeouts reduce overhead
pub const NAK_TIMEOUT_MS: u64 = 50;

/// Out-of-order buffer size for reliable UDP
/// - 1000 messages can arrive out-of-order before dropping
/// - Handles network jitter and routing variations
/// - Memory usage: ~1000 * average_message_size
pub const MAX_OUT_OF_ORDER_MESSAGES: usize = 1000;

/// Heartbeat interval for connection keep-alive
/// - 1 second interval balances network overhead vs. quick failure detection
/// - Recommended for reliable UDP connections
pub const HEARTBEAT_INTERVAL_MS: u64 = 1000;

/// Session timeout for connection management
/// - 30 seconds before considering a peer disconnected
/// - Allows for temporary network issues while preventing resource leaks
pub const SESSION_TIMEOUT_MS: u64 = 30000;
/// Validate that all constants are properly configured
pub fn validate_constants() -> Result<(), &'static str> {
    // Validate ring buffer sizes are powers of 2
    if !DEFAULT_RING_BUFFER_SIZE.is_power_of_two() {
        return Err("DEFAULT_RING_BUFFER_SIZE must be a power of 2");
    }
    if !MAX_RING_BUFFER_SIZE.is_power_of_two() {
        return Err("MAX_RING_BUFFER_SIZE must be a power of 2");
    }

    // Validate batch sizes are reasonable
    if OPTIMAL_SPSC_BATCH_SIZE == 0 || OPTIMAL_MPMC_BATCH_SIZE == 0 {
        return Err("Batch sizes must be greater than 0");
    }

    // Validate message size limits
    if MAX_MESSAGE_DATA_SIZE == 0 || MESSAGE_SLOT_SIZE == 0 {
        return Err("Message sizes must be greater than 0");
    }

    // Validate cache line size
    if CACHE_LINE_SIZE == 0 || !CACHE_LINE_SIZE.is_power_of_two() {
        return Err("CACHE_LINE_SIZE must be a power of 2");
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_constants_validation() {
        assert!(validate_constants().is_ok());
    }

    #[test]
    fn test_ring_buffer_sizes_are_powers_of_two() {
        assert!(DEFAULT_RING_BUFFER_SIZE.is_power_of_two());
        assert!(MAX_RING_BUFFER_SIZE.is_power_of_two());
    }

    #[test]
    fn test_batch_sizes_are_reasonable() {
        assert!(OPTIMAL_SPSC_BATCH_SIZE > 0);
        assert!(OPTIMAL_MPMC_BATCH_SIZE > 0);
        assert!(EXTREME_BATCH_SIZE > 0);
    }

    #[test]
    fn test_message_size_limits() {
        assert!(MAX_MESSAGE_DATA_SIZE > 0);
        assert!(MESSAGE_SLOT_SIZE > MESSAGE_SLOT_HEADER_SIZE);
    }

    #[test]
    fn test_cache_line_size_is_power_of_two() {
        assert!(CACHE_LINE_SIZE.is_power_of_two());
    }
}

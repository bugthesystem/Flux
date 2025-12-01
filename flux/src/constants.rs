//! Flux Performance Constants
//!
//! Core constants used by the ring buffer implementations.

/// Default ring buffer size (must be power of 2)
pub const DEFAULT_RING_BUFFER_SIZE: usize = 1024 * 1024; // 1M slots

/// Maximum ring buffer size
pub const MAX_RING_BUFFER_SIZE: usize = 4 * 1024 * 1024; // 4M slots

/// Cache line size for alignment (64 bytes on most CPUs)
pub const CACHE_LINE_SIZE: usize = 64;

/// Maximum data size for a single message slot
pub const MAX_MESSAGE_DATA_SIZE: usize = 1024;

/// Extreme batch size for maximum throughput
pub const EXTREME_BATCH_SIZE: usize = 2000;

/// Page size for memory allocation
pub const PAGE_SIZE: usize = 4096;

/// Huge page size (2MB)
pub const HUGE_PAGE_SIZE: usize = 2 * 1024 * 1024;

/// Number of cache lines to prefetch
pub const CACHE_PREFETCH_LINES: usize = 4;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ring_buffer_sizes_are_powers_of_two() {
        assert!(DEFAULT_RING_BUFFER_SIZE.is_power_of_two());
        assert!(MAX_RING_BUFFER_SIZE.is_power_of_two());
    }

    #[test]
    fn test_cache_line_size_is_power_of_two() {
        assert!(CACHE_LINE_SIZE.is_power_of_two());
    }

    #[test]
    fn test_page_sizes_are_powers_of_two() {
        assert!(PAGE_SIZE.is_power_of_two());
        assert!(HUGE_PAGE_SIZE.is_power_of_two());
    }
}

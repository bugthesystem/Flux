//! Kaos Performance Constants
//!
//! Core constants used by the ring buffer implementations.

/// Default ring buffer size (must be power of 2)
pub const DEFAULT_RING_BUFFER_SIZE: usize = 1024 * 1024; // 1M slots

/// Maximum data size for a single message slot
pub const MAX_MESSAGE_DATA_SIZE: usize = 1024;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ring_buffer_sizes_are_powers_of_two() {
        assert!(DEFAULT_RING_BUFFER_SIZE.is_power_of_two());
    }
}

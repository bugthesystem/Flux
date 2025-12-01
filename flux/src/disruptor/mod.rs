//! Lock-free ring buffers based on the LMAX Disruptor pattern.
//!
//! ## Implementations
//!
//! | Pattern | Type | Use Case |
//! |---------|------|----------|
//! | SPSC | `RingBuffer<T>` | Generic high-throughput |
//! | SPSC | `MessageRingBuffer` | Variable-length messages (128B) |
//! | MPSC | `MpscRingBuffer<T>` | Multiple producers |
//! | SPMC | `SpmcRingBuffer<T>` | Fan-out pattern |
//! | MPMC | `MpmcRingBuffer<T>` | Full flexibility |
//!
//! ## Features
//!
//! - Lock-free operation using atomic operations
//! - Batch processing for high throughput
//! - Cache-aligned to prevent false sharing
//! - Generic slot types (`SmallSlot`, `Slot16`, `Slot32`, `Slot64`)
//! - Memory-mapped allocation option (`new_mapped()`)
//!
//! See examples/ for usage patterns.
//!
//! ## Module Organization
//!
//! - `spsc/` - Single Producer, Single Consumer
//! - `mpsc/` - Multi Producer, Single Consumer
//! - `spmc/` - Single Producer, Multi Consumer
//! - `mpmc/` - Multi Producer, Multi Consumer
//!
//! ## Safety
//!
//! Uses `unsafe` for direct memory access. Caller must ensure:
//! - Sequences are within claimed ranges
//! - Proper synchronization between threads
//!
//! See README for benchmark results and hardware requirements.

pub mod ring_buffer_core;
pub mod completion_tracker;
pub mod spsc;
pub mod mpsc;
pub mod spmc;
pub mod mpmc;
pub mod message_slot;
pub mod small_slot;
pub mod slots;
pub mod wait_strategy;
pub mod common;
pub mod macros;
pub mod claim_batch;

// Ring buffer types
pub use spsc::{ RingBuffer, MessageRingBuffer, SharedRingBuffer };

// Generic RingBuffer<T> producer/consumer
pub use spsc::{
    RingProducer,
    RingProducerBuilder,
    RingConsumer,
    RingConsumerBuilder,
    RingEventHandler,
};

// MessageRingBuffer producer/consumer
pub use spsc::{ Producer, ProducerBuilder, Consumer, ConsumerBuilder, EventHandler };

pub use mpsc::{
    MpscRingBuffer,
    MpscProducer,
    MpscProducerBuilder,
    MpscConsumer,
    MpscConsumerBuilder,
    MpscEventHandler,
};
pub use spmc::SpmcRingBuffer;
pub use mpmc::MpmcRingBuffer;
pub use completion_tracker::{ ReadGuard, BatchReadGuard, ReadableRing, CompletionTracker };
pub use message_slot::{ MessageSlot, MessageFlags };
pub use small_slot::SmallSlot;
pub use slots::{ Slot16, Slot32, Slot64 };
pub use wait_strategy::{ WaitStrategy, BusySpinWaitStrategy, BlockingWaitStrategy };
pub use claim_batch::ClaimBatch;

use crate::error::{ Result, FluxError };
use crate::constants::DEFAULT_RING_BUFFER_SIZE;

/// Default ring buffer size (1M entries)
///
/// This constant is deprecated. Use `DEFAULT_RING_BUFFER_SIZE` from the constants module instead.
#[deprecated(since = "0.1.0", note = "Use DEFAULT_RING_BUFFER_SIZE from constants module")]
pub const DEFAULT_RING_SIZE: usize = DEFAULT_RING_BUFFER_SIZE;

/// Sequence number type for ring buffer positions
pub type Sequence = u64;

/// Trait for objects that can be stored in the ring buffer
pub trait RingBufferEntry: Clone + Default + Send + Sync + 'static {
    /// Get the sequence number of this entry
    fn sequence(&self) -> Sequence;

    /// Set the sequence number of this entry
    fn set_sequence(&mut self, seq: Sequence);

    /// Reset the entry to its default state
    fn reset(&mut self);
}

/// Configuration for ring buffer behavior
#[derive(Debug, Clone)]
pub struct RingBufferConfig {
    /// Size of the ring buffer (must be power of 2)
    pub size: usize,
    /// Number of consumers
    pub num_consumers: usize,
    /// Wait strategy for consumers
    pub wait_strategy: WaitStrategyType,
    /// Optimal batch size for maximum throughput
    pub optimal_batch_size: usize,
    /// Whether to enable cache prefetching
    pub enable_cache_prefetch: bool,
    /// Whether to use SIMD optimizations
    pub enable_simd: bool,
    /// Strict message ordering - NO MESSAGE LOSS on ring wrap (trading mode)
    /// true = 100% delivery, slightly slower | false = 99.84%, faster
    pub strict_message_ordering: bool,
    /// Block producer when ring full (guaranteed delivery)
    /// true = 100% delivery, 6x slower | false = 99.7% delivery, 6x faster
    pub block_on_full: bool,
}

/// Available wait strategies
#[derive(Debug, Clone, Copy)]
pub enum WaitStrategyType {
    /// Busy spin for lowest latency
    BusySpin,
    /// Block with yielding for balanced performance
    Blocking,
    /// Sleep for lowest CPU usage
    Sleeping,
}

impl Default for RingBufferConfig {
    fn default() -> Self {
        Self {
            size: DEFAULT_RING_BUFFER_SIZE,
            num_consumers: 1,
            wait_strategy: WaitStrategyType::BusySpin,
            optimal_batch_size: 1000,
            enable_cache_prefetch: true,
            enable_simd: true,
            strict_message_ordering: true,
            block_on_full: false, // Fast mode default (use claim_slots_relaxed() for 100%)
        }
    }
}

impl RingBufferConfig {
    /// Create a new configuration with the specified size
    pub fn new(size: usize) -> Result<Self> {
        if !size.is_power_of_two() {
            return Err(FluxError::config("Ring buffer size must be power of 2"));
        }
        if size == 0 {
            return Err(FluxError::config("Ring buffer size must be greater than 0"));
        }

        Ok(Self {
            size,
            ..Default::default()
        })
    }

    /// Set the number of consumers
    pub fn with_consumers(mut self, num_consumers: usize) -> Result<Self> {
        if num_consumers == 0 {
            return Err(FluxError::config("Number of consumers must be greater than 0"));
        }
        if num_consumers > self.size {
            return Err(FluxError::config("Number of consumers cannot exceed ring buffer size"));
        }

        self.num_consumers = num_consumers;
        Ok(self)
    }

    /// Set the wait strategy
    pub fn with_wait_strategy(mut self, strategy: WaitStrategyType) -> Self {
        self.wait_strategy = strategy;
        self
    }

    /// Set optimal batch size for maximum throughput
    pub fn with_optimal_batch_size(mut self, batch_size: usize) -> Self {
        self.optimal_batch_size = batch_size;
        self
    }

    /// Enable or disable cache prefetching
    pub fn with_cache_prefetch(mut self, enable: bool) -> Self {
        self.enable_cache_prefetch = enable;
        self
    }

    /// Enable or disable SIMD optimizations
    pub fn with_simd_optimizations(mut self, enable: bool) -> Self {
        self.enable_simd = enable;
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ring_buffer_config_creation() {
        let config = RingBufferConfig::new(1024).unwrap();
        assert_eq!(config.size, 1024);
        assert_eq!(config.num_consumers, 1);
    }

    #[test]
    fn test_ring_buffer_config_invalid_size() {
        assert!(RingBufferConfig::new(0).is_err());
        assert!(RingBufferConfig::new(1023).is_err()); // Not power of 2
    }

    #[test]
    fn test_ring_buffer_config_builder() {
        let config = RingBufferConfig::new(1024)
            .unwrap()
            .with_consumers(4)
            .unwrap()
            .with_wait_strategy(WaitStrategyType::Blocking);

        assert_eq!(config.size, 1024);
        assert_eq!(config.num_consumers, 4);
        assert!(matches!(config.wait_strategy, WaitStrategyType::Blocking));
    }

    #[test]
    fn test_ring_buffer_config_invalid_consumers() {
        let result = RingBufferConfig::new(1024).unwrap().with_consumers(0);
        assert!(result.is_err());

        let result = RingBufferConfig::new(1024).unwrap().with_consumers(2000);
        assert!(result.is_err());
    }
}

//! Lock-free ring buffers (LMAX Disruptor pattern).
//!
//! - `RingBuffer<T>` - SPSC, generic slots
//! - `MessageRingBuffer` - SPSC, 128B variable messages
//! - `MpscRingBuffer<T>` - Multiple producers
//! - `SpmcRingBuffer<T>` - Fan-out
//! - `MpmcRingBuffer<T>` - Full flexibility

pub mod ring_buffer_core;
pub mod completion_tracker;
pub mod spsc;
pub mod mpsc;
pub mod spmc;
pub mod mpmc;
pub mod message_slot;
pub mod slots;
pub mod common;
pub mod macros;

// Ring buffer types
pub use spsc::{ RingBuffer, MessageRingBuffer, SharedRingBuffer };

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
pub use message_slot::MessageSlot;
pub use slots::Slot8;
pub use slots::{ Slot16, Slot32, Slot64 };

use crate::error::{ Result, KaosError };
use crate::constants::DEFAULT_RING_BUFFER_SIZE;

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
}

impl Default for RingBufferConfig {
    fn default() -> Self {
        Self {
            size: DEFAULT_RING_BUFFER_SIZE,
            num_consumers: 1,
        }
    }
}

impl RingBufferConfig {
    /// Create a new configuration with the specified size
    pub fn new(size: usize) -> Result<Self> {
        if !size.is_power_of_two() {
            return Err(KaosError::config("Ring buffer size must be power of 2"));
        }
        if size == 0 {
            return Err(KaosError::config("Ring buffer size must be greater than 0"));
        }

        Ok(Self {
            size,
            ..Default::default()
        })
    }

    /// Set the number of consumers
    pub fn with_consumers(mut self, num_consumers: usize) -> Result<Self> {
        if num_consumers == 0 {
            return Err(KaosError::config("Number of consumers must be greater than 0"));
        }
        if num_consumers > self.size {
            return Err(KaosError::config("Number of consumers cannot exceed ring buffer size"));
        }

        self.num_consumers = num_consumers;
        Ok(self)
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
        let config = RingBufferConfig::new(1024).unwrap().with_consumers(4).unwrap();

        assert_eq!(config.size, 1024);
        assert_eq!(config.num_consumers, 4);
    }

    #[test]
    fn test_ring_buffer_config_invalid_consumers() {
        let result = RingBufferConfig::new(1024).unwrap().with_consumers(0);
        assert!(result.is_err());

        let result = RingBufferConfig::new(1024).unwrap().with_consumers(2000);
        assert!(result.is_err());
    }
}

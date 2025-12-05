//! Lock-free ring buffers (LMAX Disruptor pattern).
//!
//! - `RingBuffer<T>` - SPSC, single consumer (fastest)
//! - `BroadcastRingBuffer<T>` - SPSC, multiple consumers see ALL messages
//! - `SpmcRingBuffer<T>` - Fan-out work distribution (each msg to ONE consumer)
//! - `MpscRingBuffer<T>` - Multiple producers, single consumer
//! - `MpmcRingBuffer<T>` - Full flexibility (slowest)

mod completion;
mod ipc;
pub mod macros;
mod multi;
mod single;
mod slots;

// Re-exports
pub use completion::{BatchReadGuard, CompletionTracker, ReadGuard, ReadableRing};
pub use ipc::SharedRingBuffer;
pub use multi::{
    MpmcRingBuffer, MpscConsumer, MpscConsumerBuilder, MpscEventHandler, MpscProducer,
    MpscProducerBuilder, MpscRingBuffer, SpmcRingBuffer,
};
pub use single::{
    BroadcastRingBuffer, Consumer, ConsumerBuilder, EventHandler, FastProducer, MessageRingBuffer,
    Producer, ProducerBuilder, RingBuffer,
};
pub use slots::{MessageSlot, Slot16, Slot32, Slot64, Slot8};

use crate::error::{KaosError, Result};

/// Default ring buffer size (must be power of 2)
const DEFAULT_RING_BUFFER_SIZE: usize = 64 * 1024; // 64K slots

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
            return Err(KaosError::config(
                "Number of consumers must be greater than 0",
            ));
        }
        if num_consumers > self.size {
            return Err(KaosError::config(
                "Number of consumers cannot exceed ring buffer size",
            ));
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
        let config = RingBufferConfig::new(1024)
            .unwrap()
            .with_consumers(4)
            .unwrap();

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

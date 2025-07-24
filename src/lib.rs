//! Flux - High-performance message transport library

pub mod constants;
pub mod disruptor;
pub mod error;
pub mod monitoring;
pub mod transport;
pub mod utils;
pub mod reliability;
pub mod platform;

// Re-export main components
pub use disruptor::{ RingBuffer, RingBufferConfig, MessageSlot, WaitStrategyType };
pub use error::{ Result, FluxError };
pub use transport::{
    UdpRingBufferTransport,
    UdpTransportConfig,
    TransportConfig,
    TransportMetrics,
    reliable_udp::ReliableUdpRingBufferTransport,
};
pub use monitoring::{ PerformanceMonitor, PerformanceStats };
pub use reliability::{ ReliabilityConfig, FecEncoder };

/// Version information
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

#[cfg(test)]
mod tests {
    use super::*;
    use crate::disruptor::RingBufferEntry;

    #[test]
    fn test_ring_buffer_creation() {
        let config = RingBufferConfig::new(1024).unwrap().with_consumers(1).unwrap();

        let ring_buffer = RingBuffer::new(config);
        assert!(ring_buffer.is_ok());
    }

    #[test]
    fn test_message_slot_operations() {
        let mut slot = MessageSlot::default();
        slot.set_data(b"Hello, Flux!");

        assert_eq!(slot.data(), b"Hello, Flux!");
    }

    #[test]
    fn test_batch_operations() {
        let config = RingBufferConfig::new(1024).unwrap().with_consumers(1).unwrap();

        let mut ring_buffer = RingBuffer::new(config).unwrap();

        // Send batch
        let mut claimed = 0;
        if let Some((seq, slots)) = ring_buffer.try_claim_slots(3) {
            claimed = slots.len();
            for (i, slot) in slots.iter_mut().enumerate() {
                slot.set_sequence(seq + (i as u64));
                slot.set_data(b"Message");
            }
            ring_buffer.publish_batch(seq, claimed);
        }
        println!("[TEST DEBUG] claimed = {}", claimed);

        // Receive all available messages in batches
        let mut total_received = 0;
        loop {
            let messages = ring_buffer.try_consume_batch(0, 3);
            println!("[TEST DEBUG] messages.len() = {}", messages.len());
            if messages.is_empty() {
                break;
            }
            total_received += messages.len();
        }
        assert_eq!(total_received, claimed);
    }
}

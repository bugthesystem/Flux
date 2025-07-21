//! Flux - High-performance message transport library

pub mod constants;
pub mod disruptor;
pub mod error;
pub mod optimizations;
pub mod performance;
pub mod reliability;
pub mod transport;
pub mod utils;

// Re-export main components
pub use disruptor::{ RingBuffer, RingBufferConfig, MessageSlot, WaitStrategyType };
pub use error::{ Result, FluxError };
pub use transport::{
    BasicUdpTransport,
    BasicUdpConfig,
    TransportConfig,
    TransportMetrics,
    reliable_udp::{ ReliableUdpTransport, ReliableUdpConfig },
};
pub use optimizations::{ OptimizationManager, OptimizationConfig };

/// Version information
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// High-performance message transport
pub struct Flux {
    /// Ring buffer for message queuing
    _ring_buffer: RingBuffer,
    /// Transport layer
    transport: Option<ReliableUdpTransport>,
    /// Optimization manager
    optimizations: OptimizationManager,
}

impl Flux {
    /// Create new Flux instance
    pub fn new(config: RingBufferConfig) -> Result<Self> {
        let ring_buffer = RingBuffer::new(config)?;
        let optimizations = OptimizationManager::new(OptimizationConfig::default());

        Ok(Self {
            _ring_buffer: ring_buffer,
            transport: None,
            optimizations,
        })
    }

    /// Enable UDP transport
    pub fn with_transport(mut self, transport_config: TransportConfig) -> Result<Self> {
        use std::net::SocketAddr;
        use crate::transport::reliable_udp::ReliableUdpConfig;

        // Parse bind address from config
        let bind_addr: SocketAddr = transport_config.local_addr
            .parse()
            .map_err(|_| FluxError::config("Invalid local_addr format"))?;

        // Convert TransportConfig to ReliableUdpConfig
        let reliable_config = ReliableUdpConfig {
            window_size: transport_config.buffer_size,
            retransmit_timeout_ms: transport_config.retransmit_timeout_ms,
            max_retransmissions: transport_config.max_retransmits,
            nak_timeout_ms: 50, // Default NAK timeout
            max_out_of_order: 1000, // Default out-of-order buffer size
            heartbeat_interval_ms: 1000, // Default heartbeat
            session_timeout_ms: 30000, // Default session timeout
        };

        let transport = ReliableUdpTransport::new(bind_addr, reliable_config)?;
        self.transport = Some(transport);
        Ok(self)
    }

    /// Get optimization manager
    pub fn optimizations(&self) -> &OptimizationManager {
        &self.optimizations
    }

    /// Run performance benchmarks
    pub fn benchmark(&self) {
        println!("Flux Performance Benchmark");
        println!("==========================");

        // Run optimization benchmarks
        self.optimizations.run_benchmarks();

        // Run transport benchmarks if available
        if let Some(_transport) = &self.transport {
            println!("\nTransport Performance:");
            println!("Reliable UDP transport configured and ready");
            println!("Use examples for actual performance measurements");
        }
    }
}

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
        if let Some((seq, slots)) = ring_buffer.try_claim_slots(3) {
            for (i, slot) in slots.iter_mut().enumerate() {
                slot.set_sequence(seq + (i as u64));
                slot.set_data(b"Message");
            }
            ring_buffer.publish_batch(seq, 3);
        }

        // Receive batch
        let messages = ring_buffer.try_consume_batch(0, 3);
        assert_eq!(messages.len(), 3);
    }
}

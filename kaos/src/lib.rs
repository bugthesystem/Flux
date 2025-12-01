//! Kaos - Lock-free ring buffers

pub mod constants;
pub mod disruptor;
pub mod error;
pub mod crc32;

// Re-export main components
pub use disruptor::{RingBuffer, MessageRingBuffer, RingBufferConfig, MessageSlot};
pub use error::{Result, KaosError};

#[cfg(test)]
mod tests {
    use crate::disruptor::{MessageRingBuffer, RingBufferConfig, MessageSlot, RingBufferEntry};

    #[test]
    fn test_message_ring_buffer_creation() {
        let config = RingBufferConfig::new(1024).unwrap().with_consumers(1).unwrap();
        let ring_buffer = MessageRingBuffer::new(config);
        assert!(ring_buffer.is_ok());
    }

    #[test]
    fn test_message_slot_operations() {
        let mut slot = MessageSlot::default();
        slot.set_data(b"Hello, Kaos!");
        assert_eq!(slot.data(), b"Hello, Kaos!");
    }

    #[test]
    fn test_batch_operations() {
        let config = RingBufferConfig::new(1024).unwrap().with_consumers(1).unwrap();
        let mut ring_buffer = MessageRingBuffer::new(config).unwrap();

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
        
        // Receive
        let mut total_received = 0;
        loop {
            let messages = ring_buffer.try_consume_batch(0, 3);
            if messages.is_empty() {
                break;
            }
            total_received += messages.len();
        }
        assert_eq!(total_received, claimed);
    }
}

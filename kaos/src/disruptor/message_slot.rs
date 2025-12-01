//! 128-byte aligned message slot with hardware CRC32 on ARM64.

use crate::disruptor::{ RingBufferEntry, Sequence };
use crate::error::{ Result, KaosError };
use crate::constants::MAX_MESSAGE_DATA_SIZE;

/// Message type (only Data is actively used)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[repr(u8)]
pub enum MessageType {
    #[default]
    Data = 0,
}

/// 128-byte aligned to prevent false sharing (Apple Silicon has 128B cache lines).
#[repr(C, align(128))]
#[derive(Clone, Copy)]
pub struct MessageSlot {
    pub sequence: u64,
    pub timestamp: u64,
    pub session_id: u32,
    pub data_len: u32,
    pub checksum: u32,
    pub msg_type: u8,
    pub flags: u8,
    pub _padding: [u8; 6],
    pub data: [u8; MAX_MESSAGE_DATA_SIZE],
}

impl Default for MessageSlot {
    fn default() -> Self {
        unsafe { std::mem::zeroed() }
    }
}

impl MessageSlot {
    pub fn new(data: &[u8]) -> Result<Self> {
        if data.len() > MAX_MESSAGE_DATA_SIZE {
            return Err(
                KaosError::invalid_message(
                    format!("Data too large: {} bytes (max: {})", data.len(), MAX_MESSAGE_DATA_SIZE)
                )
            );
        }
        let mut slot = Self::default();
        slot.set_data(data);
        Ok(slot)
    }

    pub fn set_data(&mut self, data: &[u8]) {
        let data_len = data.len().min(MAX_MESSAGE_DATA_SIZE);
        self.data_len = data_len as u32;

        // Standard Rust copy - benchmarks show it's faster than custom SIMD
        self.data[..data_len].copy_from_slice(&data[..data_len]);

        self.checksum = Self::calculate_checksum_hardware(&self.data[..data_len]);
    }

    pub fn data(&self) -> &[u8] {
        &self.data[..self.data_len as usize]
    }

    #[cfg(test)]
    fn verify_checksum(&self) -> bool {
        self.checksum == Self::calculate_checksum_hardware(self.data())
    }

    /// ARM64: hardware CRC32 instructions.
    #[cfg(target_arch = "aarch64")]
    fn calculate_checksum_hardware(data: &[u8]) -> u32 {
        if data.is_empty() {
            return 0;
        }
        // SAFETY: ARM CRC32 intrinsics on valid slice
        unsafe {
            let mut crc = 0u32;
            let mut ptr = data.as_ptr();
            let end = ptr.add(data.len());
            while ptr.add(8) <= end {
                let chunk = std::ptr::read_unaligned(ptr as *const u64);
                crc = std::arch::aarch64::__crc32cd(crc, chunk);
                ptr = ptr.add(8);
            }
            while ptr < end {
                crc = std::arch::aarch64::__crc32cb(crc, *ptr);
                ptr = ptr.add(1);
            }
            crc
        }
    }

    /// Fallback: xxHash-style checksum for non-ARM.
    #[cfg(not(target_arch = "aarch64"))]
    fn calculate_checksum_hardware(data: &[u8]) -> u32 {
        if data.is_empty() {
            return 0;
        }
        let mut hash = 0x9e3779b1u32;
        for &byte in data {
            hash = hash.wrapping_mul(0x85ebca77).wrapping_add(byte as u32);
            hash ^= hash >> 13;
        }
        hash
    }
}

impl RingBufferEntry for MessageSlot {
    fn sequence(&self) -> Sequence {
        self.sequence
    }

    fn set_sequence(&mut self, seq: Sequence) {
        self.sequence = seq;
    }

    fn reset(&mut self) {
        *self = Self::default();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_alignment() {
        assert_eq!(std::mem::align_of::<MessageSlot>(), 128);
        assert!(std::mem::size_of::<MessageSlot>() % 128 == 0);
    }

    #[test]
    fn test_creation() {
        let slot = MessageSlot::new(b"Hello").unwrap();
        assert_eq!(slot.data(), b"Hello");
        assert!(slot.verify_checksum());
    }

    #[test]
    fn test_too_large() {
        assert!(MessageSlot::new(&vec![0u8; 1025]).is_err());
    }

    #[test]
    fn test_checksum_corruption() {
        let mut slot = MessageSlot::new(b"test").unwrap();
        slot.data[0] ^= 0xff;
        assert!(!slot.verify_checksum());
    }

    #[test]
    fn test_entry_trait() {
        let mut slot = MessageSlot::new(b"test").unwrap();
        slot.set_sequence(42);
        assert_eq!(slot.sequence(), 42);
        slot.reset();
        assert_eq!(slot.sequence(), 0);
    }
}

//! Slot types for ring buffer optimization
//!
//! - `Slot8` (8 bytes): Single u64 value
//! - `Slot16` (16 bytes): Two u64 values  
//! - `Slot32` (32 bytes): Four u64 values
//! - `Slot64` (64 bytes): Cache-line sized
//! - `MessageSlot` (128 bytes): Variable-length messages with CRC32

use crate::disruptor::{RingBufferEntry, Sequence};
use crate::error::{KaosError, Result};
use bytemuck::Zeroable;

/// 8-byte slot
#[repr(C, align(8))]
#[derive(Clone, Copy, Default)]
pub struct Slot8 {
    pub value: u64,
}

impl RingBufferEntry for Slot8 {
    fn sequence(&self) -> u64 {
        self.value
    }

    fn set_sequence(&mut self, seq: u64) {
        self.value = seq;
    }

    fn reset(&mut self) {
        self.value = 0;
    }
}

/// 16-byte slot - Price + Quantity or two u64 values
#[repr(C, align(16))]
#[derive(Clone, Copy, Default)]
pub struct Slot16 {
    pub value1: u64,
    pub value2: u64,
}

impl RingBufferEntry for Slot16 {
    fn sequence(&self) -> u64 {
        self.value1
    }

    fn set_sequence(&mut self, seq: u64) {
        self.value1 = seq;
    }

    fn reset(&mut self) {
        self.value1 = 0;
        self.value2 = 0;
    }
}

/// 32-byte slot - Price + Qty + Timestamp + Symbol (4 x u64)
#[repr(C, align(32))]
#[derive(Clone, Copy, Default)]
pub struct Slot32 {
    pub value1: u64,
    pub value2: u64,
    pub value3: u64,
    pub value4: u64,
}

impl RingBufferEntry for Slot32 {
    fn sequence(&self) -> u64 {
        self.value1
    }

    fn set_sequence(&mut self, seq: u64) {
        self.value1 = seq;
    }

    fn reset(&mut self) {
        self.value1 = 0;
        self.value2 = 0;
        self.value3 = 0;
        self.value4 = 0;
    }
}

/// 64-byte slot - Full cache line, 8 x u64 values
#[repr(C, align(64))]
#[derive(Clone, Copy, Default)]
pub struct Slot64 {
    pub values: [u64; 8],
}

impl RingBufferEntry for Slot64 {
    fn sequence(&self) -> u64 {
        self.values[0]
    }

    fn set_sequence(&mut self, seq: u64) {
        self.values[0] = seq;
    }

    fn reset(&mut self) {
        self.values = [0; 8];
    }
}

// ============================================================================
// MessageSlot - 128-byte aligned with hardware CRC32
// ============================================================================

/// Max payload in MessageSlot (1KB, fits in single cache prefetch)
const MAX_MESSAGE_DATA_SIZE: usize = 1024;
// Note: align(128) used directly for cache-line alignment (128B on Apple Silicon, 64B on x86)

/// Message type (only Data is actively used)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[repr(u8)]
pub enum MessageType {
    #[default]
    Data = 0,
}

/// 128-byte aligned to prevent false sharing (Apple Silicon has 128B cache lines).
/// Note: Cannot derive Pod due to alignment padding, but Zeroable is safe.
#[repr(C, align(128))]
#[derive(Clone, Copy, Zeroable)]
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

/// Safe path uses bytemuck::Zeroable, unsafe path uses mem::zeroed
#[cfg(feature = "unsafe-perf")]
impl Default for MessageSlot {
    fn default() -> Self {
        unsafe { std::mem::zeroed() }
    }
}

#[cfg(not(feature = "unsafe-perf"))]
impl Default for MessageSlot {
    fn default() -> Self {
        Zeroable::zeroed()
    }
}

impl MessageSlot {
    pub fn new(data: &[u8]) -> Result<Self> {
        if data.len() > MAX_MESSAGE_DATA_SIZE {
            return Err(KaosError::invalid_message(format!(
                "Data too large: {} bytes (max: {})",
                data.len(),
                MAX_MESSAGE_DATA_SIZE
            )));
        }
        let mut slot = Self::default();
        slot.set_data(data);
        Ok(slot)
    }

    pub fn set_data(&mut self, data: &[u8]) {
        let data_len = data.len().min(MAX_MESSAGE_DATA_SIZE);
        self.data_len = data_len as u32;
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

    #[cfg(target_arch = "aarch64")]
    fn calculate_checksum_hardware(data: &[u8]) -> u32 {
        if data.is_empty() {
            return 0;
        }
        unsafe {
            let mut crc = 0u32;
            let mut ptr = data.as_ptr();
            let end = ptr.add(data.len());
            while ptr.add(8) <= end {
                crc =
                    std::arch::aarch64::__crc32cd(crc, std::ptr::read_unaligned(ptr as *const u64));
                ptr = ptr.add(8);
            }
            while ptr < end {
                crc = std::arch::aarch64::__crc32cb(crc, *ptr);
                ptr = ptr.add(1);
            }
            crc
        }
    }

    /// Fallback checksum for non-ARM (FNV-1a inspired, fast software hash)
    #[cfg(not(target_arch = "aarch64"))]
    fn calculate_checksum_hardware(data: &[u8]) -> u32 {
        // FNV-1a constants (well-distributed primes)
        const FNV_SEED: u32 = 0x9e3779b1; // Golden ratio derived
        const FNV_PRIME: u32 = 0x85ebca77; // FNV prime for 32-bit

        if data.is_empty() {
            return 0;
        }
        let mut hash = FNV_SEED;
        for &byte in data {
            hash = hash.wrapping_mul(FNV_PRIME).wrapping_add(byte as u32);
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
    fn test_slot_sizes() {
        assert_eq!(std::mem::size_of::<Slot8>(), 8);
        assert_eq!(std::mem::size_of::<Slot16>(), 16);
        assert_eq!(std::mem::size_of::<Slot32>(), 32);
        assert_eq!(std::mem::size_of::<Slot64>(), 64);
    }

    #[test]
    fn test_slot_alignments() {
        assert_eq!(std::mem::align_of::<Slot8>(), 8);
        assert_eq!(std::mem::align_of::<Slot16>(), 16);
        assert_eq!(std::mem::align_of::<Slot32>(), 32);
        assert_eq!(std::mem::align_of::<Slot64>(), 64);
    }

    #[test]
    fn test_slot8_entry() {
        let mut slot = Slot8::default();
        assert_eq!(slot.sequence(), 0);
        slot.set_sequence(42);
        assert_eq!(slot.sequence(), 42);
        slot.reset();
        assert_eq!(slot.sequence(), 0);
    }

    #[test]
    fn test_message_slot_alignment() {
        assert_eq!(std::mem::align_of::<MessageSlot>(), 128);
        assert!(std::mem::size_of::<MessageSlot>() % 128 == 0);
    }

    #[test]
    fn test_message_slot_creation() {
        let slot = MessageSlot::new(b"Hello").unwrap();
        assert_eq!(slot.data(), b"Hello");
        assert!(slot.verify_checksum());
    }

    #[test]
    fn test_message_slot_too_large() {
        assert!(MessageSlot::new(&vec![0u8; 1025]).is_err());
    }

    #[test]
    fn test_message_slot_checksum() {
        let mut slot = MessageSlot::new(b"test").unwrap();
        slot.data[0] ^= 0xff;
        assert!(!slot.verify_checksum());
    }
}

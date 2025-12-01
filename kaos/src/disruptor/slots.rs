//! Slot types for ring buffer optimization
//!
//! - `Slot8` (8 bytes): Single u64 value
//! - `Slot16` (16 bytes): Two u64 values
//! - `Slot32` (32 bytes): Four u64 values
//! - `Slot64` (64 bytes): Cache-line sized, eight u64 values
//! - `MessageSlot` (128 bytes): Variable-length messages (see message_slot module)

use crate::disruptor::RingBufferEntry;

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
}

//! Slot types for ring buffer optimization
//!
//! - `SmallSlot` (8 bytes): Single u64 value
//! - `Slot16` (16 bytes): Two u64 values
//! - `Slot32` (32 bytes): Four u64 values
//! - `Slot64` (64 bytes): Cache-line sized, eight u64 values
//! - `MessageSlot` (128 bytes): Variable-length messages (see message_slot module)

use crate::disruptor::RingBufferEntry;

/// 8-byte slot - minimal overhead
#[repr(C, align(8))]
#[derive(Clone, Copy, Default)]
pub struct SmallSlot {
    pub value: u64,
}

impl RingBufferEntry for SmallSlot {
    #[inline(always)]
    fn sequence(&self) -> u64 {
        self.value
    }
    
    #[inline(always)]
    fn set_sequence(&mut self, seq: u64) {
        self.value = seq;
    }
    
    #[inline(always)]
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
    #[inline(always)]
    fn sequence(&self) -> u64 {
        self.value1
    }
    
    #[inline(always)]
    fn set_sequence(&mut self, seq: u64) {
        self.value1 = seq;
    }
    
    #[inline(always)]
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
    #[inline(always)]
    fn sequence(&self) -> u64 {
        self.value1
    }
    
    #[inline(always)]
    fn set_sequence(&mut self, seq: u64) {
        self.value1 = seq;
    }
    
    #[inline(always)]
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
    #[inline(always)]
    fn sequence(&self) -> u64 {
        self.values[0]
    }
    
    #[inline(always)]
    fn set_sequence(&mut self, seq: u64) {
        self.values[0] = seq;
    }
    
    #[inline(always)]
    fn reset(&mut self) {
        self.values = [0; 8];
    }
}

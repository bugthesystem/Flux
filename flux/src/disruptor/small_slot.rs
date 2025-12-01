//! SmallSlot - Minimal 8-byte slot for maximum throughput

use crate::disruptor::RingBufferEntry;

/// Minimal 8-byte slot for maximum throughput
///
/// Use when you need maximum throughput with fixed-size u64 data.
#[repr(C, align(8))]
#[derive(Clone, Copy, Default)]
pub struct SmallSlot {
    /// Sequence number
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

impl SmallSlot {
    /// Create a new SmallSlot with the given value
    #[inline(always)]
    pub fn new(value: u64) -> Self {
        Self { value }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_small_slot_size() {
        assert_eq!(std::mem::size_of::<SmallSlot>(), 8);
        assert_eq!(std::mem::align_of::<SmallSlot>(), 8);
    }

    #[test]
    fn test_ring_buffer_entry() {
        let mut slot = SmallSlot::default();
        assert_eq!(slot.sequence(), 0);

        slot.set_sequence(42);
        assert_eq!(slot.sequence(), 42);
        assert_eq!(slot.value, 42);

        slot.reset();
        assert_eq!(slot.sequence(), 0);
    }
}

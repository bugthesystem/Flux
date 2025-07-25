//! This module provides a `ReliableWindowRingBuffer`, a specialized ring buffer for managing the receive window in a reliable UDP protocol.
//!
//! It's designed for high-performance scenarios where packets might arrive out of order.
//! The `HybridWindow` combines this ring buffer with a `BTreeMap` to handle packets that are far ahead of the current sequence, providing a more robust solution for varying network conditions.

use std::collections::BTreeMap;
use std::sync::atomic::Ordering;
use crate::transport::reliable_udp::DEBUG_NAK;

const MAX_PACKET_SIZE: usize = 2048;

#[repr(C, align(128))]
#[derive(Clone, Copy)]
pub struct ReliableWindowSlot {
    pub seq: u64,
    pub valid: bool,
    pub data: [u8; MAX_PACKET_SIZE],
    pub len: usize,
}

pub struct ReliableWindowRingBuffer {
    pub slots: Vec<ReliableWindowSlot>,
    pub next_expected_seq: u64,
    pub window_size: usize,
}

impl ReliableWindowRingBuffer {
    pub fn new(window_size: usize, start_seq: u64) -> Self {
        Self {
            slots: vec![ReliableWindowSlot {
                seq: 0,
                valid: false,
                data: [0u8; MAX_PACKET_SIZE],
                len: 0,
            }; window_size],
            next_expected_seq: start_seq,
            window_size,
        }
    }

    pub fn insert(&mut self, seq: u64, data: &[u8]) -> bool {
        if
            seq < self.next_expected_seq ||
            seq >= self.next_expected_seq + (self.window_size as u64)
        {
            // Out of window, drop
            return false;
        }
        let idx = (seq % (self.window_size as u64)) as usize;
        let slot = &mut self.slots[idx];
        // If the slot is valid, it means it's either a duplicate or occupied by an undelivered packet
        if slot.valid {
            if slot.seq == seq {
                // Duplicate insert for same seq, ignore
                return false;
            } else {
                // Slot occupied by undelivered packet, drop
                return false;
            }
        }
        slot.seq = seq;
        slot.len = data.len().min(MAX_PACKET_SIZE);
        slot.data[..slot.len].copy_from_slice(&data[..slot.len]);
        slot.valid = true;
        true
    }

    /// Returns indices of in-order deliverable slots. Caller must copy data and mark slots as invalid.
    /// NOTE: This method is currently not used in favor of the more efficient `deliver_in_order_with`.
    #[allow(dead_code)]
    pub fn deliver_in_order(&self) -> Vec<usize> {
        let mut delivered = Vec::new();
        let mut seq = self.next_expected_seq;
        loop {
            let idx = (seq % (self.window_size as u64)) as usize;
            let slot = &self.slots[idx];
            if slot.valid && slot.seq == seq {
                delivered.push(idx);
                seq += 1;
            } else {
                break;
            }
        }
        delivered
    }

    pub fn deliver_in_order_with<F: FnMut(&[u8])>(&mut self, mut f: F) {
        loop {
            let idx = (self.next_expected_seq % (self.window_size as u64)) as usize;
            let slot = &mut self.slots[idx];
            if slot.valid && slot.seq == self.next_expected_seq {
                f(&slot.data[..slot.len]);
                slot.valid = false;
                let prev_seq = self.next_expected_seq;
                self.next_expected_seq += 1;
                if self.next_expected_seq % (self.window_size as u64) == 0 {
                    debug_nak!("[WINDOW] Wraparound at seq {}", prev_seq);
                }
            } else {
                break;
            }
        }
    }

    /// Scan for missing ranges and send batch NAKs for gaps in the window
    pub fn send_batch_naks_for_gaps<T: FnMut(u64, u64)>(&self, mut send_nak: T) {
        let mut seq = self.next_expected_seq;
        let end_seq = self.next_expected_seq + (self.window_size as u64);
        let mut missing_start = None;
        while seq < end_seq {
            let idx = (seq % (self.window_size as u64)) as usize;
            let slot = &self.slots[idx];
            if !slot.valid || slot.seq != seq {
                if missing_start.is_none() {
                    missing_start = Some(seq);
                }
            } else if let Some(start) = missing_start {
                send_nak(start, seq - 1);
                missing_start = None;
            }
            seq += 1;
        }
        if let Some(start) = missing_start {
            send_nak(start, end_seq - 1);
        }
    }
}

/// A hybrid receive window that combines a ring buffer for the primary window and a BTreeMap for future packets.
/// This allows for efficient in-order processing while still handling packets that arrive far out of order.
pub struct HybridWindow {
    pub ring: ReliableWindowRingBuffer,
    pub map: BTreeMap<u64, Vec<u8>>,
}

impl HybridWindow {
    /// Creates a new `HybridWindow` with the given window size and starting sequence number.
    pub fn new(window_size: usize, start_seq: u64) -> Self {
        Self {
            ring: ReliableWindowRingBuffer::new(window_size, start_seq),
            map: BTreeMap::new(),
        }
    }

    /// Inserts a packet into the hybrid window.
    /// If the packet falls within the ring buffer's window, it's inserted there.
    /// Otherwise, it's stored in the BTreeMap for later processing.
    pub fn insert(&mut self, seq: u64, data: &[u8]) {
        if
            seq >= self.ring.next_expected_seq &&
            seq < self.ring.next_expected_seq + (self.ring.window_size as u64)
        {
            self.ring.insert(seq, data);
        } else {
            self.map.insert(seq, data.to_vec());
        }
    }

    /// Delivers in-order packets to the provided closure.
    /// It first delivers packets from the ring buffer, and then checks the BTreeMap to see if any stored packets can now be processed.
    pub fn deliver_in_order_with<F: FnMut(&[u8])>(&mut self, mut f: F) {
        self.ring.deliver_in_order_with(|msg| { f(msg) });
        // After each delivery, check the map for the next expected seq
        while let Some(data) = self.map.remove(&self.ring.next_expected_seq) {
            self.ring.insert(self.ring.next_expected_seq, &data);
            self.ring.deliver_in_order_with(|msg| { f(msg) });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn in_order_delivery() {
        let mut win = ReliableWindowRingBuffer::new(8, 0);
        for i in 0..4 {
            assert!(win.insert(i, &[i as u8]));
        }
        let mut delivered = Vec::new();
        win.deliver_in_order_with(|msg| delivered.push(msg[0]));
        assert_eq!(delivered, vec![0, 1, 2, 3]);
    }

    #[test]
    fn out_of_order_delivery() {
        let mut win = ReliableWindowRingBuffer::new(8, 0);
        assert!(win.insert(1, &[1]));
        assert!(win.insert(2, &[2]));
        assert!(win.insert(0, &[0]));
        let mut delivered = Vec::new();
        win.deliver_in_order_with(|msg| delivered.push(msg[0]));
        assert_eq!(delivered, vec![0, 1, 2]);
    }

    #[test]
    fn missing_then_fill_gap() {
        let mut win = ReliableWindowRingBuffer::new(8, 0);
        assert!(win.insert(0, &[0]));
        assert!(win.insert(2, &[2]));
        assert!(win.insert(1, &[1]));
        let mut delivered = Vec::new();
        win.deliver_in_order_with(|msg| delivered.push(msg[0]));
        assert_eq!(delivered, vec![0, 1, 2]);
    }

    #[test]
    fn duplicate_insertion() {
        let mut win = ReliableWindowRingBuffer::new(8, 0);
        assert!(win.insert(0, &[42]));
        assert!(!win.insert(0, &[99])); // duplicate now returns false
        let mut delivered = Vec::new();
        win.deliver_in_order_with(|msg| delivered.push(msg[0]));
        assert_eq!(delivered, vec![42]);
    }

    #[test]
    fn window_wraparound() {
        let mut win = ReliableWindowRingBuffer::new(4, 0);
        for i in 0..8 {
            assert!(win.insert(i, &[i as u8]));
            let mut delivered = Vec::new();
            win.deliver_in_order_with(|msg| delivered.push(msg[0]));
            // Only in-order up to i
        }
        // After all, window should have delivered 0..8
        let mut win = ReliableWindowRingBuffer::new(4, 0);
        for i in 0..4 {
            assert!(win.insert(i, &[i as u8]));
        }
        let mut delivered = Vec::new();
        win.deliver_in_order_with(|msg| delivered.push(msg[0]));
        assert_eq!(delivered, vec![0, 1, 2, 3]);
        for i in 4..8 {
            assert!(win.insert(i, &[i as u8]));
        }
        let mut delivered2 = Vec::new();
        win.deliver_in_order_with(|msg| delivered2.push(msg[0]));
        assert_eq!(delivered2, vec![4, 5, 6, 7]);
    }
}

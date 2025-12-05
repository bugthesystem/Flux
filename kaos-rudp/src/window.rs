//! Receive window for reliable UDP protocol.
//!
//! This module provides a `BitmapWindow` for managing the receive window,
//! handling out-of-order packet arrival efficiently using a bitmap-based approach.

/// Max packet size (2KB > typical MTU 1500, with headroom for headers)
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
            slots: vec![
                ReliableWindowSlot {
                    seq: 0,
                    valid: false,
                    data: [0u8; MAX_PACKET_SIZE],
                    len: 0,
                };
                window_size
            ],
            next_expected_seq: start_seq,
            window_size,
        }
    }

    pub fn insert(&mut self, seq: u64, data: &[u8]) -> bool {
        if seq < self.next_expected_seq || seq >= self.next_expected_seq + (self.window_size as u64)
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

    pub fn deliver_in_order_with<F: FnMut(&[u8])>(&mut self, mut f: F) {
        loop {
            let idx = (self.next_expected_seq % (self.window_size as u64)) as usize;
            let slot = &mut self.slots[idx];
            if slot.valid && slot.seq == self.next_expected_seq {
                f(&slot.data[..slot.len]);
                slot.valid = false;
                let _prev_seq = self.next_expected_seq;
                self.next_expected_seq += 1;
                if self.next_expected_seq % (self.window_size as u64) == 0 {}
            } else {
                break;
            }
        }
    }

    /// Scan for missing ranges and send batch NAKs for gaps in the window
    pub fn send_batch_naks_for_gaps<T: FnMut(u64, u64)>(&self, mut send_nak: T) {
        let mut highest_seq = self.next_expected_seq;
        for i in 0..self.window_size {
            let slot = &self.slots[i];
            if slot.valid && slot.seq > highest_seq {
                highest_seq = slot.seq;
            }
        }

        if highest_seq <= self.next_expected_seq {
            return;
        }

        let reasonable_lookahead = 32;
        let end_seq = (highest_seq + reasonable_lookahead)
            .min(self.next_expected_seq + (self.window_size as u64));

        let mut seq = self.next_expected_seq;
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

/// A bitmap-based receive window that tracks received packets efficiently.
/// Uses a bitmap for O(1) lookup and only stores packets within a reasonable future window.
#[repr(C, align(128))] // MEMORY-OPTIMIZED: Cache-line align the entire struct
pub struct BitmapWindow {
    pub ring: ReliableWindowRingBuffer,
    /// Bitmap tracking received packets (1 = received, 0 = not received)
    /// Covers a window of 64 * bitmap_size sequence numbers
    /// MEMORY-OPTIMIZED: Use Box<[u64]> for better cache locality
    bitmap: Box<[u64]>,
    /// Base sequence number for the bitmap (aligned to 64-bit boundaries)
    bitmap_base: u64,
    /// Maximum number of future packets to store
    max_future_packets: usize,
    /// Storage for future packets (only within max_future_packets range)
    /// MEMORY-OPTIMIZED: Pre-allocate with capacity for better performance
    future_packets: Vec<(u64, Vec<u8>)>,
}

impl BitmapWindow {
    /// Creates a new `BitmapWindow` with the given window size and starting sequence number.
    pub fn new(window_size: usize, start_seq: u64) -> Self {
        // Bitmap covers 64 * 32 = 2048 sequence numbers (enough for most use cases)
        let bitmap_size = 32;
        Self {
            ring: ReliableWindowRingBuffer::new(window_size, start_seq),
            // MEMORY-OPTIMIZED: Use Box<[u64]> for better cache locality
            bitmap: vec![0u64; bitmap_size].into_boxed_slice(),
            bitmap_base: (start_seq >> 6) << 6, // Align to 64-bit boundary using bitwise ops
            max_future_packets: window_size * 2, // Store up to 2x window size future packets
            // MEMORY-OPTIMIZED: Pre-allocate with capacity for better performance
            future_packets: Vec::with_capacity(window_size * 2),
        }
    }

    /// Sets a bit in the bitmap for the given sequence number
    fn set_bit(&mut self, seq: u64) {
        if seq < self.bitmap_base {
            return; // Too old, ignore
        }
        // BITMAP-OPTIMIZED: Use bitwise operations instead of division/modulo
        let relative_seq = seq - self.bitmap_base;
        let bitmap_index = (relative_seq >> 6) as usize; // Divide by 64 using bit shift
        if bitmap_index >= self.bitmap.len() {
            return; // Too far in future, ignore
        }
        let bit_offset = relative_seq & 63; // Modulo 64 using bitwise AND
        self.bitmap[bitmap_index] |= 1u64 << bit_offset;
    }

    /// Advances the bitmap base when the ring buffer advances significantly
    fn advance_bitmap_if_needed(&mut self) {
        // BITMAP-OPTIMIZED: Use bitwise operations for alignment
        let new_base = (self.ring.next_expected_seq >> 6) << 6; // Align to 64-bit boundary
        if new_base > self.bitmap_base + 64 {
            // Shift bitmap left by the difference
            let shift_amount = ((new_base - self.bitmap_base) >> 6) as usize; // Divide by 64 using bit shift
            if shift_amount >= self.bitmap.len() {
                // Reset bitmap if we've advanced too far
                self.bitmap.fill(0);
                self.bitmap_base = new_base;
            } else {
                // Shift bitmap
                for i in 0..self.bitmap.len() - shift_amount {
                    self.bitmap[i] = self.bitmap[i + shift_amount];
                }
                for i in self.bitmap.len() - shift_amount..self.bitmap.len() {
                    self.bitmap[i] = 0;
                }
                self.bitmap_base = new_base;
            }
        }
    }

    /// Inserts a packet into the bitmap window.
    /// If the packet falls within the ring buffer's window, it's inserted there.
    /// Otherwise, it's stored in future_packets if within the reasonable future window.
    pub fn insert(&mut self, seq: u64, data: &[u8]) {
        // Set the bit to mark this sequence as received
        self.set_bit(seq);

        if seq >= self.ring.next_expected_seq
            && seq < self.ring.next_expected_seq + (self.ring.window_size as u64)
        {
            // Within ring buffer window
            self.ring.insert(seq, data);
        } else if seq >= self.ring.next_expected_seq
            && seq < self.ring.next_expected_seq + (self.max_future_packets as u64)
        {
            // Within reasonable future window, store for later
            // Check if we already have this packet
            if !self.future_packets.iter().any(|(s, _)| *s == seq) {
                self.future_packets.push((seq, data.to_vec()));
                // Keep sorted by sequence number for efficient processing
                self.future_packets.sort_by_key(|(s, _)| *s);
            }
        }
        // If packet is too far in future, just mark it as received in bitmap
    }

    /// Delivers in-order packets to the provided closure.
    /// Processes ring buffer first, then checks future packets.
    pub fn deliver_in_order_with<F: FnMut(&[u8])>(&mut self, mut f: F) {
        self.ring.deliver_in_order_with(|msg| f(msg));

        // Check if any future packets can now be moved to ring buffer
        let mut i = 0;
        while i < self.future_packets.len() {
            let (seq, data) = &self.future_packets[i];
            if *seq == self.ring.next_expected_seq {
                // This packet can now be processed
                self.ring.insert(*seq, data);
                self.future_packets.remove(i);
                // Continue processing ring buffer
                self.ring.deliver_in_order_with(|msg| f(msg));
            } else if *seq < self.ring.next_expected_seq {
                // This packet is too old, remove it
                self.future_packets.remove(i);
            } else {
                // This packet is still in the future
                i += 1;
            }
        }

        // Advance bitmap if needed
        self.advance_bitmap_if_needed();
    }

    /// Sends NAKs for missing packets in the window
    pub fn send_batch_naks_for_gaps<T: FnMut(u64, u64)>(&self, mut send_nak: T) {
        self.ring.send_batch_naks_for_gaps(&mut send_nak);
    }

    /// Returns the highest sequence number that has been delivered in-order.
    /// This can be used to send ACKs to the sender.
    pub fn last_delivered_seq(&self) -> u64 {
        if self.ring.next_expected_seq == 0 {
            0
        } else {
            self.ring.next_expected_seq - 1
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

    #[test]
    fn bitmap_in_order_delivery() {
        let mut win = BitmapWindow::new(8, 0);
        for i in 0..4 {
            win.insert(i, &[i as u8]);
        }
        let mut delivered = Vec::new();
        win.deliver_in_order_with(|msg| delivered.push(msg[0]));
        assert_eq!(delivered, vec![0, 1, 2, 3]);
    }

    #[test]
    fn bitmap_out_of_order_delivery() {
        let mut win = BitmapWindow::new(8, 0);
        win.insert(1, &[1]);
        win.insert(2, &[2]);
        win.insert(0, &[0]);
        let mut delivered = Vec::new();
        win.deliver_in_order_with(|msg| delivered.push(msg[0]));
        assert_eq!(delivered, vec![0, 1, 2]);
    }

    #[test]
    fn bitmap_missing_then_fill_gap() {
        let mut win = BitmapWindow::new(8, 0);
        win.insert(0, &[0]);
        win.insert(2, &[2]);
        win.insert(1, &[1]);
        let mut delivered = Vec::new();
        win.deliver_in_order_with(|msg| delivered.push(msg[0]));
        assert_eq!(delivered, vec![0, 1, 2]);
    }

    #[test]
    fn bitmap_duplicate_insertion() {
        let mut win = BitmapWindow::new(8, 0);
        win.insert(0, &[42]);
        win.insert(0, &[99]); // duplicate, should not overwrite
        let mut delivered = Vec::new();
        win.deliver_in_order_with(|msg| delivered.push(msg[0]));
        assert_eq!(delivered, vec![42]);
    }

    #[test]
    fn bitmap_window_wraparound() {
        let mut win = BitmapWindow::new(4, 0);
        for i in 0..8 {
            win.insert(i, &[i as u8]);
            let mut delivered = Vec::new();
            win.deliver_in_order_with(|msg| delivered.push(msg[0]));
            // Only in-order up to i
        }
        // After all, window should have delivered 0..8
        let mut win = BitmapWindow::new(4, 0);
        for i in 0..4 {
            win.insert(i, &[i as u8]);
        }
        let mut delivered = Vec::new();
        win.deliver_in_order_with(|msg| delivered.push(msg[0]));
        assert_eq!(delivered, vec![0, 1, 2, 3]);
        // Now wrap window
        for i in 4..8 {
            win.insert(i, &[i as u8]);
        }
        let mut delivered2 = Vec::new();
        win.deliver_in_order_with(|msg| delivered2.push(msg[0]));
        assert_eq!(delivered2, vec![4, 5, 6, 7]);
    }

    #[test]
    fn bitmap_bounded_future_packets() {
        let mut win = BitmapWindow::new(4, 0);
        // Insert far-future packets (should not be stored)
        win.insert(100, &[100]);
        win.insert(101, &[101]);
        // Insert within future window
        win.insert(4, &[4]);
        win.insert(5, &[5]);

        // First, deliver packets 0-3 (which should be empty since we didn't insert them)
        let mut delivered = Vec::new();
        win.deliver_in_order_with(|msg| delivered.push(msg[0]));
        assert_eq!(delivered, vec![]);

        // Now advance the window by inserting and delivering packets 0-3
        for i in 0..4 {
            win.insert(i, &[i as u8]);
        }
        let mut delivered2 = Vec::new();
        win.deliver_in_order_with(|msg| delivered2.push(msg[0]));
        // Since packets 4 and 5 are already in future_packets, they get delivered immediately
        assert_eq!(delivered2, vec![0, 1, 2, 3, 4, 5]);

        // No more packets should be delivered
        let mut delivered3 = Vec::new();
        win.deliver_in_order_with(|msg| delivered3.push(msg[0]));
        assert_eq!(delivered3, vec![]);

        // Far-future packets (100, 101) should not be delivered
        let mut delivered4 = Vec::new();
        win.deliver_in_order_with(|msg| delivered4.push(msg[0]));
        assert_eq!(delivered4, vec![]);
    }
}

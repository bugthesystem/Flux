// Reliable window ring buffer for reliable UDP receive windows

use std::collections::VecDeque;
use std::collections::BTreeMap;

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
    valid_indices: VecDeque<usize>,
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
            valid_indices: VecDeque::with_capacity(window_size),
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
        let was_invalid = !slot.valid;
        if slot.valid && slot.seq != seq {
            // Slot occupied by undelivered packet, drop
            return false;
        }
        slot.seq = seq;
        slot.len = data.len().min(MAX_PACKET_SIZE);
        slot.data[..slot.len].copy_from_slice(&data[..slot.len]);
        slot.valid = true;
        if was_invalid {
            self.valid_indices.push_back(idx);
        }
        true
    }

    /// Returns indices of in-order deliverable slots. Caller must copy data and mark slots as invalid.
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

    /// Zero-copy: Call the provided closure with each in-order message (&[u8]), then mark slot as invalid.
    pub fn deliver_in_order_with<F: FnMut(&[u8])>(&mut self, mut f: F) {
        while let Some(&idx) = self.valid_indices.front() {
            let slot = &mut self.slots[idx];
            if slot.valid && slot.seq == self.next_expected_seq {
                f(&slot.data[..slot.len]);
                slot.valid = false;
                self.next_expected_seq += 1;
                self.valid_indices.pop_front();
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

pub struct HybridWindow {
    pub ring: ReliableWindowRingBuffer,
    pub map: BTreeMap<u64, Vec<u8>>,
}

impl HybridWindow {
    pub fn new(window_size: usize, start_seq: u64) -> Self {
        Self {
            ring: ReliableWindowRingBuffer::new(window_size, start_seq),
            map: BTreeMap::new(),
        }
    }
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
    pub fn deliver_in_order_with<F: FnMut(&[u8])>(&mut self, mut f: F) {
        self.ring.deliver_in_order_with(|msg| { f(msg) });
        // After each delivery, check the map for the next expected seq
        while let Some(data) = self.map.remove(&self.ring.next_expected_seq) {
            self.ring.insert(self.ring.next_expected_seq, &data);
            self.ring.deliver_in_order_with(|msg| { f(msg) });
        }
    }
}

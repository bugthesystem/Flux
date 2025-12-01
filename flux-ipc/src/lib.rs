//! # flux-ipc
//!
//! High-performance inter-process communication using flux's shared memory ring buffers.
//!
//! This is a thin wrapper around `flux::disruptor::SharedRingBuffer`, providing
//! a simplified API for IPC use cases.
//!
//! ## Architecture
//!
//! ```text
//! Process A (Publisher)          Shared Memory          Process B (Subscriber)
//! ┌─────────────────┐      ┌─────────────────────┐      ┌─────────────────┐
//! │   Application   │      │  flux SharedRingBuf │      │   Application   │
//! │       │         │      │  (mmap-backed)      │      │       │         │
//! │       ▼         │      │  ┌─┬─┬─┬─┬─┬─┬─┬─┐  │      │       ▲         │
//! │   Publisher ────┼──────┼──► │ │ │█│█│ │ │ │──┼──────┼── Subscriber    │
//! │                 │      │  └─┴─┴─┴─┴─┴─┴─┴─┘  │      │                 │
//! └─────────────────┘      └─────────────────────┘      └─────────────────┘
//! ```
//!
//! ## Performance Target: 20M+ msgs/sec

use std::path::Path;
use std::io;

// Re-export from flux core
pub use flux::disruptor::{ SharedRingBuffer, SmallSlot, Slot16, Slot32, Slot64, MessageSlot };

/// Default IPC slot - 128 bytes for variable messages
pub type IpcSlot = MessageSlot;

/// Publisher for inter-process communication.
///
/// Wraps `SharedRingBuffer` with a simpler byte-oriented API.
pub struct Publisher<T: flux::disruptor::RingBufferEntry = IpcSlot> {
    inner: SharedRingBuffer<T>,
}

impl<T: flux::disruptor::RingBufferEntry> Publisher<T> {
    /// Create a new publisher, creating the shared memory file.
    ///
    /// # Arguments
    /// * `path` - Path to shared memory file (e.g., "/tmp/my-channel")
    /// * `capacity` - Ring buffer size (must be power of 2)
    pub fn new<P: AsRef<Path>>(path: P, capacity: usize) -> io::Result<Self> {
        let inner = SharedRingBuffer::create(path, capacity)?;
        Ok(Self { inner })
    }

    /// Send raw bytes (copied into a slot).
    pub fn send(&mut self, data: &[u8]) -> io::Result<u64> {
        self.inner.try_send(data)
    }

    /// Try to claim a slot for zero-copy write.
    pub fn try_claim(&mut self) -> Option<u64> {
        self.inner.try_claim()
    }

    /// Write to a claimed slot.
    ///
    /// # Safety
    /// Must have claimed the sequence first.
    pub unsafe fn write_slot(&mut self, seq: u64, value: T) {
        self.inner.write_slot(seq, value);
    }

    /// Publish slots up to sequence.
    pub fn publish(&mut self, seq: u64) {
        self.inner.publish(seq);
    }
}

/// Subscriber for inter-process communication.
///
/// Wraps `SharedRingBuffer` for the consumer side.
pub struct Subscriber<T: flux::disruptor::RingBufferEntry = IpcSlot> {
    inner: SharedRingBuffer<T>,
}

impl<T: flux::disruptor::RingBufferEntry> Subscriber<T> {
    /// Connect to an existing shared memory channel.
    pub fn new<P: AsRef<Path>>(path: P) -> io::Result<Self> {
        let inner = SharedRingBuffer::open(path)?;
        Ok(Self { inner })
    }

    /// Process available messages with a callback (zero-copy).
    pub fn receive<F: FnMut(&T)>(&mut self, callback: F) -> usize {
        self.inner.receive(callback)
    }

    /// Get number of available messages.
    pub fn available(&mut self) -> u64 {
        self.inner.available()
    }

    /// Read a single slot.
    ///
    /// # Safety
    /// Caller must ensure slot is published.
    pub unsafe fn read_slot(&self, seq: u64) -> T {
        self.inner.read_slot(seq)
    }

    /// Advance consumer after reading.
    pub fn advance(&mut self, seq: u64) {
        self.inner.advance_consumer(seq);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use flux::disruptor::SmallSlot;
    use std::fs;

    #[test]
    fn test_publisher_subscriber() {
        let path = "/tmp/flux-ipc-wrapper-test";
        let _ = fs::remove_file(path);

        let mut publisher = Publisher::<SmallSlot>::new(path, 1024).unwrap();
        let mut subscriber = Subscriber::<SmallSlot>::new(path).unwrap();

        // Send
        publisher.send(&(42u64).to_le_bytes()).unwrap();

        // Receive
        let mut received = 0u64;
        subscriber.receive(|slot| {
            received = slot.value;
        });

        assert_eq!(received, 42);

        let _ = fs::remove_file(path);
    }

    #[test]
    fn test_high_throughput() {
        let path = "/tmp/flux-ipc-throughput-test";
        let _ = fs::remove_file(path);

        let mut publisher = Publisher::<SmallSlot>::new(path, 64 * 1024).unwrap();
        let mut subscriber = Subscriber::<SmallSlot>::new(path).unwrap();

        const COUNT: u64 = 10_000;

        // Interleaved send/receive to avoid buffer full
        let mut sent = 0u64;
        let mut received = 0u64;

        while received < COUNT {
            // Send batch
            while sent < COUNT && sent - received < 60_000 {
                if publisher.send(&sent.to_le_bytes()).is_ok() {
                    sent += 1;
                } else {
                    break;
                }
            }

            // Receive batch
            received += subscriber.receive(|_| {}) as u64;
        }

        assert_eq!(received, COUNT);

        let _ = fs::remove_file(path);
    }
}

//! kaos-ipc - High-performance inter-process communication via shared memory.
//!
//! Zero-copy reads via mmap. Writes copy data to shared memory.
//!
//! ```rust,no_run
//! use kaos_ipc::{Publisher, Subscriber};
//!
//! // Process A
//! let mut pub_ = Publisher::create("/tmp/ipc", 1024).unwrap();
//! pub_.send(42u64).unwrap();
//!
//! // Process B  
//! let mut sub = Subscriber::open("/tmp/ipc").unwrap();
//! while let Some(val) = sub.try_receive() {
//!     println!("Got: {}", val);
//! }
//! ```

use kaos::disruptor::{SharedRingBuffer, Slot8};
use kaos::{record_receive, record_send};
use std::io;
use std::path::Path;

/// Publisher (producer) - creates the shared memory file
pub struct Publisher {
    inner: SharedRingBuffer<Slot8>,
}

impl Publisher {
    pub fn create<P: AsRef<Path>>(path: P, capacity: usize) -> io::Result<Self> {
        Ok(Self {
            inner: SharedRingBuffer::create(path, capacity)?,
        })
    }

    /// Send a u64 value (returns sequence number)
    pub fn send(&mut self, value: u64) -> io::Result<u64> {
        let result = self.inner.try_send(&value.to_le_bytes());
        if result.is_ok() {
            record_send(8);
        }
        result
    }

    /// Try to send (non-blocking, returns None if full)
    pub fn try_send(&mut self, data: &[u8]) -> Option<u64> {
        let result = self.inner.try_send(data).ok();
        if result.is_some() {
            record_send(data.len() as u64);
        }
        result
    }
}

/// Subscriber (consumer) - opens existing shared memory file
pub struct Subscriber {
    inner: SharedRingBuffer<Slot8>,
}

impl Subscriber {
    pub fn open<P: AsRef<Path>>(path: P) -> io::Result<Self> {
        Ok(Self {
            inner: SharedRingBuffer::open(path)?,
        })
    }

    /// Try to receive one message (non-blocking, for polling)
    pub fn try_receive(&mut self) -> Option<u64> {
        let result = self.inner.try_receive().map(|slot| slot.value);
        if result.is_some() {
            record_receive(8);
        }
        result
    }

    /// Receive all available messages via callback (batch, faster)
    pub fn receive<F: FnMut(u64)>(&mut self, mut f: F) -> usize {
        let count = self.inner.receive(|slot| f(slot.value));
        if count > 0 {
            record_receive((count * 8) as u64);
        }
        count
    }

    /// Number of messages available
    pub fn available(&mut self) -> u64 {
        self.inner.available()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn test_send_receive() {
        let path = "/tmp/kaos-ipc-test-simple";
        let _ = fs::remove_file(path);

        let mut pub_ = Publisher::create(path, 1024).unwrap();
        let mut sub = Subscriber::open(path).unwrap();

        pub_.send(42).unwrap();
        assert_eq!(sub.try_receive(), Some(42));

        let _ = fs::remove_file(path);
    }

    #[test]
    fn test_throughput() {
        let path = "/tmp/kaos-ipc-test-throughput";
        let _ = fs::remove_file(path);

        let mut pub_ = Publisher::create(path, 64 * 1024).unwrap();
        let mut sub = Subscriber::open(path).unwrap();

        const N: u64 = 10_000;
        let mut sent = 0u64;
        let mut sum = 0u64;
        let mut count = 0u64;

        // Interleave send/receive to avoid deadlock (single thread)
        while count < N {
            // Send batch
            while sent < N && pub_.send(sent).is_ok() {
                sent += 1;
            }
            // Receive batch
            while let Some(val) = sub.try_receive() {
                sum += val;
                count += 1;
            }
        }

        assert_eq!(count, N);
        assert_eq!(sum, (0..N).sum::<u64>());
        let _ = fs::remove_file(path);
    }
}

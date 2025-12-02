//! # kaos-ipc - Inter-process communication using kaos shared memory ring buffers.

use std::path::Path;
use std::io;

pub use kaos::disruptor::{SharedRingBuffer, Slot8, Slot16, Slot32, Slot64, MessageSlot};

pub type IpcSlot = MessageSlot;

pub struct Publisher<T: kaos::disruptor::RingBufferEntry = IpcSlot> {
    inner: SharedRingBuffer<T>,
}

impl<T: kaos::disruptor::RingBufferEntry> Publisher<T> {
    pub fn new<P: AsRef<Path>>(path: P, capacity: usize) -> io::Result<Self> {
        Ok(Self { inner: SharedRingBuffer::create(path, capacity)? })
    }

    pub fn send(&mut self, data: &[u8]) -> io::Result<u64> { self.inner.try_send(data) }
}

pub struct Subscriber<T: kaos::disruptor::RingBufferEntry = IpcSlot> {
    inner: SharedRingBuffer<T>,
}

impl<T: kaos::disruptor::RingBufferEntry> Subscriber<T> {
    pub fn new<P: AsRef<Path>>(path: P) -> io::Result<Self> {
        Ok(Self { inner: SharedRingBuffer::open(path)? })
    }

    pub fn receive<F: FnMut(&T)>(&mut self, callback: F) -> usize { self.inner.receive(callback) }
    pub fn available(&mut self) -> u64 { self.inner.available() }
}

#[cfg(test)]
mod tests {
    use super::*;
    use kaos::disruptor::Slot8;
    use std::fs;

    #[test]
    fn test_publisher_subscriber() {
        let path = "/tmp/kaos-ipc-test";
        let _ = fs::remove_file(path);
        let mut publisher = Publisher::<Slot8>::new(path, 1024).unwrap();
        let mut subscriber = Subscriber::<Slot8>::new(path).unwrap();
        publisher.send(&(42u64).to_le_bytes()).unwrap();
        let mut received = 0u64;
        subscriber.receive(|slot| { received = slot.value; });
        assert_eq!(received, 42);
        let _ = fs::remove_file(path);
    }

    #[test]
    fn test_high_throughput() {
        let path = "/tmp/kaos-ipc-throughput";
        let _ = fs::remove_file(path);
        let mut publisher = Publisher::<Slot8>::new(path, 64 * 1024).unwrap();
        let mut subscriber = Subscriber::<Slot8>::new(path).unwrap();
        const COUNT: u64 = 10_000;
        let mut sent = 0u64;
        let mut received = 0u64;
        while received < COUNT {
            while sent < COUNT && sent.saturating_sub(received) < 60_000 {
                if publisher.send(&sent.to_le_bytes()).is_ok() { sent += 1; } else { break; }
            }
            received += subscriber.receive(|_| {}) as u64;
        }
        assert_eq!(received, COUNT);
        let _ = fs::remove_file(path);
    }
}

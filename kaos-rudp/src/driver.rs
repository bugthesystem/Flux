//! Media driver backend for kaos-rudp
//!
//! Uses shared memory (kaos-ipc) instead of direct syscalls.
//! Apps get zero-syscall messaging, driver handles network I/O.

use std::io;
use kaos_ipc::{Publisher, Subscriber};

/// Default IPC path for app → driver messages
const SEND_PATH: &str = "/tmp/kaos-send";
/// Default IPC path for driver → app messages
const RECV_PATH: &str = "/tmp/kaos-recv";
/// IPC ring buffer size (64K slots, must be power of 2)
const RING_SIZE: usize = 64 * 1024;

/// Driver-backed transport (zero syscalls from app)
pub struct DriverTransport {
    to_driver: Publisher,
    from_driver: Subscriber,
    next_seq: u64,
}

impl DriverTransport {
    /// Create transport (requires kaos-driver running)
    pub fn new() -> io::Result<Self> {
        // Step 1: App creates send ring (driver is waiting for this)
        let to_driver = Publisher::create(SEND_PATH, RING_SIZE)?;
        println!("Created send ring: {}", SEND_PATH);
        
        // Step 2: Wait for driver to create recv ring (it will after seeing send)
        let from_driver = Self::wait_for_recv_ring()?;
        println!("Connected to recv ring: {}", RECV_PATH);
        
        Ok(Self {
            to_driver,
            from_driver,
            next_seq: 0,
        })
    }

    /// Create with custom paths
    pub fn with_paths(send_path: &str, recv_path: &str) -> io::Result<Self> {
        let to_driver = Publisher::create(send_path, RING_SIZE)?;
        
        // Wait for driver
        let from_driver = loop {
            match Subscriber::open(recv_path) {
                Ok(s) => break s,
                Err(_) => std::thread::sleep(std::time::Duration::from_millis(10)),
            }
        };
        
        Ok(Self {
            to_driver,
            from_driver,
            next_seq: 0,
        })
    }

    fn wait_for_recv_ring() -> io::Result<Subscriber> {
        for _ in 0..100 {
            if let Ok(s) = Subscriber::open(RECV_PATH) {
                return Ok(s);
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        Err(io::Error::new(io::ErrorKind::NotFound, "kaos-driver not running"))
    }

    /// Send a u64 value (zero syscalls!)
    #[inline]
    pub fn send(&mut self, value: u64) -> io::Result<u64> {
        let seq = self.next_seq;
        self.to_driver.send(value)?;
        self.next_seq += 1;
        Ok(seq)
    }

    /// Try to receive (non-blocking, zero syscalls!)
    #[inline]
    pub fn try_receive(&mut self) -> Option<u64> {
        self.from_driver.try_receive()
    }

    /// Receive with callback (batch, zero syscalls!)
    #[inline]
    pub fn receive<F: FnMut(u64)>(&mut self, f: F) -> usize {
        self.from_driver.receive(f)
    }

    /// Check available messages
    #[inline]
    pub fn available(&mut self) -> u64 {
        self.from_driver.available()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Note: These tests require kaos-driver to be running
    // Run with: cargo test -p kaos-rudp driver -- --ignored

    #[test]
    #[ignore]
    fn test_driver_transport() {
        let mut transport = DriverTransport::new().expect("Start kaos-driver first");
        
        // Send some values
        for i in 0..100u64 {
            transport.send(i).unwrap();
        }
        
        println!("Sent 100 values via driver");
    }
}


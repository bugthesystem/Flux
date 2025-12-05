//! UDP multicast with Kaos ring buffer backbone.
//!
//! ```rust,no_run
//! use kaos_rudp::MulticastTransport;
//! use std::net::Ipv4Addr;
//!
//! // Join multicast group with ring buffer
//! let group = Ipv4Addr::new(239, 255, 0, 1);
//! let mut transport = MulticastTransport::new("0.0.0.0:5000", group, 1024).unwrap();
//!
//! // Send (buffered in ring)
//! transport.send(b"hello").unwrap();
//! transport.flush().unwrap();
//!
//! // Receive batch
//! transport.receive_batch(64, |msg| {
//!     println!("got {} bytes", msg.len());
//! });
//! ```

use kaos::disruptor::{MessageRingBuffer, RingBufferConfig};
use std::io;
use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4, ToSocketAddrs, UdpSocket};

/// Recv buffer size per packet (> MTU 1500)
const RECV_PACKET_SIZE: usize = 2048;

/// Socket buffer size (4MB for throughput)
const SOCKET_BUFFER_SIZE: usize = 4 * 1024 * 1024;

/// Create multicast socket with SO_REUSEADDR.
fn create_multicast_socket(bind_addr: SocketAddr, group: Ipv4Addr) -> io::Result<UdpSocket> {
    let socket2 = socket2::Socket::new(
        socket2::Domain::IPV4,
        socket2::Type::DGRAM,
        Some(socket2::Protocol::UDP),
    )?;
    socket2.set_reuse_address(true)?;
    socket2.set_send_buffer_size(SOCKET_BUFFER_SIZE)?;
    socket2.set_recv_buffer_size(SOCKET_BUFFER_SIZE)?;

    let addr = SocketAddrV4::new(
        match bind_addr.ip() {
            std::net::IpAddr::V4(ip) => ip,
            _ => return Err(io::Error::new(io::ErrorKind::InvalidInput, "IPv4 only")),
        },
        bind_addr.port(),
    );
    socket2.bind(&addr.into())?;

    let socket: UdpSocket = socket2.into();
    socket.join_multicast_v4(&group, &Ipv4Addr::UNSPECIFIED)?;
    socket.set_multicast_loop_v4(false)?;
    socket.set_multicast_ttl_v4(1)?;

    Ok(socket)
}

/// UDP multicast transport with Kaos ring buffer.
pub struct MulticastTransport {
    socket: UdpSocket,
    group: Ipv4Addr,
    port: u16,
    send_ring: MessageRingBuffer,
    consumer_seq: u64,
}

impl MulticastTransport {
    /// Create multicast transport.
    ///
    /// - `bind_addr`: Local address (use "0.0.0.0:PORT")
    /// - `group`: Multicast group (224.0.0.0 - 239.255.255.255)
    /// - `ring_size`: Ring buffer size (must be power of 2)
    pub fn new<A: ToSocketAddrs>(
        bind_addr: A,
        group: Ipv4Addr,
        ring_size: usize,
    ) -> io::Result<Self> {
        let bind_addr = bind_addr
            .to_socket_addrs()?
            .next()
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "invalid address"))?;

        let socket = create_multicast_socket(bind_addr, group)?;
        socket.set_nonblocking(true)?;

        let config = RingBufferConfig::new(ring_size)
            .map_err(|e| io::Error::other(format!("ring config: {}", e)))?
            .with_consumers(1)
            .map_err(|e| io::Error::other(format!("ring config: {}", e)))?;

        let send_ring = MessageRingBuffer::new(config).map_err(|e|
            io::Error::other(format!("ring buffer: {}", e))
        )?;

        Ok(Self {
            socket,
            group,
            port: bind_addr.port(),
            send_ring,
            consumer_seq: 0,
        })
    }

    /// Queue message for sending.
    pub fn send(&mut self, data: &[u8]) -> io::Result<()> {
        if let Some((seq, slots)) = self.send_ring.try_claim_slots(1) {
            slots[0].set_data(data);
            self.send_ring.publish_batch(seq, 1);
            Ok(())
        } else {
            Err(io::Error::new(io::ErrorKind::WouldBlock, "send ring full"))
        }
    }

    /// Queue batch of messages.
    pub fn send_batch(&mut self, messages: &[&[u8]]) -> io::Result<usize> {
        let count = messages.len();
        if let Some((seq, slots)) = self.send_ring.try_claim_slots(count) {
            for (i, msg) in messages.iter().enumerate() {
                slots[i].set_data(msg);
            }
            self.send_ring.publish_batch(seq, count);
            Ok(count)
        } else {
            Err(io::Error::new(io::ErrorKind::WouldBlock, "send ring full"))
        }
    }

    /// Flush send ring to network.
    pub fn flush(&mut self) -> io::Result<usize> {
        let dest = SocketAddr::new(self.group.into(), self.port);
        let mut sent = 0;

        let slots = self.send_ring.peek_batch(0, 1024);
        for slot in slots.iter().skip(self.consumer_seq as usize) {
            let data = slot.data();
            if data.is_empty() {
                break;
            }
            match self.socket.send_to(data, dest) {
                Ok(_) => {
                    sent += 1;
                    self.consumer_seq += 1;
                }
                Err(e) if e.kind() == io::ErrorKind::WouldBlock => {
                    break;
                }
                Err(e) => {
                    return Err(e);
                }
            }
        }

        if sent > 0 {
            self.send_ring.advance_consumer(0, self.consumer_seq);
        }

        Ok(sent)
    }

    /// Send immediately (bypass ring).
    pub fn send_now(&self, data: &[u8]) -> io::Result<usize> {
        let dest = SocketAddr::new(self.group.into(), self.port);
        self.socket.send_to(data, dest)
    }

    /// Receive batch of messages.
    pub fn receive_batch<F>(&self, max: usize, mut handler: F) -> usize where F: FnMut(&[u8]) {
        let mut buf = [0u8; RECV_PACKET_SIZE];
        let mut count = 0;

        for _ in 0..max {
            match self.socket.recv_from(&mut buf) {
                Ok((len, _)) => {
                    handler(&buf[..len]);
                    count += 1;
                }
                Err(_) => {
                    break;
                }
            }
        }

        count
    }

    /// Receive single message (blocking).
    pub fn recv(&self, buf: &mut [u8]) -> io::Result<(usize, SocketAddr)> {
        self.socket.set_nonblocking(false)?;
        let result = self.socket.recv_from(buf);
        self.socket.set_nonblocking(true)?;
        result
    }

    /// Set TTL (hop limit). 1 = local network, 255 = unrestricted.
    pub fn set_ttl(&self, ttl: u32) -> io::Result<()> {
        self.socket.set_multicast_ttl_v4(ttl)
    }

    /// Enable/disable receiving own messages.
    pub fn set_loopback(&self, enable: bool) -> io::Result<()> {
        self.socket.set_multicast_loop_v4(enable)
    }

    /// Get multicast group.
    pub fn group(&self) -> Ipv4Addr {
        self.group
    }

    /// Get underlying socket.
    pub fn socket(&self) -> &UdpSocket {
        &self.socket
    }
}

impl Drop for MulticastTransport {
    fn drop(&mut self) {
        let _ = self.socket.leave_multicast_v4(&self.group, &Ipv4Addr::UNSPECIFIED);
    }
}

// Simple socket for basic use cases
/// Simple UDP multicast socket (no ring buffer).
pub struct MulticastSocket {
    socket: UdpSocket,
    group: Ipv4Addr,
    port: u16,
}

impl MulticastSocket {
    /// Join a multicast group.
    pub fn join<A: ToSocketAddrs>(bind_addr: A, group: Ipv4Addr) -> io::Result<Self> {
        let bind_addr = bind_addr
            .to_socket_addrs()?
            .next()
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "invalid address"))?;

        let socket = create_multicast_socket(bind_addr, group)?;

        Ok(Self {
            socket,
            group,
            port: bind_addr.port(),
        })
    }

    /// Send to all group members.
    pub fn send(&self, data: &[u8], port: u16) -> io::Result<usize> {
        self.socket.send_to(data, SocketAddr::new(self.group.into(), port))
    }

    /// Send to group on same port.
    pub fn broadcast(&self, data: &[u8]) -> io::Result<usize> {
        self.send(data, self.port)
    }

    /// Receive from group.
    pub fn recv(&self, buf: &mut [u8]) -> io::Result<(usize, SocketAddr)> {
        self.socket.recv_from(buf)
    }

    /// Set non-blocking mode.
    pub fn set_nonblocking(&self, nonblocking: bool) -> io::Result<()> {
        self.socket.set_nonblocking(nonblocking)
    }

    /// Set TTL.
    pub fn set_ttl(&self, ttl: u32) -> io::Result<()> {
        self.socket.set_multicast_ttl_v4(ttl)
    }

    /// Set loopback.
    pub fn set_loopback(&self, enable: bool) -> io::Result<()> {
        self.socket.set_multicast_loop_v4(enable)
    }

    /// Get group.
    pub fn group(&self) -> Ipv4Addr {
        self.group
    }

    /// Get socket.
    pub fn socket(&self) -> &UdpSocket {
        &self.socket
    }
}

impl Drop for MulticastSocket {
    fn drop(&mut self) {
        let _ = self.socket.leave_multicast_v4(&self.group, &Ipv4Addr::UNSPECIFIED);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_transport_create() {
        let group = Ipv4Addr::new(239, 255, 0, 1);
        // May fail in sandboxed environments
        if let Ok(t) = MulticastTransport::new("0.0.0.0:0", group, 64) {
            assert_eq!(t.group(), group);
        }
    }

    #[test]
    fn test_socket_create() {
        let group = Ipv4Addr::new(239, 255, 0, 1);
        if let Ok(s) = MulticastSocket::join("0.0.0.0:0", group) {
            assert_eq!(s.group(), group);
        }
    }
}

//! Reliable UDP transport with NAK-based retransmission
//!
//! This module implements a high-performance reliable UDP protocol using:
//! 1. Sequence numbering for packet ordering and loss detection
//! 2. NAK (Negative Acknowledgment) for efficient retransmission requests
//! 3. Sliding window flow control
//! 4. Timeout-based recovery for lost NAKs
//! 5. Multicast-friendly design (no ACKs, only NAKs)
//! 6. Integration with ring buffer

use std::net::{ SocketAddr, UdpSocket };
use std::time::{ Instant, SystemTime, UNIX_EPOCH };

/// Message types for reliable UDP protocol
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum MessageType {
    Data = 0,
    Heartbeat = 1,
    Nak = 2,
    SessionStart = 3,
    SessionEnd = 4,
}

/// Reliable UDP packet header
#[repr(C, packed)]
#[derive(Debug, Clone, Copy)]
pub struct ReliableUdpHeader {
    /// Session ID for multiplexing
    pub session_id: u32,
    /// Sequence number
    pub sequence: u64,
    /// Message type
    pub msg_type: u8,
    /// Flags (reserved for future use)
    pub flags: u8,
    /// Payload length
    pub payload_len: u16,
    /// Timestamp (nanoseconds since epoch)
    pub timestamp: u64,
    /// Checksum (CRC32)
    pub checksum: u32,
}

impl ReliableUdpHeader {
    const SIZE: usize = std::mem::size_of::<Self>();

    /// Create new header
    pub fn new(session_id: u32, sequence: u64, msg_type: MessageType, payload_len: u16) -> Self {
        let timestamp = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos() as u64;

        Self {
            session_id,
            sequence,
            msg_type: msg_type as u8,
            flags: 0,
            payload_len,
            timestamp,
            checksum: 0, // Calculated later
        }
    }

    /// Calculate and set checksum
    pub fn calculate_checksum(&mut self, payload: &[u8]) {
        // Save original checksum
        let _original = self.checksum;
        self.checksum = 0;

        // Calculate CRC32 over header + payload
        let header_bytes = unsafe {
            std::slice::from_raw_parts(self as *const Self as *const u8, Self::SIZE)
        };

        let mut hasher = crc32fast::Hasher::new();
        hasher.update(header_bytes);
        hasher.update(payload);
        self.checksum = hasher.finalize();
    }

    /// Verify checksum
    pub fn verify_checksum(&self, payload: &[u8]) -> bool {
        let mut temp_header = *self;
        temp_header.calculate_checksum(payload);
        temp_header.checksum == self.checksum
    }

    /// Convert to bytes
    pub fn as_bytes(&self) -> &[u8] {
        unsafe { std::slice::from_raw_parts(self as *const Self as *const u8, Self::SIZE) }
    }

    /// Convert from bytes
    pub fn from_bytes(bytes: &[u8]) -> Option<Self> {
        if bytes.len() < Self::SIZE {
            return None;
        }

        unsafe { Some(std::ptr::read_unaligned(bytes.as_ptr() as *const Self)) }
    }
}

/// Packet stored in retransmission buffer
#[derive(Debug, Clone)]
pub struct RetransmissionPacket {
    /// Complete packet data (header + payload)
    pub data: Vec<u8>,
    /// Send timestamp
    pub sent_time: Instant,
    /// Number of retransmission attempts
    pub attempts: u32,
    /// Destination address
    pub addr: SocketAddr,
}

impl TryFrom<u8> for MessageType {
    type Error = ();

    fn try_from(value: u8) -> std::result::Result<Self, Self::Error> {
        match value {
            0 => Ok(MessageType::Data),
            1 => Ok(MessageType::Heartbeat),
            2 => Ok(MessageType::Nak),
            3 => Ok(MessageType::SessionStart),
            4 => Ok(MessageType::SessionEnd),
            _ => Err(()),
        }
    }
}

/// Fully ring buffer-based reliable UDP transport (experimental)
pub struct ReliableUdpRingBufferTransport {
    /// UDP socket
    socket: UdpSocket,
    /// Send window (ring buffer of RetransmissionPacket)
    send_window: Vec<Option<RetransmissionPacket>>,
    /// Receive buffer (ring buffer of received payloads)
    recv_buffer: Vec<Option<Vec<u8>>>,
    /// Window size (must be < u64::MAX/2 for wraparound)
    window_size: usize,
    /// Next sequence to send
    next_send_seq: u64,
    /// Next expected sequence to receive
    next_recv_seq: u64,
    /// Remote address
    remote_addr: SocketAddr,
}

impl ReliableUdpRingBufferTransport {
    pub fn new(
        bind_addr: SocketAddr,
        remote_addr: SocketAddr,
        window_size: usize
    ) -> std::io::Result<Self> {
        let socket = UdpSocket::bind(bind_addr)?;
        socket.set_nonblocking(true)?;
        Ok(Self {
            socket,
            send_window: vec![None; window_size],
            recv_buffer: vec![None; window_size],
            window_size,
            next_send_seq: 0,
            next_recv_seq: 0,
            remote_addr,
        })
    }

    /// Send a message reliably (enqueue in send window, add header, send over UDP)
    pub fn send(&mut self, data: &[u8]) -> std::io::Result<u64> {
        let seq = self.next_send_seq;
        let idx = (seq as usize) % self.window_size;
        // Build header
        let mut header = ReliableUdpHeader::new(0, seq, MessageType::Data, data.len() as u16);
        header.calculate_checksum(data);
        let mut packet = Vec::with_capacity(ReliableUdpHeader::SIZE + data.len());
        packet.extend_from_slice(unsafe {
            std::slice::from_raw_parts(&header as *const _ as *const u8, ReliableUdpHeader::SIZE)
        });
        packet.extend_from_slice(data);
        // Store in send window
        self.send_window[idx] = Some(RetransmissionPacket {
            data: packet.clone(),
            sent_time: Instant::now(),
            attempts: 0,
            addr: self.remote_addr,
        });
        // Send over UDP
        let _ = self.socket.send_to(&packet, self.remote_addr);
        self.next_send_seq = self.next_send_seq.wrapping_add(1);
        Ok(seq)
    }

    /// Actually transmit packets in the send window (simulate network send)
    pub fn transmit(&mut self) {
        for (i, slot) in self.send_window.iter_mut().enumerate() {
            if let Some(pkt) = slot {
                // Simulate sending over UDP
                let _ = self.socket.send_to(&pkt.data, pkt.addr);
                pkt.sent_time = Instant::now();
                pkt.attempts += 1;
            }
        }
    }

    /// Receive a message, parse header, place in recv_buffer, deliver in order
    pub fn receive(&mut self) -> Option<Vec<u8>> {
        let mut buf = [0u8; 2048];
        if let Ok((len, _src)) = self.socket.recv_from(&mut buf) {
            if len < ReliableUdpHeader::SIZE {
                return None;
            }
            let header = ReliableUdpHeader::from_bytes(&buf[..ReliableUdpHeader::SIZE])?;
            if header.msg_type != (MessageType::Data as u8) {
                return None;
            }
            let payload = &buf[ReliableUdpHeader::SIZE..len];
            if !header.verify_checksum(payload) {
                return None;
            }
            let seq = header.sequence;
            let idx = (seq as usize) % self.window_size;
            self.recv_buffer[idx] = Some(payload.to_vec());
            // Deliver in order
            let expected_idx = (self.next_recv_seq as usize) % self.window_size;
            if let Some(msg) = self.recv_buffer[expected_idx].take() {
                self.next_recv_seq = self.next_recv_seq.wrapping_add(1);
                return Some(msg);
            }
        }
        None
    }

    /// Retransmit lost packets (on NAK)
    pub fn retransmit(&mut self, lost_seq: u64) {
        let idx = (lost_seq as usize) % self.window_size;
        if let Some(pkt) = &self.send_window[idx] {
            let _ = self.socket.send_to(&pkt.data, pkt.addr);
        }
    }

    /// Send a NAK for a missing sequence number
    pub fn send_nak(&self, missing_seq: u64) {
        let mut header = ReliableUdpHeader::new(0, missing_seq, MessageType::Nak, 0);
        header.calculate_checksum(&[]);
        let mut packet = Vec::with_capacity(ReliableUdpHeader::SIZE);
        packet.extend_from_slice(unsafe {
            std::slice::from_raw_parts(&header as *const _ as *const u8, ReliableUdpHeader::SIZE)
        });
        let _ = self.socket.send_to(&packet, self.remote_addr);
    }

    /// Process incoming NAKs and retransmit as needed
    pub fn process_naks(&mut self) {
        let mut buf = [0u8; 2048];
        if let Ok((len, _src)) = self.socket.recv_from(&mut buf) {
            if len < ReliableUdpHeader::SIZE {
                return;
            }
            let header = ReliableUdpHeader::from_bytes(&buf[..ReliableUdpHeader::SIZE]);
            if let Some(h) = header {
                if h.msg_type == (MessageType::Nak as u8) {
                    self.retransmit(h.sequence);
                }
            }
        }
    }
}

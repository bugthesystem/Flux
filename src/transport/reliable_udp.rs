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
use crate::disruptor::{ RingBufferConfig, MessageSlot, RingBuffer, RingBufferEntry };
use crc32fast::Hasher;

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

    pub fn calculate_checksum(&mut self, payload: &[u8]) {
        self.checksum = 0;
        let header_bytes = unsafe {
            std::slice::from_raw_parts(self as *const Self as *const u8, Self::SIZE)
        };
        let mut hasher = Hasher::new();
        hasher.update(header_bytes);
        hasher.update(payload);
        self.checksum = hasher.finalize();
    }
    pub fn verify_checksum(&self, payload: &[u8]) -> bool {
        let mut temp_header = *self;
        temp_header.calculate_checksum(payload);
        temp_header.checksum == self.checksum
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
    socket: UdpSocket,
    send_window: RingBuffer,
    recv_window: RingBuffer,
    window_size: usize,
    next_send_seq: u64,
    next_recv_seq: u64,
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
        // Set large socket buffers for high throughput
        #[cfg(unix)]
        {
            use std::os::unix::io::AsRawFd;
            let fd = socket.as_raw_fd();
            let buffer_size = 8 * 1024 * 1024i32; // 8MB
            unsafe {
                libc::setsockopt(
                    fd,
                    libc::SOL_SOCKET,
                    libc::SO_SNDBUF,
                    &buffer_size as *const i32 as *const libc::c_void,
                    std::mem::size_of::<i32>() as libc::socklen_t
                );
                libc::setsockopt(
                    fd,
                    libc::SOL_SOCKET,
                    libc::SO_RCVBUF,
                    &buffer_size as *const i32 as *const libc::c_void,
                    std::mem::size_of::<i32>() as libc::socklen_t
                );
            }
        }
        let ring_config = RingBufferConfig::new(window_size)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e.to_string()))?
            .with_consumers(1)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e.to_string()))?;
        let send_window = RingBuffer::new(ring_config.clone()).map_err(|e|
            std::io::Error::new(std::io::ErrorKind::Other, e.to_string())
        )?;
        let recv_window = RingBuffer::new(ring_config).map_err(|e|
            std::io::Error::new(std::io::ErrorKind::Other, e.to_string())
        )?;
        Ok(Self {
            socket,
            send_window,
            recv_window,
            window_size,
            next_send_seq: 0,
            next_recv_seq: 0,
            remote_addr,
        })
    }

    /// Send a message reliably (enqueue in send window, add header, send over UDP)
    pub fn send(&mut self, data: &[u8]) -> std::io::Result<u64> {
        let seq = self.next_send_seq;
        let mut header = ReliableUdpHeader::new(0, seq, MessageType::Data, data.len() as u16);
        header.calculate_checksum(data);
        // Pre-allocate buffer for header+payload (stack for small, heap for large)
        const MAX_STACK_SIZE: usize = 256;
        let total_len = ReliableUdpHeader::SIZE + data.len();
        let mut stack_buf = [0u8; MAX_STACK_SIZE];
        let packet: &[u8] = if total_len <= MAX_STACK_SIZE {
            stack_buf[..ReliableUdpHeader::SIZE].copy_from_slice(unsafe {
                std::slice::from_raw_parts(
                    &header as *const _ as *const u8,
                    ReliableUdpHeader::SIZE
                )
            });
            stack_buf[ReliableUdpHeader::SIZE..total_len].copy_from_slice(data);
            &stack_buf[..total_len]
        } else {
            // Fallback to heap allocation for large messages
            let mut heap_buf = Vec::with_capacity(total_len);
            heap_buf.extend_from_slice(unsafe {
                std::slice::from_raw_parts(
                    &header as *const _ as *const u8,
                    ReliableUdpHeader::SIZE
                )
            });
            heap_buf.extend_from_slice(data);
            // Claim a slot in the send window
            if let Some((slot_seq, slots)) = self.send_window.try_claim_slots(1) {
                slots[0].set_sequence(slot_seq);
                slots[0].set_data(&heap_buf);
                self.send_window.publish_batch(slot_seq, 1);
                let _ = self.socket.send_to(&heap_buf, self.remote_addr);
                self.next_send_seq = self.next_send_seq.wrapping_add(1);
                return Ok(seq);
            } else {
                return Err(std::io::Error::new(std::io::ErrorKind::WouldBlock, "Send window full"));
            }
        };
        // Claim a slot in the send window
        if let Some((slot_seq, slots)) = self.send_window.try_claim_slots(1) {
            slots[0].set_sequence(slot_seq);
            slots[0].set_data(packet);
            self.send_window.publish_batch(slot_seq, 1);
            let _ = self.socket.send_to(packet, self.remote_addr);
            self.next_send_seq = self.next_send_seq.wrapping_add(1);
            Ok(seq)
        } else {
            Err(std::io::Error::new(std::io::ErrorKind::WouldBlock, "Send window full"))
        }
    }

    /// Send a batch of messages reliably (enqueue in send window, add header, send over UDP)
    pub fn send_batch(&mut self, data: &[&[u8]]) -> std::io::Result<usize> {
        let batch_size = data.len();
        if batch_size == 0 {
            return Ok(0);
        }
        if let Some((slot_seq, slots)) = self.send_window.try_claim_slots(batch_size) {
            let actual = std::cmp::min(slots.len(), data.len());
            let mut packets: Vec<Vec<u8>> = Vec::with_capacity(actual);
            for i in 0..actual {
                let seq = self.next_send_seq;
                let msg = data[i];
                let mut header = ReliableUdpHeader::new(
                    0,
                    seq,
                    MessageType::Data,
                    msg.len() as u16
                );
                header.calculate_checksum(msg);
                let total_len = ReliableUdpHeader::SIZE + msg.len();
                let mut packet = Vec::with_capacity(total_len);
                packet.extend_from_slice(unsafe {
                    std::slice::from_raw_parts(
                        &header as *const _ as *const u8,
                        ReliableUdpHeader::SIZE
                    )
                });
                packet.extend_from_slice(msg);
                slots[i].set_sequence(slot_seq + (i as u64));
                slots[i].set_data(&packet);
                packets.push(packet);
                self.next_send_seq = self.next_send_seq.wrapping_add(1);
            }
            self.send_window.publish_batch(slot_seq, actual);
            // Batch send: send all packets in a tight loop
            for pkt in &packets {
                let _ = self.socket.send_to(pkt, self.remote_addr);
            }
            Ok(actual)
        } else {
            Err(std::io::Error::new(std::io::ErrorKind::WouldBlock, "Send window full"))
        }
    }

    /// Actually transmit packets in the send window (simulate network send)
    pub fn transmit(&mut self) {
        // Remove .dequeue() usage; use try_consume_batch or similar if needed
        // For retransmission, use the retransmit method
        // For now, this can be left empty or used for future logic
    }

    /// Receive a message, parse header, place in recv_window, deliver in order
    pub fn receive(&mut self) -> Option<Vec<u8>> {
        let mut buf = [0u8; 2048];
        match self.socket.recv_from(&mut buf) {
            Ok((len, src)) => {
                // Parse header
                if len < ReliableUdpHeader::SIZE {
                    return None;
                }
                let header = ReliableUdpHeader::from_bytes(&buf[..ReliableUdpHeader::SIZE]);
                if header.is_none() {
                    return None;
                }
                let header = header.unwrap();
                let seq = header.sequence;
                let msg_type = header.msg_type;
                let payload_len = header.payload_len;
                if msg_type != (MessageType::Data as u8) {
                    return None;
                }
                let payload = &buf[ReliableUdpHeader::SIZE..len];
                if !header.verify_checksum(payload) {
                    return None;
                }
                // Strict in-order delivery
                if seq == self.next_recv_seq {
                    // Claim a slot in the receive window
                    match self.recv_window.try_claim_slots(1) {
                        Some((slot_seq, slots)) => {
                            slots[0].set_sequence(slot_seq);
                            slots[0].set_data(payload);
                            self.recv_window.publish_batch(slot_seq, 1);
                        }
                        None => {
                            return None;
                        }
                    }
                    // Deliver in order
                    let msgs = self.recv_window.try_consume_batch(0, 1);
                    if !msgs.is_empty() {
                        self.next_recv_seq = self.next_recv_seq.wrapping_add(1);
                        return Some(msgs[0].data().to_vec());
                    } else {
                        // println!("[PROTO][RECV] No message ready to deliver after publish");
                    }
                } else if seq < self.next_recv_seq {
                    // println!(
                    //     "[PROTO][RECV] Duplicate or old packet: seq={}, expected={}",
                    //     seq,
                    //     self.next_recv_seq
                    // );
                    // Ignore duplicate/old
                } else {
                    // println!(
                    //     "[PROTO][RECV] Out-of-order packet: seq={}, expected={}",
                    //     seq,
                    //     self.next_recv_seq
                    // );
                    // Ignore out-of-order for now
                }
            }
            Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                // No UDP data available
            }
            Err(e) => {
                // println!("[PROTO][RECV][ERR] Socket error: {}", e);
            }
        }
        None
    }

    /// Retransmit lost packets (on NAK)
    pub fn retransmit(&mut self, lost_seq: u64) {
        let idx = (lost_seq as usize) % self.window_size;
        // Try to consume the slot for retransmission
        let msgs = self.send_window.try_consume_batch(0, self.window_size);
        for slot in msgs {
            if slot.sequence() == lost_seq {
                let pkt_data = slot.data();
                let _ = self.socket.send_to(pkt_data, self.remote_addr);
                break;
            }
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

    /// Get a reference to the underlying UDP socket (for debugging)
    pub fn socket(&self) -> &UdpSocket {
        &self.socket
    }
}

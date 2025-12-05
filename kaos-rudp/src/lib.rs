//! # kaos-rudp
//!
//! Reliable UDP transport built on kaos ring buffers.

#![allow(unused_variables)]
#![allow(clippy::needless_range_loop)]
//!
//! ## Features
//!
//! - **Fast**: 3M+ messages/sec on localhost (faster than Aeron UDP)
//! - **Reliable**: NAK-based retransmission for loss recovery
//! - **Minimal allocations**: Pre-allocated buffers in hot path
//! - **Flexible**: Multiple header formats (FastHeader for speed, full header for reliability)
//!
//! ## Quick Start
//!
//! ```rust,no_run
//! use kaos_rudp::ReliableUdpRingBufferTransport;
//!
//! // Server
//! let server_addr = "127.0.0.1:9000".parse().unwrap();
//! let client_addr = "127.0.0.1:9001".parse().unwrap();
//! let mut server = ReliableUdpRingBufferTransport::new(server_addr, client_addr, 65536).unwrap();
//!
//! // Receive messages
//! server.receive_batch_with(64, |msg| {
//!     println!("Received: {} bytes", msg.len());
//! });
//! ```
//!
//! ## Protocol
//!
//! - Sequence numbering for ordering
//! - NAK (Negative Acknowledgment) for retransmission requests
//! - Sliding window flow control
//! - Multicast-friendly (no ACKs required)

use bytemuck::{Pod, Zeroable};
use kaos::crc32;
use kaos::disruptor::{MessageRingBuffer, RingBufferConfig, RingBufferEntry};
use std::cell::RefCell;
use std::net::{SocketAddr, UdpSocket};
use std::time::{SystemTime, UNIX_EPOCH};

// Buffer sizes for UDP I/O
/// Max send buffer (64KB = max UDP payload with some headroom)
const SEND_BUFFER_SIZE: usize = 65536;
/// Large message assembly buffer (4KB typical MTU-friendly size)
const LARGE_MSG_SIZE: usize = 4096;
/// Per-packet receive buffer (2KB > typical MTU 1500)
const RECV_PACKET_SIZE: usize = 2048;
/// Batch size for recvmmsg (64 packets per syscall)
const RECV_BATCH_SIZE: usize = 64;
/// Socket buffer size (8MB for high throughput)
const SOCKET_BUFFER_SIZE: i32 = 8 * 1024 * 1024;

thread_local! {
    static SEND_BUFFER: RefCell<Vec<u8>> = RefCell::new(Vec::with_capacity(SEND_BUFFER_SIZE));
    static LARGE_MSG_BUFFER: RefCell<Vec<u8>> = RefCell::new(Vec::with_capacity(LARGE_MSG_SIZE));
    static RECV_BUFFERS: RefCell<Vec<[u8; RECV_PACKET_SIZE]>> = RefCell::new(vec![[0u8; RECV_PACKET_SIZE]; RECV_BATCH_SIZE]);
    static RECV_LENS: RefCell<Vec<usize>> = RefCell::new(vec![0usize; RECV_BATCH_SIZE]);
}

pub mod congestion;
#[cfg(feature = "driver")]
pub mod driver;
pub mod multicast;
mod sendmmsg;
mod window;

use congestion::CongestionController;
pub use congestion::CongestionController as Congestion;
#[cfg(feature = "driver")]
pub use driver::DriverTransport;
pub use multicast::{MulticastSocket, MulticastTransport};
use kaos::{record_backpressure, record_receive, record_retransmit, record_send};
use window::BitmapWindow;

/// Message types for reliable UDP protocol
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum MessageType {
    Data = 0,
    Heartbeat = 1,
    Nak = 2,
    SessionStart = 3,
    SessionEnd = 4,
    Ack = 5, // Acknowledgment for reliable delivery
}

/// Header flags for fast path
pub const FLAG_NO_CRC: u8 = 0x01; // Skip CRC

/// Aeron-style minimal frame header (8 bytes only!)
/// 8-byte header (3x smaller than ReliableUdpHeader)
#[repr(C, packed)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
pub struct FastHeader {
    pub frame_length: u32, // Total frame length including header (high bit set = FastHeader magic)
    pub sequence: u32,     // Sequence number (32-bit is enough for most use cases)
}

/// Magic marker in high bit of frame_length to identify FastHeader format
pub const FAST_HEADER_MAGIC: u32 = 0x80000000;

impl FastHeader {
    pub const SIZE: usize = 8;

    #[inline(always)]
    pub fn new(sequence: u32, payload_len: usize) -> Self {
        Self {
            // Set high bit as magic marker + actual length in lower 31 bits
            frame_length: FAST_HEADER_MAGIC | ((Self::SIZE + payload_len) as u32),
            sequence,
        }
    }
}

/// Reliable UDP packet header
#[repr(C, packed)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
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
    /// Timestamp (milliseconds since epoch) - RELIABLE-UDP-OPTIMIZED: Reduced from 64-bit to 32-bit
    pub timestamp: u32,
    /// Checksum (CRC32)
    pub checksum: u32,
}

impl ReliableUdpHeader {
    const SIZE: usize = std::mem::size_of::<Self>();

    pub fn new(session_id: u32, sequence: u64, msg_type: MessageType, payload_len: u16) -> Self {
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_millis() as u32;

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

    /// Convert from bytes (safe via bytemuck)
    pub fn from_bytes(bytes: &[u8]) -> Option<Self> {
        if bytes.len() < Self::SIZE {
            return None;
        }
        // Safe: ReliableUdpHeader derives Pod
        Some(*bytemuck::from_bytes::<Self>(&bytes[..Self::SIZE]))
    }

    pub fn from_packet_with_payload_check(packet: &[u8]) -> Option<(&Self, &[u8])> {
        if packet.len() < Self::SIZE {
            return None;
        }

        let (header_bytes, payload) = packet.split_at(Self::SIZE);
        let header = bytemuck::from_bytes::<Self>(header_bytes);

        // Validate payload length
        if payload.len() < (header.payload_len as usize) {
            return None;
        }

        let (actual_payload, _) = payload.split_at(header.payload_len as usize);
        Some((header, actual_payload))
    }

    pub fn calculate_checksum(&mut self, payload: &[u8]) {
        self.checksum = 0;
        // Safe: ReliableUdpHeader derives Pod
        let header_bytes = bytemuck::bytes_of(self);

        // Use platform-optimized CRC32 calculation (incremental to avoid allocation)
        let mut crc = crc32::crc32_simd(header_bytes);
        crc = crc32::crc32_incremental(crc, payload);
        self.checksum = crc;
    }
    pub fn verify_checksum(&self, payload: &[u8]) -> bool {
        let mut temp_header = *self;
        temp_header.calculate_checksum(payload);
        temp_header.checksum == self.checksum
    }
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
            5 => Ok(MessageType::Ack),
            _ => Err(()),
        }
    }
}

/// Fully ring buffer-based reliable UDP transport (experimental)
pub struct ReliableUdpRingBufferTransport {
    socket: std::sync::Arc<UdpSocket>,
    nak_socket: UdpSocket,
    send_window: MessageRingBuffer,
    recv_window: BitmapWindow,
    window_size: usize,
    next_send_seq: u64,
    _next_recv_seq: u64,
    acked_seq: u64,
    remote_addr: SocketAddr,
    remote_nak_addr: SocketAddr,
    congestion: CongestionController,
    #[cfg(target_os = "linux")]
    batch_sender: sendmmsg::BatchSender,
    #[cfg(target_os = "linux")]
    batch_receiver: sendmmsg::BatchReceiver,
}

#[derive(Debug, Clone)]
pub struct ReliableUdpConfig {
    pub local_addr: String,
    pub remote_addr: String,
    pub window_size: usize,
}

impl Default for ReliableUdpConfig {
    fn default() -> Self {
        Self {
            local_addr: "127.0.0.1:0".to_string(),
            remote_addr: "127.0.0.1:0".to_string(),
            window_size: 1024,
        }
    }
}

impl ReliableUdpRingBufferTransport {
    pub fn new(
        bind_addr: SocketAddr,
        remote_addr: SocketAddr,
        window_size: usize,
    ) -> std::io::Result<Self> {
        let socket = UdpSocket::bind(bind_addr)?;
        socket.set_nonblocking(true)?;

        // Create NAK socket on port+1
        let nak_port = bind_addr.port() + 1;
        let nak_bind_addr = SocketAddr::new(bind_addr.ip(), nak_port);
        let nak_socket = UdpSocket::bind(nak_bind_addr)?;
        nak_socket.set_nonblocking(true)?;

        #[cfg(unix)]
        {
            use std::os::unix::io::AsRawFd;
            let fd = socket.as_raw_fd();
            let buffer_size = SOCKET_BUFFER_SIZE;
            unsafe {
                libc::setsockopt(
                    fd,
                    libc::SOL_SOCKET,
                    libc::SO_SNDBUF,
                    &buffer_size as *const i32 as *const libc::c_void,
                    std::mem::size_of::<i32>() as u32,
                );
                libc::setsockopt(
                    fd,
                    libc::SOL_SOCKET,
                    libc::SO_RCVBUF,
                    &buffer_size as *const i32 as *const libc::c_void,
                    std::mem::size_of::<i32>() as u32,
                );
            }
        }

        // Remote NAK port is also port+1
        let remote_nak_addr = SocketAddr::new(remote_addr.ip(), remote_addr.port() + 1);

        let config = RingBufferConfig::new(window_size)
            .map_err(|e| std::io::Error::other(format!("Invalid window size: {}", e)))?
            .with_consumers(1)
            .map_err(|e| std::io::Error::other(format!("Config error: {}", e)))?;

        let send_window = MessageRingBuffer::new(config)
            .map_err(|e| std::io::Error::other(format!("RingBuffer error: {}", e)))?;

        Ok(Self {
            socket: std::sync::Arc::new(socket),
            nak_socket,
            send_window,
            recv_window: BitmapWindow::new(window_size, 0),
            window_size,
            next_send_seq: 0,
            _next_recv_seq: 0,
            acked_seq: 0,
            remote_addr,
            remote_nak_addr,
            congestion: CongestionController::new(64, window_size as u32),
            #[cfg(target_os = "linux")]
            batch_sender: sendmmsg::BatchSender::new(64),
            #[cfg(target_os = "linux")]
            batch_receiver: sendmmsg::BatchReceiver::new(64, 65536),
        })
    }

    pub fn auto(config: ReliableUdpConfig) -> std::io::Result<Self> {
        let bind_addr: std::net::SocketAddr = config.local_addr.parse().map_err(|e| {
            std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                format!("Invalid local_addr: {}", e),
            )
        })?;
        let remote_addr: std::net::SocketAddr = config.remote_addr.parse().map_err(|e| {
            std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                format!("Invalid remote_addr: {}", e),
            )
        })?;
        Self::new(bind_addr, remote_addr, config.window_size)
    }

    pub fn send(&mut self, data: &[u8]) -> std::io::Result<u64> {
        // Congestion control: check if we can send
        if !self.congestion.can_send() {
            record_backpressure();
            return Err(std::io::Error::new(
                std::io::ErrorKind::WouldBlock,
                "Congestion window full",
            ));
        }

        let seq = self.next_send_seq;
        let mut header = ReliableUdpHeader::new(0, seq, MessageType::Data, data.len() as u16);
        header.calculate_checksum(data);

        const MAX_STACK_SIZE: usize = 256;
        let total_len = ReliableUdpHeader::SIZE + data.len();
        let mut stack_buf = [0u8; MAX_STACK_SIZE];

        let packet: &[u8] = if total_len <= MAX_STACK_SIZE {
            // Safe: ReliableUdpHeader derives Pod
            stack_buf[..ReliableUdpHeader::SIZE].copy_from_slice(bytemuck::bytes_of(&header));
            stack_buf[ReliableUdpHeader::SIZE..total_len].copy_from_slice(data);
            &stack_buf[..total_len]
        } else {
            return LARGE_MSG_BUFFER.with(|buf_cell| {
                let mut buffer = buf_cell.borrow_mut();
                buffer.clear();
                // Safe: ReliableUdpHeader derives Pod
                buffer.extend_from_slice(bytemuck::bytes_of(&header));
                buffer.extend_from_slice(data);

                if let Some((slot_seq, slots)) = self.send_window.try_claim_slots(1) {
                    slots[0].set_sequence(slot_seq);
                    slots[0].set_data(&buffer);
                    self.send_window.publish_batch(slot_seq, 1);

                    self.socket.send_to(&buffer, self.remote_addr)?;
                    self.congestion.on_send();
                    record_send(buffer.len() as u64);
                    self.next_send_seq = self.next_send_seq.wrapping_add(1);
                    Ok(seq)
                } else {
                    record_backpressure();
                    Err(std::io::Error::new(
                        std::io::ErrorKind::WouldBlock,
                        "Send window full",
                    ))
                }
            });
        };

        if let Some((slot_seq, slots)) = self.send_window.try_claim_slots(1) {
            slots[0].set_sequence(slot_seq);
            slots[0].set_data(packet);
            self.send_window.publish_batch(slot_seq, 1);

            self.socket.send_to(packet, self.remote_addr)?;
            self.congestion.on_send();
            record_send(packet.len() as u64);
            self.next_send_seq = self.next_send_seq.wrapping_add(1);
            Ok(seq)
        } else {
            record_backpressure();
            Err(std::io::Error::new(
                std::io::ErrorKind::WouldBlock,
                "Send window full",
            ))
        }
    }

    pub fn send_batch(&mut self, data: &[&[u8]]) -> std::io::Result<usize> {
        self.send_batch_ultra(data)
    }

    ///  Batch send with minimal header and no CRC or timestamp
    #[inline]
    pub fn send_batch_ultra(&mut self, data: &[&[u8]]) -> std::io::Result<usize> {
        let batch_size = data.len();
        if batch_size == 0 {
            return Ok(0);
        }

        if let Some((slot_seq, _slots)) = self.send_window.try_claim_slots(batch_size) {
            let actual = batch_size;

            SEND_BUFFER.with(|buf_cell| {
                let mut buf = buf_cell.borrow_mut();
                buf.clear();

                // Pre-size buffer
                let estimated_size = actual * (FastHeader::SIZE + 64);
                let current_cap = buf.capacity();
                if current_cap < estimated_size {
                    buf.reserve(estimated_size - current_cap);
                }

                for i in 0..actual {
                    let seq = (slot_seq + (i as u64)) as u32;
                    let msg = data[i];

                    // Minimal 8-byte header
                    let header = FastHeader::new(seq, msg.len());
                    // Safe: FastHeader derives Pod
                    buf.extend_from_slice(bytemuck::bytes_of(&header));
                    buf.extend_from_slice(msg);
                }

                let _ = self.socket.send_to(&buf, self.remote_addr);
            });

            self.send_window.publish_batch(slot_seq, actual);
            // Keep messages for retransmission - only advance on ACK
            self.next_send_seq = slot_seq + (actual as u64);
            Ok(actual)
        } else {
            Err(std::io::Error::new(
                std::io::ErrorKind::WouldBlock,
                "Send window full",
            ))
        }
    }

    pub fn receive(&mut self) -> Option<Box<[u8; 2048]>> {
        let mut buf = [0u8; 2048];
        match self.socket.recv_from(&mut buf) {
            Ok((len, src)) => {
                let data = &buf[..len];

                if len >= ReliableUdpHeader::SIZE {
                    if let Some((header, payload)) =
                        ReliableUdpHeader::from_packet_with_payload_check(data)
                    {
                        let msg_type = header.msg_type;
                        let seq = header.sequence;
                        if msg_type == (MessageType::Data as u8) {
                            if header.verify_checksum(payload) {
                                #[cfg(feature = "debug")]
                                eprintln!(
                                    "[RECV] Received DATA seq {} ({} bytes) from {}",
                                    seq,
                                    payload.len(),
                                    src
                                );
                                self.recv_window.insert(seq, payload);
                            } else {
                                #[cfg(feature = "debug")]
                                eprintln!("[RECV-ERROR] Checksum failed for seq {}", seq);
                            }
                        }
                    }
                }
                self.recv_window.send_batch_naks_for_gaps(|start, end| {
                    #[cfg(feature = "debug")]
                    eprintln!("[RECV-GAP] Detected gap seq {}-{}, sending NAK", start, end);
                    self.send_batch_nak(start, end);
                });
            }
            Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                // No UDP data available, just fall through
            }
            Err(e) => {
                #[cfg(feature = "debug")]
                eprintln!("[WARN] Socket error in receive: {}", e);
            }
        }
        let mut found = None;
        self.recv_window.deliver_in_order_with(|msg| {
            if found.is_none() {
                let mut out_buf = Box::new([0u8; 2048]);
                out_buf[..msg.len()].copy_from_slice(msg);
                found = Some(out_buf);
            }
        });
        found
    }

    /// Retransmit lost packets (on NAK)
    pub fn retransmit(&mut self, lost_seq: u64) {
        let slots = self.send_window.peek_batch(0, self.window_size);
        if let Some(slot) = slots.iter().find(|s| s.sequence() == lost_seq) {
            let pkt_data = slot.data();
            if !pkt_data.is_empty() {
                record_retransmit();
                let _ = self.socket.send_to(pkt_data, self.remote_addr);
            }
        }
    }

    pub fn send_batch_nak(&self, start_seq: u64, end_seq: u64) {
        let mut packet = Vec::with_capacity(ReliableUdpHeader::SIZE + 16);
        let payload = [start_seq.to_le_bytes(), end_seq.to_le_bytes()].concat();
        let mut header = ReliableUdpHeader::new(0, start_seq, MessageType::Nak, 16);
        header.calculate_checksum(&payload);
        // Safe: ReliableUdpHeader derives Pod
        packet.extend_from_slice(bytemuck::bytes_of(&header));
        packet.extend_from_slice(&payload);

        #[cfg(feature = "debug")]
        eprintln!(
            "[NAK-SEND] Sending batch NAK for seq {}-{} to {}",
            start_seq, end_seq, self.remote_nak_addr
        );
        if let Err(e) = self.nak_socket.send_to(&packet, self.remote_nak_addr) {
            #[cfg(feature = "debug")]
            eprintln!("[NAK-SEND-ERROR] Failed to send NAK: {}", e);
        } else {
            #[cfg(feature = "debug")]
            eprintln!("[NAK-SEND-OK] Batch NAK sent successfully");
        }
    }

    /// Send ACK to confirm receipt up to a sequence number
    pub fn send_ack(&self, acked_seq: u64) {
        let mut header = ReliableUdpHeader::new(0, acked_seq, MessageType::Ack, 0);
        header.calculate_checksum(&[]);
        // Safe: ReliableUdpHeader derives Pod
        let packet = bytemuck::bytes_of(&header);

        #[cfg(feature = "debug")]
        eprintln!(
            "[ACK-SEND] Sending ACK for seq {} to {}",
            acked_seq, self.remote_nak_addr
        );
        let _ = self.nak_socket.send_to(packet, self.remote_nak_addr);
    }

    /// Process incoming ACKs and advance send window
    pub fn process_acks(&mut self) {
        let mut buf = [0u8; 256];

        loop {
            match self.nak_socket.recv_from(&mut buf) {
                Ok((len, _src)) => {
                    if len < ReliableUdpHeader::SIZE {
                        continue;
                    }

                    if let Some((header, _payload)) =
                        ReliableUdpHeader::from_packet_with_payload_check(&buf[..len])
                    {
                        if header.msg_type == (MessageType::Ack as u8) {
                            let acked = header.sequence;
                            if acked > self.acked_seq {
                                #[cfg(feature = "debug")]
                                eprintln!(
                                    "[ACK-RECV] Received ACK for seq {}, advancing window",
                                    acked
                                );
                                // Congestion control: ACK received
                                self.congestion.on_ack();
                                self.acked_seq = acked;
                                self.send_window.advance_consumer(0, acked);
                            }
                        } else if header.msg_type == (MessageType::Nak as u8) {
                            // Handle NAK - indicates loss
                            self.congestion.on_loss();
                            record_retransmit();
                            let sequence = header.sequence;
                            self.retransmit(sequence);
                        }
                    }
                }
                Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    break;
                }
                Err(_) => {
                    break;
                }
            }
        }
    }

    /// Process incoming NAKs and retransmit as needed
    pub fn process_naks(&mut self) {
        let mut buf = [0u8; 2048];
        let mut nak_count = 0;
        let mut retransmit_count = 0;

        loop {
            match self.nak_socket.recv_from(&mut buf) {
                Ok((len, src)) => {
                    nak_count += 1;

                    if len < ReliableUdpHeader::SIZE {
                        #[cfg(feature = "debug")]
                        eprintln!(
                            "[NAK] Received invalid NAK (too short: {} bytes) from {}",
                            len, src
                        );
                        continue;
                    }

                    if let Some((header, payload)) =
                        ReliableUdpHeader::from_packet_with_payload_check(&buf[..len])
                    {
                        let msg_type = header.msg_type;

                        if msg_type != (MessageType::Nak as u8) {
                            #[cfg(feature = "debug")]
                            eprintln!("[NAK] Received non-NAK message type: {}", msg_type);
                            continue;
                        }

                        let sequence = header.sequence;

                        if payload.len() >= 16 && payload.len() % 16 == 0 {
                            let range_count = payload.len() / 16;
                            #[cfg(feature = "debug")]
                            eprintln!(
                                "[NAK] Received batch NAK from {} with {} ranges",
                                src, range_count
                            );

                            for i in 0..range_count {
                                let offset = i * 16;
                                let start_seq = u64::from_le_bytes(
                                    payload[offset..offset + 8].try_into().unwrap(),
                                );
                                let end_seq = u64::from_le_bytes(
                                    payload[offset + 8..offset + 16].try_into().unwrap(),
                                );
                                let count = (end_seq - start_seq + 1) as usize;

                                #[cfg(feature = "debug")]
                                eprintln!(
                                    "[NAK] Range {}: seq {}-{} ({} packets)",
                                    i, start_seq, end_seq, count
                                );
                                self.retransmit_batch(start_seq, end_seq);
                                retransmit_count += count;
                            }
                        } else {
                            #[cfg(feature = "debug")]
                            eprintln!(
                                "[NAK] Received single NAK from {} for seq {}",
                                src, sequence
                            );
                            self.retransmit(sequence);
                            retransmit_count += 1;
                        }
                    } else {
                        #[cfg(feature = "debug")]
                        eprintln!("[NAK] Failed to parse NAK header");
                    }
                }
                Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    break;
                }
                Err(e) => {
                    #[cfg(feature = "debug")]
                    eprintln!("[NAK] Socket error: {}", e);
                    break;
                }
            }
        }
    }

    /// Retransmit a batch of lost packets (on batch NAK)
    pub fn retransmit_batch(&mut self, start_seq: u64, end_seq: u64) {
        self.congestion.on_loss(); // Loss event triggers congestion control
        let slots = self.send_window.peek_batch(0, self.window_size);
        for slot in slots.iter().filter(|s| {
            let seq = s.sequence();
            seq >= start_seq && seq <= end_seq
        }) {
            let pkt_data = slot.data();
            if !pkt_data.is_empty() {
                record_retransmit();
                let _ = self.socket.send_to(pkt_data, self.remote_addr);
            }
        }
    }

    /// Get congestion window size
    pub fn congestion_window(&self) -> u32 {
        self.congestion.window_size()
    }

    /// Get packets in flight
    pub fn in_flight(&self) -> u32 {
        self.congestion.in_flight()
    }

    /// Callback-based delivery: process each message with the provided closure.
    pub fn receive_batch_with<F: FnMut(&[u8])>(&mut self, max_count: usize, mut f: F) {
        RECV_BUFFERS.with(|bufs_cell| {
            RECV_LENS.with(|lens_cell| {
                let mut bufs = bufs_cell.borrow_mut();
                let mut lens = lens_cell.borrow_mut();

                let max_recv = max_count.min(bufs.len());
                let mut n = 0;
                for i in 0..max_recv {
                    match self.socket.recv_from(&mut bufs[i]) {
                        Ok((len, _src)) => {
                            lens[i] = len;
                            n += 1;
                        }
                        Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                            break;
                        }
                        Err(e) => {
                            #[cfg(feature = "debug")]
                            eprintln!("[ERR] Socket error: {}", e);
                            break;
                        }
                    }
                }
                for i in 0..n {
                    let len = lens[i];
                    let data = &bufs[i][..len];

                    if len < FastHeader::SIZE {
                        continue;
                    }

                    // Detect format via magic bit in first u32
                    let first_u32 = u32::from_le_bytes([data[0], data[1], data[2], data[3]]);
                    let is_fast = (first_u32 & FAST_HEADER_MAGIC) != 0;

                    if is_fast {
                        // Parse FastHeader format (ultra-fast path)
                        let mut offset = 0;
                        while offset + FastHeader::SIZE <= len {
                            let frame_len_raw = u32::from_le_bytes([
                                data[offset],
                                data[offset + 1],
                                data[offset + 2],
                                data[offset + 3],
                            ]);
                            let frame_len = (frame_len_raw & !FAST_HEADER_MAGIC) as usize;
                            let seq = u32::from_le_bytes([
                                data[offset + 4],
                                data[offset + 5],
                                data[offset + 6],
                                data[offset + 7],
                            ]) as u64;

                            if frame_len >= FastHeader::SIZE && offset + frame_len <= len {
                                let payload = &data[offset + FastHeader::SIZE..offset + frame_len];
                                self.recv_window.insert(seq, payload);
                                offset += frame_len;
                            } else {
                                break;
                            }
                        }
                    } else if first_u32 >= (ReliableUdpHeader::SIZE as u32)
                        && first_u32 < 2000
                        && 4 + (first_u32 as usize) <= len
                    {
                        // Parse standard batch format (fast path with 24-byte header)
                        let mut offset = 0;
                        while offset + 4 <= len {
                            let pkt_len = u32::from_le_bytes([
                                data[offset],
                                data[offset + 1],
                                data[offset + 2],
                                data[offset + 3],
                            ]) as usize;
                            offset += 4;

                            if pkt_len >= ReliableUdpHeader::SIZE && offset + pkt_len <= len {
                                if let Some(header) = ReliableUdpHeader::from_bytes(
                                    &data[offset..offset + ReliableUdpHeader::SIZE],
                                ) {
                                    let payload_len = header.payload_len as usize;
                                    let flags = header.flags;
                                    if pkt_len >= ReliableUdpHeader::SIZE + payload_len {
                                        let payload = &data[offset + ReliableUdpHeader::SIZE
                                            ..offset + ReliableUdpHeader::SIZE + payload_len];
                                        let valid = (flags & FLAG_NO_CRC) != 0
                                            || header.verify_checksum(payload);
                                        if valid {
                                            self.recv_window.insert(header.sequence, payload);
                                        }
                                    }
                                }
                                offset += pkt_len;
                            } else {
                                break;
                            }
                        }
                    } else {
                        // Parse single packet format
                        if let Some(header) =
                            ReliableUdpHeader::from_bytes(&data[..ReliableUdpHeader::SIZE])
                        {
                            let payload_len = header.payload_len as usize;
                            if len >= ReliableUdpHeader::SIZE + payload_len {
                                let payload = &data[ReliableUdpHeader::SIZE
                                    ..ReliableUdpHeader::SIZE + payload_len];
                                if header.verify_checksum(payload) {
                                    self.recv_window.insert(header.sequence, payload);
                                }
                            }
                        }
                    }
                }
                self.recv_window.deliver_in_order_with(|msg| {
                    record_receive(msg.len() as u64);
                    f(msg);
                });

                // Send ACK for highest delivered sequence
                let last_delivered = self.recv_window.last_delivered_seq();
                if last_delivered > 0 {
                    self.send_ack(last_delivered);
                }

                self.recv_window.send_batch_naks_for_gaps(|start, end| {
                    self.send_batch_nak(start, end);
                });
            });
        });
    }
}

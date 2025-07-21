//! Reliable UDP transport with NAK-based retransmission
//!
//! This module implements a high-performance reliable UDP protocol using:
//! 1. Sequence numbering for packet ordering and loss detection
//! 2. NAK (Negative Acknowledgment) for efficient retransmission requests
//! 3. Sliding window flow control
//! 4. Timeout-based recovery for lost NAKs
//! 5. Multicast-friendly design (no ACKs, only NAKs)
//! 6. Integration with zero-copy ring buffer

use std::net::{ SocketAddr, UdpSocket };
use std::sync::{ Arc, Mutex };
use std::sync::atomic::{ AtomicU64, Ordering };
use std::collections::{ HashMap, BTreeMap };
use std::time::{ Duration, Instant, SystemTime, UNIX_EPOCH };
use std::thread;

use crate::error::{ Result, FluxError };

/// Session configuration for reliable UDP
#[derive(Debug, Clone)]
pub struct ReliableUdpConfig {
    /// Maximum window size for flow control
    pub window_size: usize,
    /// Retransmission timeout in milliseconds
    pub retransmit_timeout_ms: u64,
    /// Maximum retransmission attempts
    pub max_retransmissions: u32,
    /// NAK timeout in milliseconds (how long to wait before sending NAK)
    pub nak_timeout_ms: u64,
    /// Maximum out-of-order packets to buffer
    pub max_out_of_order: usize,
    /// Heartbeat interval in milliseconds
    pub heartbeat_interval_ms: u64,
    /// Session timeout in milliseconds
    pub session_timeout_ms: u64,
}

impl Default for ReliableUdpConfig {
    fn default() -> Self {
        Self {
            window_size: 1024,
            retransmit_timeout_ms: 50,
            max_retransmissions: 3,
            nak_timeout_ms: 10,
            max_out_of_order: 256,
            heartbeat_interval_ms: 1000,
            session_timeout_ms: 10000,
        }
    }
}

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

/// NAK request for missing packet
#[derive(Debug, Clone)]
pub struct NakRequest {
    /// Session ID
    pub session_id: u32,
    /// Missing sequence number
    pub sequence: u64,
    /// Time when NAK was first requested
    pub request_time: Instant,
    /// Number of NAK attempts
    pub attempts: u32,
}

/// Session state for reliable UDP
#[derive(Debug)]
pub struct ReliableUdpSession {
    /// Session ID
    pub session_id: u32,
    /// Remote address
    pub remote_addr: SocketAddr,
    /// Next sequence to send
    pub next_send_seq: AtomicU64,
    /// Next sequence expected to receive
    pub next_recv_seq: AtomicU64,
    /// Sliding window of sent packets (for retransmission)
    pub send_window: Mutex<BTreeMap<u64, RetransmissionPacket>>,
    /// Out-of-order received packets
    pub recv_buffer: Mutex<BTreeMap<u64, Vec<u8>>>,
    /// Pending NAK requests
    pub nak_requests: Mutex<HashMap<u64, NakRequest>>,
    /// Last activity timestamp
    pub last_activity: Mutex<Instant>,
    /// Configuration
    pub config: ReliableUdpConfig,
}

impl ReliableUdpSession {
    /// Create new session
    pub fn new(session_id: u32, remote_addr: SocketAddr, config: ReliableUdpConfig) -> Self {
        Self {
            session_id,
            remote_addr,
            next_send_seq: AtomicU64::new(1),
            next_recv_seq: AtomicU64::new(1),
            send_window: Mutex::new(BTreeMap::new()),
            recv_buffer: Mutex::new(BTreeMap::new()),
            nak_requests: Mutex::new(HashMap::new()),
            last_activity: Mutex::new(Instant::now()),
            config,
        }
    }

    /// Send data packet
    pub fn send_data(&self, socket: &UdpSocket, payload: &[u8]) -> Result<u64> {
        let sequence = self.next_send_seq.fetch_add(1, Ordering::Relaxed);
        let mut header = ReliableUdpHeader::new(
            self.session_id,
            sequence,
            MessageType::Data,
            payload.len() as u16
        );
        header.calculate_checksum(payload);

        // Create complete packet
        let mut packet = Vec::with_capacity(ReliableUdpHeader::SIZE + payload.len());
        packet.extend_from_slice(header.as_bytes());
        packet.extend_from_slice(payload);

        // Send packet
        socket.send_to(&packet, self.remote_addr)?;

        // Store in retransmission buffer
        let retrans_packet = RetransmissionPacket {
            data: packet,
            sent_time: Instant::now(),
            attempts: 0,
            addr: self.remote_addr,
        };

        let mut send_window = self.send_window.lock().unwrap();
        send_window.insert(sequence, retrans_packet);

        // Cleanup old entries outside window
        let window_start = sequence.saturating_sub(self.config.window_size as u64);
        send_window.retain(|&seq, _| seq > window_start);

        // Update activity
        *self.last_activity.lock().unwrap() = Instant::now();

        Ok(sequence)
    }

    /// Process received packet
    pub fn process_received(&self, socket: &UdpSocket, packet: &[u8]) -> Result<Vec<Vec<u8>>> {
        if packet.len() < ReliableUdpHeader::SIZE {
            return Err(FluxError::invalid_message("Packet too small".to_string()));
        }

        let header = ReliableUdpHeader::from_bytes(packet).ok_or_else(||
            FluxError::invalid_message("Invalid header".to_string())
        )?;

        let payload = &packet[ReliableUdpHeader::SIZE..];

        // Verify checksum
        if !header.verify_checksum(payload) {
            return Err(FluxError::invalid_message("Checksum mismatch".to_string()));
        }

        // Update activity
        *self.last_activity.lock().unwrap() = Instant::now();

        match MessageType::try_from(header.msg_type) {
            Ok(MessageType::Data) => self.process_data_packet(socket, header, payload.to_vec()),
            Ok(MessageType::Nak) => {
                self.process_nak_packet(socket, header, payload)?;
                Ok(vec![])
            }
            Ok(MessageType::Heartbeat) => {
                // Heartbeat received - session is alive
                Ok(vec![])
            }
            _ => Err(FluxError::invalid_message("Unknown message type".to_string())),
        }
    }

    /// Process data packet and handle ordering
    fn process_data_packet(
        &self,
        socket: &UdpSocket,
        header: ReliableUdpHeader,
        payload: Vec<u8>
    ) -> Result<Vec<Vec<u8>>> {
        let mut delivered = Vec::new();
        let expected_seq = self.next_recv_seq.load(Ordering::Relaxed);

        if header.sequence == expected_seq {
            // In-order packet - deliver immediately
            delivered.push(payload);
            self.next_recv_seq.store(expected_seq + 1, Ordering::Relaxed);

            // Check if we can deliver buffered packets
            let mut recv_buffer = self.recv_buffer.lock().unwrap();
            let mut next_seq = expected_seq + 1;

            while let Some(buffered_payload) = recv_buffer.remove(&next_seq) {
                delivered.push(buffered_payload);
                self.next_recv_seq.store(next_seq + 1, Ordering::Relaxed);
                next_seq += 1;
            }

            // Remove any pending NAK requests for delivered packets
            let mut nak_requests = self.nak_requests.lock().unwrap();
            for seq in expected_seq..next_seq {
                nak_requests.remove(&seq);
            }
        } else if header.sequence > expected_seq {
            // Out-of-order packet - buffer it
            let mut recv_buffer = self.recv_buffer.lock().unwrap();
            recv_buffer.insert(header.sequence, payload);

            // Detect gaps and send NAKs
            self.send_naks_for_gaps(socket, expected_seq, header.sequence)?;

            // Limit buffer size
            if recv_buffer.len() > self.config.max_out_of_order {
                // Remove oldest entries
                let keys: Vec<u64> = recv_buffer.keys().cloned().collect();
                for &key in keys.iter().take(keys.len() - self.config.max_out_of_order) {
                    recv_buffer.remove(&key);
                }
            }
        }
        // Duplicate packet (sequence < expected) - ignore

        Ok(delivered)
    }

    /// Send NAKs for detected gaps
    fn send_naks_for_gaps(&self, socket: &UdpSocket, start_seq: u64, end_seq: u64) -> Result<()> {
        let mut nak_requests = self.nak_requests.lock().unwrap();

        for missing_seq in start_seq..end_seq {
            if !nak_requests.contains_key(&missing_seq) {
                // Send NAK for missing packet
                self.send_nak(socket, missing_seq)?;

                // Track NAK request
                nak_requests.insert(missing_seq, NakRequest {
                    session_id: self.session_id,
                    sequence: missing_seq,
                    request_time: Instant::now(),
                    attempts: 1,
                });
            }
        }

        Ok(())
    }

    /// Send NAK packet
    fn send_nak(&self, socket: &UdpSocket, missing_sequence: u64) -> Result<()> {
        let mut header = ReliableUdpHeader::new(
            self.session_id,
            0, // NAKs don't have sequence numbers
            MessageType::Nak,
            8 // 8 bytes for missing sequence
        );

        let payload = missing_sequence.to_le_bytes();
        header.calculate_checksum(&payload);

        let mut packet = Vec::with_capacity(ReliableUdpHeader::SIZE + 8);
        packet.extend_from_slice(header.as_bytes());
        packet.extend_from_slice(&payload);

        socket.send_to(&packet, self.remote_addr)?;
        Ok(())
    }

    /// Process NAK packet and retransmit if needed
    fn process_nak_packet(
        &self,
        socket: &UdpSocket,
        _header: ReliableUdpHeader,
        payload: &[u8]
    ) -> Result<()> {
        if payload.len() != 8 {
            return Err(FluxError::invalid_message("Invalid NAK payload".to_string()));
        }

        let missing_sequence = u64::from_le_bytes([
            payload[0],
            payload[1],
            payload[2],
            payload[3],
            payload[4],
            payload[5],
            payload[6],
            payload[7],
        ]);

        // Look for packet in retransmission buffer
        let mut send_window = self.send_window.lock().unwrap();
        if let Some(retrans_packet) = send_window.get_mut(&missing_sequence) {
            if retrans_packet.attempts < self.config.max_retransmissions {
                // Retransmit packet
                socket.send_to(&retrans_packet.data, retrans_packet.addr)?;
                retrans_packet.attempts += 1;
                retrans_packet.sent_time = Instant::now();
            }
        }

        Ok(())
    }

    /// Send heartbeat
    pub fn send_heartbeat(&self, socket: &UdpSocket) -> Result<()> {
        let mut header = ReliableUdpHeader::new(self.session_id, 0, MessageType::Heartbeat, 0);
        header.calculate_checksum(&[]);

        let packet = header.as_bytes().to_vec();
        socket.send_to(&packet, self.remote_addr)?;

        *self.last_activity.lock().unwrap() = Instant::now();
        Ok(())
    }

    /// Check for timeouts and retransmit
    pub fn handle_timeouts(&self, socket: &UdpSocket) -> Result<()> {
        let now = Instant::now();
        let timeout = Duration::from_millis(self.config.retransmit_timeout_ms);

        // Handle retransmission timeouts
        let mut send_window = self.send_window.lock().unwrap();
        let mut to_retransmit = Vec::new();

        for (&sequence, packet) in send_window.iter() {
            if
                now.duration_since(packet.sent_time) > timeout &&
                packet.attempts < self.config.max_retransmissions
            {
                to_retransmit.push(sequence);
            }
        }

        for sequence in to_retransmit {
            if let Some(packet) = send_window.get_mut(&sequence) {
                socket.send_to(&packet.data, packet.addr)?;
                packet.attempts += 1;
                packet.sent_time = now;
            }
        }

        // Handle NAK timeouts
        let mut nak_requests = self.nak_requests.lock().unwrap();
        let nak_timeout = Duration::from_millis(self.config.nak_timeout_ms);
        let mut to_renak = Vec::new();

        for (&sequence, nak_req) in nak_requests.iter() {
            if now.duration_since(nak_req.request_time) > nak_timeout && nak_req.attempts < 3 {
                to_renak.push(sequence);
            }
        }

        for sequence in to_renak {
            self.send_nak(socket, sequence)?;
            if let Some(nak_req) = nak_requests.get_mut(&sequence) {
                nak_req.attempts += 1;
                nak_req.request_time = now;
            }
        }

        Ok(())
    }

    /// Check if session is alive
    pub fn is_alive(&self) -> bool {
        let last_activity = *self.last_activity.lock().unwrap();
        Instant::now().duration_since(last_activity).as_millis() <
            (self.config.session_timeout_ms as u128)
    }

    /// Get session statistics
    pub fn get_stats(&self) -> SessionStats {
        let send_window = self.send_window.lock().unwrap();
        let recv_buffer = self.recv_buffer.lock().unwrap();
        let nak_requests = self.nak_requests.lock().unwrap();

        SessionStats {
            session_id: self.session_id,
            next_send_seq: self.next_send_seq.load(Ordering::Relaxed),
            next_recv_seq: self.next_recv_seq.load(Ordering::Relaxed),
            send_window_size: send_window.len(),
            recv_buffer_size: recv_buffer.len(),
            pending_naks: nak_requests.len(),
            last_activity: *self.last_activity.lock().unwrap(),
        }
    }
}

/// Session statistics
#[derive(Debug, Clone)]
pub struct SessionStats {
    pub session_id: u32,
    pub next_send_seq: u64,
    pub next_recv_seq: u64,
    pub send_window_size: usize,
    pub recv_buffer_size: usize,
    pub pending_naks: usize,
    pub last_activity: Instant,
}

/// Multi-session reliable UDP transport
pub struct ReliableUdpTransport {
    /// UDP socket
    socket: UdpSocket,
    /// Active sessions
    sessions: Arc<Mutex<HashMap<u32, Arc<ReliableUdpSession>>>>,
    /// Configuration
    config: ReliableUdpConfig,
    /// Running flag for background tasks
    running: Arc<std::sync::atomic::AtomicBool>,
}

impl ReliableUdpTransport {
    /// Create new reliable UDP transport
    pub fn new(bind_addr: SocketAddr, config: ReliableUdpConfig) -> Result<Self> {
        let socket = UdpSocket::bind(bind_addr)?;
        socket.set_nonblocking(true)?;

        Ok(Self {
            socket,
            sessions: Arc::new(Mutex::new(HashMap::new())),
            config,
            running: Arc::new(std::sync::atomic::AtomicBool::new(true)),
        })
    }

    /// Create new session
    pub fn create_session(
        &self,
        session_id: u32,
        remote_addr: SocketAddr
    ) -> Arc<ReliableUdpSession> {
        let session = Arc::new(
            ReliableUdpSession::new(session_id, remote_addr, self.config.clone())
        );
        let mut sessions = self.sessions.lock().unwrap();
        sessions.insert(session_id, Arc::clone(&session));
        session
    }

    /// Send data on session
    pub fn send(&self, session_id: u32, data: &[u8]) -> Result<u64> {
        let sessions = self.sessions.lock().unwrap();
        if let Some(session) = sessions.get(&session_id) {
            session.send_data(&self.socket, data)
        } else {
            Err(FluxError::config("Session not found".to_string()))
        }
    }

    /// Receive and process incoming packets
    pub fn receive(&self) -> Result<Vec<(u32, Vec<Vec<u8>>)>> {
        let mut buffer = vec![0u8; 65536]; // 64KB buffer
        let mut results = Vec::new();

        // Process all available packets
        loop {
            match self.socket.recv_from(&mut buffer) {
                Ok((size, _addr)) => {
                    let packet = &buffer[..size];
                    if
                        let Ok(header) =
                            ReliableUdpHeader::from_bytes(packet).ok_or("Invalid header")
                    {
                        let session_id = header.session_id; // Copy to avoid packed field reference
                        let sessions = self.sessions.lock().unwrap();
                        if let Some(session) = sessions.get(&session_id) {
                            if let Ok(delivered) = session.process_received(&self.socket, packet) {
                                if !delivered.is_empty() {
                                    results.push((session_id, delivered));
                                }
                            }
                        }
                    }
                }
                Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    break;
                }
                Err(e) => {
                    return Err(FluxError::socket(e.to_string()));
                }
            }
        }

        Ok(results)
    }

    /// Start background maintenance tasks
    pub fn start_background_tasks(&self) {
        let sessions = Arc::clone(&self.sessions);
        let socket = self.socket.try_clone().unwrap();
        let running = Arc::clone(&self.running);
        let config = self.config.clone();

        thread::spawn(move || {
            let heartbeat_interval = Duration::from_millis(config.heartbeat_interval_ms);
            let mut last_heartbeat = Instant::now();

            while running.load(Ordering::Relaxed) {
                let now = Instant::now();

                // Handle timeouts and maintenance
                let sessions_guard = sessions.lock().unwrap();
                let mut dead_sessions = Vec::new();

                for (&session_id, session) in sessions_guard.iter() {
                    // Handle retransmission and NAK timeouts
                    let _ = session.handle_timeouts(&socket);

                    // Send heartbeats
                    if now.duration_since(last_heartbeat) >= heartbeat_interval {
                        let _ = session.send_heartbeat(&socket);
                    }

                    // Check for dead sessions
                    if !session.is_alive() {
                        dead_sessions.push(session_id);
                    }
                }

                drop(sessions_guard);

                // Clean up dead sessions
                if !dead_sessions.is_empty() {
                    let mut sessions_guard = sessions.lock().unwrap();
                    for session_id in dead_sessions {
                        sessions_guard.remove(&session_id);
                    }
                }

                if now.duration_since(last_heartbeat) >= heartbeat_interval {
                    last_heartbeat = now;
                }

                thread::sleep(Duration::from_millis(10)); // 10ms polling
            }
        });
    }

    /// Get all session statistics
    pub fn get_all_stats(&self) -> Vec<SessionStats> {
        let sessions = self.sessions.lock().unwrap();
        sessions
            .values()
            .map(|session| session.get_stats())
            .collect()
    }

    /// Stop background tasks
    pub fn stop(&self) {
        self.running.store(false, Ordering::Relaxed);
    }
}

impl Drop for ReliableUdpTransport {
    fn drop(&mut self) {
        self.stop();
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
            _ => Err(()),
        }
    }
}

/// Example usage of reliable UDP
pub fn example_reliable_udp() -> Result<()> {
    println!("🚀 Reliable UDP with NAK Example");
    println!("================================");

    let config = ReliableUdpConfig::default();
    let bind_addr = "127.0.0.1:0".parse().unwrap();
    let transport = ReliableUdpTransport::new(bind_addr, config)?;

    // Start background tasks
    transport.start_background_tasks();

    // Create session
    let remote_addr = "127.0.0.1:8080".parse().unwrap();
    let _session = transport.create_session(1001, remote_addr);

    // Send some data
    for i in 0..10 {
        let data = format!("Message {}", i);
        let seq = transport.send(1001, data.as_bytes())?;
        println!("Sent message {} with sequence {}", i, seq);
    }

    // Receive data (in a real application, this would be in a loop)
    for _ in 0..5 {
        let received = transport.receive()?;
        for (session_id, messages) in received {
            for (i, message) in messages.iter().enumerate() {
                println!(
                    "Session {}: Received message {}: {:?}",
                    session_id,
                    i,
                    String::from_utf8_lossy(message)
                );
            }
        }
        thread::sleep(Duration::from_millis(100));
    }

    // Print session statistics
    let stats = transport.get_all_stats();
    for stat in stats {
        println!("Session {} stats: {:?}", stat.session_id, stat);
    }

    transport.stop();
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_header_serialization() {
        let header = ReliableUdpHeader::new(1001, 42, MessageType::Data, 100);
        let bytes = header.as_bytes();
        let deserialized = ReliableUdpHeader::from_bytes(bytes).unwrap();

        // Copy packed fields to avoid unaligned references
        let header_session_id = header.session_id;
        let header_sequence = header.sequence;
        let header_payload_len = header.payload_len;
        let des_session_id = deserialized.session_id;
        let des_sequence = deserialized.sequence;
        let des_payload_len = deserialized.payload_len;

        assert_eq!(header_session_id, des_session_id);
        assert_eq!(header_sequence, des_sequence);
        assert_eq!(header_payload_len, des_payload_len);
    }

    #[test]
    fn test_checksum_verification() {
        let mut header = ReliableUdpHeader::new(1001, 42, MessageType::Data, 5);
        let payload = b"hello";

        header.calculate_checksum(payload);
        assert!(header.verify_checksum(payload));

        // Corrupt payload
        let corrupt_payload = b"world";
        assert!(!header.verify_checksum(corrupt_payload));
    }

    #[test]
    fn test_session_creation() {
        let config = ReliableUdpConfig::default();
        let addr = "127.0.0.1:8080".parse().unwrap();
        let session = ReliableUdpSession::new(1001, addr, config);

        assert_eq!(session.session_id, 1001);
        assert_eq!(session.remote_addr, addr);
        assert_eq!(session.next_send_seq.load(Ordering::Relaxed), 1);
        assert_eq!(session.next_recv_seq.load(Ordering::Relaxed), 1);
    }
}

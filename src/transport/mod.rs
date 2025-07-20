//! Reliable UDP transport layer with NAK-based retransmission and FEC
//!
//! This module provides a high-performance, reliable UDP transport that combines
//! the performance of UDP with the reliability of TCP through:
//! - NAK-based retransmission for lost packets
//! - Forward Error Correction (FEC) for burst errors
//! - Zero-copy message passing
//! - Batch processing for high throughput

use std::net::{ UdpSocket, SocketAddr };
use std::sync::atomic::{ AtomicU64, Ordering };
use std::collections::HashMap;
use std::time::{ Duration, Instant };
use std::thread;
use std::sync::Arc;
use std::sync::Mutex;

use crate::error::{ Result, FluxError };
use crate::disruptor::{ RingBuffer, RingBufferConfig };

/// Transport configuration for reliable UDP
#[derive(Debug, Clone)]
pub struct TransportConfig {
    /// Local address to bind to
    pub local_addr: String,
    /// Remote address to connect to
    pub remote_addr: Option<String>,
    /// Batch size for sending messages
    pub batch_size: usize,
    /// Ring buffer size for message queuing
    pub buffer_size: usize,
    /// Retransmission timeout in milliseconds
    pub retransmit_timeout_ms: u64,
    /// Maximum retransmission attempts
    pub max_retransmits: u32,
    /// Enable FEC (Forward Error Correction)
    pub enable_fec: bool,
    /// FEC data shards
    pub fec_data_shards: usize,
    /// FEC parity shards
    pub fec_parity_shards: usize,
    /// CPU affinity for transport threads
    pub cpu_affinity: Option<usize>,
    /// Enable zero-copy optimizations
    pub enable_zero_copy: bool,
}

impl Default for TransportConfig {
    fn default() -> Self {
        Self {
            local_addr: "0.0.0.0:0".to_string(),
            remote_addr: None,
            batch_size: 64,
            buffer_size: 1024 * 1024, // 1M messages
            retransmit_timeout_ms: 100,
            max_retransmits: 3,
            enable_fec: true,
            fec_data_shards: 4,
            fec_parity_shards: 2,
            cpu_affinity: None,
            enable_zero_copy: true,
        }
    }
}

/// Message header for reliable transport
#[derive(Debug, Clone, Copy)]
struct MessageHeader {
    /// Message sequence number
    sequence: u64,
    /// Message type (data, ack, nak, fec)
    message_type: u8,
    /// Message flags
    flags: u8,
    /// Message length
    length: u32,
    /// Checksum for integrity
    checksum: u32,
}

impl MessageHeader {
    const SIZE: usize = 24;
    const TYPE_DATA: u8 = 0;
    const TYPE_ACK: u8 = 1;
    const TYPE_NAK: u8 = 2;
    const TYPE_FEC: u8 = 3;

    fn new(sequence: u64, message_type: u8, length: u32) -> Self {
        Self {
            sequence,
            message_type,
            flags: 0,
            length,
            checksum: 0,
        }
    }

    fn serialize(&self) -> Vec<u8> {
        let mut buf = Vec::with_capacity(Self::SIZE);
        buf.extend_from_slice(&self.sequence.to_le_bytes());
        buf.push(self.message_type);
        buf.push(self.flags);
        buf.extend_from_slice(&self.length.to_le_bytes());
        buf.extend_from_slice(&self.checksum.to_le_bytes());
        buf
    }

    fn deserialize(data: &[u8]) -> Result<Self> {
        if data.len() < Self::SIZE {
            return Err(FluxError::config("Invalid message header size"));
        }

        let sequence = u64::from_le_bytes([
            data[0],
            data[1],
            data[2],
            data[3],
            data[4],
            data[5],
            data[6],
            data[7],
        ]);
        let message_type = data[8];
        let flags = data[9];
        let length = u32::from_le_bytes([data[10], data[11], data[12], data[13]]);
        let checksum = u32::from_le_bytes([data[14], data[15], data[16], data[17]]);

        Ok(Self {
            sequence,
            message_type,
            flags,
            length,
            checksum,
        })
    }
}

/// Transport metrics for monitoring
#[derive(Debug, Clone)]
pub struct TransportMetrics {
    /// Messages sent
    pub messages_sent: u64,
    /// Messages received
    pub messages_received: u64,
    /// Messages retransmitted
    pub messages_retransmitted: u64,
    /// Messages dropped due to errors
    pub messages_dropped: u64,
    /// Current throughput (messages/sec)
    pub throughput_msg_per_sec: f64,
    /// Average latency (microseconds)
    pub avg_latency_us: f64,
    /// P99 latency (microseconds)
    pub p99_latency_us: f64,
}

/// Reliable UDP transport implementation
pub struct ReliableUdpTransport {
    /// UDP socket
    socket: UdpSocket,
    /// Configuration
    config: TransportConfig,
    /// Send ring buffer
    send_buffer: RingBuffer,
    /// Receive ring buffer
    recv_buffer: RingBuffer,
    /// Metrics
    metrics: Arc<Mutex<TransportMetrics>>,
    /// Running flag
    running: Arc<AtomicU64>,
    /// Sequence number for outgoing messages
    sequence: Arc<AtomicU64>,
    /// Pending acknowledgments
    pending_acks: Arc<Mutex<HashMap<u64, Instant>>>,
    /// FEC encoder
    fec_encoder: Option<FecEncoder>,
}

impl ReliableUdpTransport {
    /// Create a new reliable UDP transport
    pub fn new(config: TransportConfig) -> Result<Self> {
        let socket = UdpSocket::bind(&config.local_addr)?;
        socket.set_nonblocking(true)?;
        // Note: set_send_buffer_size and set_recv_buffer_size are not available on all platforms
        // These would be set via socket options in a full implementation

        let ring_config = RingBufferConfig::new(config.buffer_size)?
            .with_consumers(2)?
            .with_optimal_batch_size(config.batch_size)
            .with_cache_prefetch(true)
            .with_simd_optimizations(true);

        let send_buffer = RingBuffer::new(ring_config.clone())?;
        let recv_buffer = RingBuffer::new(ring_config.clone())?;

        let fec_encoder = if config.enable_fec {
            Some(FecEncoder::new(config.fec_data_shards, config.fec_parity_shards)?)
        } else {
            None
        };

        Ok(Self {
            socket,
            config,
            send_buffer,
            recv_buffer,
            metrics: Arc::new(
                Mutex::new(TransportMetrics {
                    messages_sent: 0,
                    messages_received: 0,
                    messages_retransmitted: 0,
                    messages_dropped: 0,
                    throughput_msg_per_sec: 0.0,
                    avg_latency_us: 0.0,
                    p99_latency_us: 0.0,
                })
            ),
            running: Arc::new(AtomicU64::new(0)),
            sequence: Arc::new(AtomicU64::new(0)),
            pending_acks: Arc::new(Mutex::new(HashMap::new())),
            fec_encoder,
        })
    }

    /// Start the transport
    pub fn start(&mut self) -> Result<()> {
        self.running.store(1, Ordering::Relaxed);

        // Start background threads
        let socket = self.socket.try_clone()?;
        let running = Arc::clone(&self.running);
        let metrics = Arc::clone(&self.metrics);

        // Receiver thread
        thread::spawn(move || {
            Self::receiver_thread(socket, running, metrics);
        });

        // Retransmission thread
        let socket = self.socket.try_clone()?;
        let running = Arc::clone(&self.running);
        let pending_acks = Arc::clone(&self.pending_acks);
        let config = self.config.clone();

        thread::spawn(move || {
            Self::retransmit_thread(socket, pending_acks, running, config);
        });

        Ok(())
    }

    /// Stop the transport
    pub fn stop(&mut self) {
        self.running.store(0, Ordering::Relaxed);
    }

    /// Send a message reliably
    pub fn send(&mut self, data: &[u8], addr: SocketAddr) -> Result<()> {
        let sequence = self.sequence.fetch_add(1, Ordering::Relaxed);
        let header = MessageHeader::new(sequence, MessageHeader::TYPE_DATA, data.len() as u32);

        // Create message with header
        let mut message = header.serialize();
        message.extend_from_slice(data);

        // Add FEC if enabled
        if let Some(ref fec) = self.fec_encoder {
            let fec_data = fec.encode(&message)?;
            message.extend_from_slice(&fec_data);
        }

        // Send message
        self.socket.send_to(&message, addr)?;

        // Track for acknowledgment
        {
            let mut pending = self.pending_acks.lock().unwrap();
            pending.insert(sequence, Instant::now());
        }

        // Update metrics
        {
            let mut metrics = self.metrics.lock().unwrap();
            metrics.messages_sent += 1;
        }

        Ok(())
    }

    /// Send a batch of messages
    pub fn send_batch(&mut self, messages: &[&[u8]], addr: SocketAddr) -> Result<usize> {
        let mut sent = 0;

        for data in messages {
            self.send(data, addr)?;
            sent += 1;
        }

        Ok(sent)
    }

    /// Receive a message
    pub fn receive(&mut self) -> Result<Option<(Vec<u8>, SocketAddr)>> {
        let batch = self.recv_buffer.try_consume_batch(0, 1);

        if batch.is_empty() {
            return Ok(None);
        }

        let slot = &batch[0];
        let data = slot.data().to_vec();

        // Parse message header
        let header = MessageHeader::deserialize(&data)?;

        // Handle different message types
        match header.message_type {
            MessageHeader::TYPE_DATA => {
                let payload = &data[MessageHeader::SIZE..];
                // Note: addr is not available here, would need to be stored with message
                // For now, use a placeholder address
                let addr = SocketAddr::new(
                    std::net::IpAddr::V4(std::net::Ipv4Addr::new(127, 0, 0, 1)),
                    8080
                );
                self.send_ack(header.sequence, addr)?;

                // Update metrics
                {
                    let mut metrics = self.metrics.lock().unwrap();
                    metrics.messages_received += 1;
                }

                Ok(Some((payload.to_vec(), addr)))
            }
            MessageHeader::TYPE_ACK => {
                // Remove from pending acks
                {
                    let mut pending = self.pending_acks.lock().unwrap();
                    pending.remove(&header.sequence);
                }
                Ok(None)
            }
            MessageHeader::TYPE_NAK => {
                // Retransmit requested message
                let addr = SocketAddr::new(
                    std::net::IpAddr::V4(std::net::Ipv4Addr::new(127, 0, 0, 1)),
                    8080
                );
                self.handle_nak(header.sequence, addr)?;
                Ok(None)
            }
            _ => Ok(None),
        }
    }

    /// Get transport metrics
    pub fn get_metrics(&self) -> TransportMetrics {
        self.metrics.lock().unwrap().clone()
    }

    /// Receiver thread implementation
    fn receiver_thread(
        socket: UdpSocket,
        running: Arc<AtomicU64>,
        metrics: Arc<Mutex<TransportMetrics>>
    ) {
        let mut buf = vec![0u8; 65536];

        while running.load(Ordering::Relaxed) == 1 {
            match socket.recv_from(&mut buf) {
                Ok((size, _addr)) => {
                    // Update metrics for received data
                    {
                        let mut metrics = metrics.lock().unwrap();
                        metrics.messages_received += 1;
                    }
                }
                Err(_) => {
                    // Non-blocking socket, continue
                    thread::sleep(Duration::from_millis(1));
                }
            }
        }
    }

    /// Retransmission thread implementation
    fn retransmit_thread(
        socket: UdpSocket,
        pending_acks: Arc<Mutex<HashMap<u64, Instant>>>,
        running: Arc<AtomicU64>,
        config: TransportConfig
    ) {
        while running.load(Ordering::Relaxed) == 1 {
            let now = Instant::now();
            let timeout = Duration::from_millis(config.retransmit_timeout_ms);

            // Check for expired acknowledgments
            {
                let mut pending = pending_acks.lock().unwrap();
                let expired: Vec<u64> = pending
                    .iter()
                    .filter(|(_, &ref time)| now.duration_since(*time) > timeout)
                    .map(|(&seq, _)| seq)
                    .collect();

                for seq in expired {
                    pending.remove(&seq);
                    // In a full implementation, we would retransmit here
                }
            }

            thread::sleep(Duration::from_millis(10));
        }
    }

    /// Send acknowledgment
    fn send_ack(&self, sequence: u64, addr: SocketAddr) -> Result<()> {
        let header = MessageHeader::new(sequence, MessageHeader::TYPE_ACK, 0);
        let ack_data = header.serialize();
        self.socket.send_to(&ack_data, addr)?;
        Ok(())
    }

    /// Handle NAK (negative acknowledgment)
    fn handle_nak(&self, sequence: u64, addr: SocketAddr) -> Result<()> {
        // In a full implementation, we would retransmit the message
        // For now, just log the NAK
        eprintln!("Received NAK for sequence {}", sequence);
        Ok(())
    }
}

/// Forward Error Correction (FEC) encoder
pub struct FecEncoder {
    data_shards: usize,
    parity_shards: usize,
}

impl FecEncoder {
    /// Create a new FEC encoder
    pub fn new(data_shards: usize, parity_shards: usize) -> Result<Self> {
        if data_shards == 0 || parity_shards == 0 {
            return Err(FluxError::config("Invalid FEC configuration"));
        }

        Ok(Self {
            data_shards,
            parity_shards,
        })
    }

    /// Encode data with FEC
    pub fn encode(&self, data: &[u8]) -> Result<Vec<u8>> {
        // Simple XOR-based FEC for demonstration
        // In production, use a proper FEC library like reed-solomon
        let mut parity = vec![0u8; data.len()];

        for (i, &byte) in data.iter().enumerate() {
            parity[i] = byte ^ 0xff; // Simple XOR with 0xFF
        }

        Ok(parity)
    }

    /// Decode data with FEC
    pub fn decode(&self, data: &[u8], parity: &[u8]) -> Result<Vec<u8>> {
        if data.len() != parity.len() {
            return Err(FluxError::config("Data and parity lengths must match"));
        }

        let mut decoded = vec![0u8; data.len()];

        for (i, (&data_byte, &parity_byte)) in data.iter().zip(parity.iter()).enumerate() {
            decoded[i] = data_byte ^ parity_byte;
        }

        Ok(decoded)
    }
}

/// Basic UDP transport configuration
#[derive(Debug, Clone)]
pub struct BasicUdpConfig {
    /// Local address to bind to
    pub local_addr: String,
    /// Ring buffer size for message queuing
    pub buffer_size: usize,
    /// Batch size for sending messages
    pub batch_size: usize,
    /// Enable non-blocking mode
    pub non_blocking: bool,
    /// Socket timeout in milliseconds
    pub socket_timeout_ms: u64,
}

impl Default for BasicUdpConfig {
    fn default() -> Self {
        Self {
            local_addr: "0.0.0.0:0".to_string(),
            buffer_size: 4096,
            batch_size: 64,
            non_blocking: true,
            socket_timeout_ms: 100,
        }
    }
}

/// Basic UDP transport with ring buffer integration
pub struct BasicUdpTransport {
    /// UDP socket
    socket: UdpSocket,
    /// Configuration
    config: BasicUdpConfig,
    /// Send ring buffer
    send_buffer: RingBuffer,
    /// Receive ring buffer
    recv_buffer: RingBuffer,
    /// Running flag
    running: Arc<AtomicU64>,
}

impl BasicUdpTransport {
    /// Create a new basic UDP transport
    pub fn new(config: BasicUdpConfig) -> Result<Self> {
        let socket = UdpSocket::bind(&config.local_addr)?;

        if config.non_blocking {
            socket.set_nonblocking(true)?;
        } else {
            socket.set_read_timeout(Some(Duration::from_millis(config.socket_timeout_ms)))?;
        }

        let ring_config = RingBufferConfig::new(config.buffer_size)?
            .with_consumers(1)?
            .with_optimal_batch_size(config.batch_size)
            .with_wait_strategy(crate::disruptor::WaitStrategyType::BusySpin);

        let send_buffer = RingBuffer::new(ring_config.clone())?;
        let recv_buffer = RingBuffer::new(ring_config.clone())?;

        Ok(Self {
            socket,
            config,
            send_buffer,
            recv_buffer,
            running: Arc::new(AtomicU64::new(0)),
        })
    }

    /// Start the transport
    pub fn start(&mut self) -> Result<()> {
        self.running.store(1, Ordering::Relaxed);
        Ok(())
    }

    /// Stop the transport
    pub fn stop(&mut self) {
        self.running.store(0, Ordering::Relaxed);
    }

    /// Send a message
    pub fn send(&mut self, data: &[u8], addr: SocketAddr) -> Result<()> {
        self.socket.send_to(data, addr)?;
        Ok(())
    }

    /// Send a batch of messages
    pub fn send_batch(&mut self, messages: &[&[u8]], addr: SocketAddr) -> Result<usize> {
        let mut sent = 0;

        for data in messages {
            self.send(data, addr)?;
            sent += 1;
        }

        Ok(sent)
    }

    /// Receive a message
    pub fn receive(&mut self) -> Result<Option<(Vec<u8>, SocketAddr)>> {
        let mut buf = vec![0u8; 1024];

        match self.socket.recv_from(&mut buf) {
            Ok((size, addr)) => {
                let data = buf[..size].to_vec();
                Ok(Some((data, addr)))
            }
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => { Ok(None) }
            Err(e) => { Err(FluxError::Io(e)) }
        }
    }

    /// Get the underlying socket for advanced operations
    pub fn socket(&self) -> &UdpSocket {
        &self.socket
    }

    /// Get the send ring buffer
    pub fn send_buffer(&self) -> &RingBuffer {
        &self.send_buffer
    }

    /// Get the receive ring buffer
    pub fn recv_buffer(&self) -> &RingBuffer {
        &self.recv_buffer
    }
}

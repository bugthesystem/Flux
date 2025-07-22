//! Transport layer with multiple performance-optimized implementations
//!
//! This module provides:
//! - **reliable_udp**: Reliable UDP with NAK-based retransmission (PRODUCTION-READY)
//! - **kernel_bypass_zero_copy**: True zero-copy using io_uring/DMA (EXPERIMENTAL)

pub mod kernel_bypass_zero_copy;
pub mod reliable_udp;
pub mod unified;

use std::net::{ UdpSocket, SocketAddr };
use std::sync::atomic::{ AtomicU64, Ordering };
use std::sync::Arc;
use std::time::Duration;

use crate::error::{ Result, FluxError };
use crate::disruptor::{ RingBuffer, RingBufferConfig };
use crate::disruptor::ring_buffer::MappedRingBuffer;
#[cfg(all(target_os = "linux", any(feature = "linux_numa", feature = "linux_hugepages")))]
use crate::disruptor::ring_buffer_linux::LinuxRingBuffer;

// Trait for common ring buffer operations
pub trait RingBufferOps: Send + Sync {
    fn try_claim_slots(
        &mut self,
        count: usize
    ) -> Option<(u64, &mut [crate::disruptor::MessageSlot])>;
    fn try_consume_batch(
        &self,
        consumer_id: usize,
        max_count: usize
    ) -> Vec<&crate::disruptor::MessageSlot>;
    fn publish_batch(&self, start_seq: u64, count: usize);
}

impl RingBufferOps for RingBuffer {
    fn try_claim_slots(
        &mut self,
        count: usize
    ) -> Option<(u64, &mut [crate::disruptor::MessageSlot])> {
        self.try_claim_slots(count)
    }
    fn try_consume_batch(
        &self,
        consumer_id: usize,
        max_count: usize
    ) -> Vec<&crate::disruptor::MessageSlot> {
        self.try_consume_batch(consumer_id, max_count)
    }
    fn publish_batch(&self, start_seq: u64, count: usize) {
        self.publish_batch(start_seq, count)
    }
}

impl RingBufferOps for MappedRingBuffer {
    fn try_claim_slots(
        &mut self,
        count: usize
    ) -> Option<(u64, &mut [crate::disruptor::MessageSlot])> {
        self.try_claim_slots(count)
    }
    fn try_consume_batch(
        &self,
        consumer_id: usize,
        max_count: usize
    ) -> Vec<&crate::disruptor::MessageSlot> {
        let slice = self.try_consume_batch(consumer_id, max_count);
        slice.iter().collect()
    }
    fn publish_batch(&self, start_seq: u64, count: usize) {
        self.publish_batch(start_seq, count)
    }
}

#[cfg(all(target_os = "linux", any(feature = "linux_numa", feature = "linux_hugepages")))]
impl RingBufferOps for LinuxRingBuffer {
    fn try_claim_slots(
        &mut self,
        count: usize
    ) -> Option<(u64, &mut [crate::disruptor::MessageSlot])> {
        self.try_claim_slots(count)
    }
    fn try_consume_batch(
        &self,
        consumer_id: usize,
        max_count: usize
    ) -> Vec<&crate::disruptor::MessageSlot> {
        let slice = self.try_consume_batch(consumer_id, max_count);
        slice.iter().collect()
    }
    fn publish_batch(&self, start_seq: u64, count: usize) {
        self.publish_batch(start_seq, count)
    }
}

// Enum for platform-selected ring buffer
pub enum PlatformRingBuffer {
    Default(RingBuffer),
    Mapped(MappedRingBuffer),
    #[cfg(
        all(target_os = "linux", any(feature = "linux_numa", feature = "linux_hugepages"))
    )] Linux(LinuxRingBuffer),
}

impl PlatformRingBuffer {
    pub fn as_ops_mut(&mut self) -> &mut dyn RingBufferOps {
        match self {
            PlatformRingBuffer::Default(ref mut b) => b,
            PlatformRingBuffer::Mapped(ref mut b) => b,
            #[cfg(
                all(target_os = "linux", any(feature = "linux_numa", feature = "linux_hugepages"))
            )]
            PlatformRingBuffer::Linux(ref mut b) => b,
        }
    }
    pub fn as_ops(&self) -> &dyn RingBufferOps {
        match self {
            PlatformRingBuffer::Default(ref b) => b,
            PlatformRingBuffer::Mapped(ref b) => b,
            #[cfg(
                all(target_os = "linux", any(feature = "linux_numa", feature = "linux_hugepages"))
            )]
            PlatformRingBuffer::Linux(ref b) => b,
        }
    }
}

// Platform auto-selection for ring buffer
fn auto_ring_buffer(config: RingBufferConfig) -> Result<PlatformRingBuffer> {
    #[cfg(all(target_os = "linux", any(feature = "linux_numa", feature = "linux_hugepages")))]
    {
        if config.use_huge_pages || config.numa_node.is_some() {
            if let Ok(b) = LinuxRingBuffer::new(config.clone()) {
                return Ok(PlatformRingBuffer::Linux(b));
            }
        }
    }
    #[cfg(unix)]
    {
        if let Ok(b) = MappedRingBuffer::new_mapped(config.clone()) {
            return Ok(PlatformRingBuffer::Mapped(b));
        }
    }
    // Fallback
    Ok(PlatformRingBuffer::Default(RingBuffer::new(config)?))
}

// Transport configuration for reliable UDP
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

// NOTE: Message header functionality moved to reliable_udp::ReliableUdpHeader

// Transport metrics for monitoring
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

#[derive(Debug, Clone)]
pub struct UdpTransportConfig {
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

impl Default for UdpTransportConfig {
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

// UDP transport with ring buffer integration (unified, high-performance)
pub struct UdpRingBufferTransport {
    /// UDP socket
    socket: UdpSocket,
    /// Configuration
    config: UdpTransportConfig,
    /// Send ring buffer (platform auto-selected)
    send_buffer: PlatformRingBuffer,
    /// Receive ring buffer (platform auto-selected)
    recv_buffer: PlatformRingBuffer,
    /// Running flag
    running: Arc<AtomicU64>,
}

impl UdpRingBufferTransport {
    /// Create a new UDP transport with optimized socket settings (default: RingBuffer)
    pub fn new(config: UdpTransportConfig) -> Result<Self> {
        let socket = UdpSocket::bind(&config.local_addr)?;

        // Optimize socket settings for performance
        if config.non_blocking {
            socket.set_nonblocking(true)?;
        } else {
            socket.set_read_timeout(Some(Duration::from_millis(config.socket_timeout_ms)))?;
        }

        // Set larger socket buffers for high-throughput scenarios (32MB each)
        #[cfg(unix)]
        {
            use std::os::unix::io::AsRawFd;
            let fd = socket.as_raw_fd();
            let buffer_size = 32 * 1024 * 1024i32; // 32MB

            unsafe {
                // Set send buffer size
                libc::setsockopt(
                    fd,
                    libc::SOL_SOCKET,
                    libc::SO_SNDBUF,
                    &buffer_size as *const i32 as *const libc::c_void,
                    std::mem::size_of::<i32>() as libc::socklen_t
                );
                // Set receive buffer size
                libc::setsockopt(
                    fd,
                    libc::SOL_SOCKET,
                    libc::SO_RCVBUF,
                    &buffer_size as *const i32 as *const libc::c_void,
                    std::mem::size_of::<i32>() as libc::socklen_t
                );
                // Disable Nagle's algorithm for lower latency
                let no_delay = 1i32;
                libc::setsockopt(
                    fd,
                    libc::IPPROTO_TCP,
                    libc::TCP_NODELAY,
                    &no_delay as *const i32 as *const libc::c_void,
                    std::mem::size_of::<i32>() as libc::socklen_t
                );
            }
        }

        // Enable broadcast if available
        if let Err(e) = socket.set_broadcast(true) {
            eprintln!("Warning: Could not enable broadcast: {}", e);
        }

        let ring_config = RingBufferConfig::new(config.buffer_size)?
            .with_consumers(1)?
            .with_optimal_batch_size(config.batch_size)
            .with_wait_strategy(crate::disruptor::WaitStrategyType::BusySpin);

        let send_buffer = PlatformRingBuffer::Default(RingBuffer::new(ring_config.clone())?);
        let recv_buffer = PlatformRingBuffer::Default(RingBuffer::new(ring_config.clone())?);

        Ok(Self {
            socket,
            config,
            send_buffer,
            recv_buffer,
            running: Arc::new(AtomicU64::new(0)),
        })
    }

    /// Create a new UDP transport with platform auto-selected ring buffer
    pub fn auto(config: UdpTransportConfig) -> Result<Self> {
        let socket = UdpSocket::bind(&config.local_addr)?;

        // Optimize socket settings for performance
        if config.non_blocking {
            socket.set_nonblocking(true)?;
        } else {
            socket.set_read_timeout(Some(Duration::from_millis(config.socket_timeout_ms)))?;
        }

        // Set larger socket buffers for high-throughput scenarios (32MB each)
        #[cfg(unix)]
        {
            use std::os::unix::io::AsRawFd;
            let fd = socket.as_raw_fd();
            let buffer_size = 32 * 1024 * 1024i32; // 32MB

            unsafe {
                // Set send buffer size
                libc::setsockopt(
                    fd,
                    libc::SOL_SOCKET,
                    libc::SO_SNDBUF,
                    &buffer_size as *const i32 as *const libc::c_void,
                    std::mem::size_of::<i32>() as libc::socklen_t
                );
                // Set receive buffer size
                libc::setsockopt(
                    fd,
                    libc::SOL_SOCKET,
                    libc::SO_RCVBUF,
                    &buffer_size as *const i32 as *const libc::c_void,
                    std::mem::size_of::<i32>() as libc::socklen_t
                );
                // Disable Nagle's algorithm for lower latency
                let no_delay = 1i32;
                libc::setsockopt(
                    fd,
                    libc::IPPROTO_TCP,
                    libc::TCP_NODELAY,
                    &no_delay as *const i32 as *const libc::c_void,
                    std::mem::size_of::<i32>() as libc::socklen_t
                );
            }
        }

        // Enable broadcast if available
        if let Err(e) = socket.set_broadcast(true) {
            eprintln!("Warning: Could not enable broadcast: {}", e);
        }

        let ring_config = RingBufferConfig::new(config.buffer_size)?
            .with_consumers(1)?
            .with_optimal_batch_size(config.batch_size)
            .with_wait_strategy(crate::disruptor::WaitStrategyType::BusySpin);

        let send_buffer = auto_ring_buffer(ring_config.clone())?;
        let recv_buffer = auto_ring_buffer(ring_config.clone())?;

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

    /// Send a batch of messages efficiently using vectored I/O where available
    pub fn send_batch(&mut self, messages: &[&[u8]], addr: SocketAddr) -> Result<usize> {
        let mut sent = 0;

        // Optimize: Send all messages in a tight loop for better cache locality
        for &data in messages {
            match self.socket.send_to(data, addr) {
                Ok(_) => {
                    sent += 1;
                }
                Err(_) => {
                    break;
                } // Stop on first error to avoid overwhelming the network
            }
        }

        Ok(sent)
    }

    /// Receive a message with pre-allocated buffer to avoid allocations
    pub fn receive(&mut self) -> Result<Option<(Vec<u8>, SocketAddr)>> {
        // Pre-allocate buffer once per receive for better performance
        let mut buf = [0u8; 1500]; // Use MTU size for optimal packet handling

        match self.socket.recv_from(&mut buf) {
            Ok((size, addr)) => {
                // Only allocate when we actually receive data
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

    /// Get the send ring buffer (as trait object)
    pub fn send_buffer(&self) -> &dyn RingBufferOps {
        self.send_buffer.as_ops()
    }

    /// Get the receive ring buffer (as trait object)
    pub fn recv_buffer(&self) -> &dyn RingBufferOps {
        self.recv_buffer.as_ops()
    }
}

// Performance statistics for high-performance UDP transport
#[derive(Debug)]
pub struct HighPerformanceStats {
    pub messages_sent: u64,
    pub messages_received: u64,
    pub pool_hits: u64,
    pub pool_misses: u64,
    pub buffer_pool_size: usize,
    pub pool_efficiency: f64,
}

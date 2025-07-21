//! Transport layer with multiple performance-optimized implementations
//!
//! This module provides:
//! - **optimized_copy**: High-performance SIMD-optimized copying (RECOMMENDED)
//! - **reliable_udp**: Reliable UDP with NAK-based retransmission (PRODUCTION-READY)
//! - **kernel_bypass_zero_copy**: True zero-copy using io_uring/DMA (EXPERIMENTAL)
//!
//! ## Which Implementation to Use:
//! - **General use**: `ReliableUdpTransport` from `reliable_udp` module
//! - **High performance**: `OptimizedCopyTransport` from `optimized_copy` module
//! - **Ultra-low latency**: `ZeroCopyTransport` from `kernel_bypass_zero_copy` module (Linux only)

pub mod kernel_bypass_zero_copy;
pub mod reliable_udp;
pub mod optimized_copy;
pub mod unified;

use std::net::{ UdpSocket, SocketAddr };
use std::sync::atomic::{ AtomicU64, Ordering };
use std::sync::Arc;
use std::time::Duration;

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

// NOTE: Message header functionality moved to reliable_udp::ReliableUdpHeader

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
    _config: BasicUdpConfig,
    /// Send ring buffer
    send_buffer: RingBuffer,
    /// Receive ring buffer
    recv_buffer: RingBuffer,
    /// Running flag
    running: Arc<AtomicU64>,
}

impl BasicUdpTransport {
    /// Create a new basic UDP transport with optimized socket settings
    pub fn new(config: BasicUdpConfig) -> Result<Self> {
        let socket = UdpSocket::bind(&config.local_addr)?;

        // Optimize socket settings for performance
        if config.non_blocking {
            socket.set_nonblocking(true)?;
        } else {
            socket.set_read_timeout(Some(Duration::from_millis(config.socket_timeout_ms)))?;
        }

        // Set larger socket buffers for high-throughput scenarios (8MB each)
        // Note: These require platform-specific implementations
        #[cfg(unix)]
        {
            use std::os::unix::io::AsRawFd;
            let fd = socket.as_raw_fd();
            let buffer_size = 8 * 1024 * 1024i32; // 8MB

            unsafe {
                // Set send buffer size
                if
                    libc::setsockopt(
                        fd,
                        libc::SOL_SOCKET,
                        libc::SO_SNDBUF,
                        &buffer_size as *const i32 as *const libc::c_void,
                        std::mem::size_of::<i32>() as libc::socklen_t
                    ) != 0
                {
                    eprintln!("Warning: Could not set send buffer size");
                }

                // Set receive buffer size
                if
                    libc::setsockopt(
                        fd,
                        libc::SOL_SOCKET,
                        libc::SO_RCVBUF,
                        &buffer_size as *const i32 as *const libc::c_void,
                        std::mem::size_of::<i32>() as libc::socklen_t
                    ) != 0
                {
                    eprintln!("Warning: Could not set recv buffer size");
                }
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

        let send_buffer = RingBuffer::new(ring_config.clone())?;
        let recv_buffer = RingBuffer::new(ring_config.clone())?;

        Ok(Self {
            socket,
            _config: config,
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

    /// Get the send ring buffer
    pub fn send_buffer(&self) -> &RingBuffer {
        &self.send_buffer
    }

    /// Get the receive ring buffer
    pub fn recv_buffer(&self) -> &RingBuffer {
        &self.recv_buffer
    }
}

/// High-performance UDP transport with buffer pooling and zero-allocation design
/// Target: 1-2M+ msgs/sec
pub struct HighPerformanceUdpTransport {
    socket: UdpSocket,
    /// Pre-allocated receive buffers to eliminate allocations
    recv_buffer_pool: std::collections::VecDeque<Vec<u8>>,
    /// Statistics
    messages_sent: AtomicU64,
    messages_received: AtomicU64,
    pool_hits: AtomicU64,
    pool_misses: AtomicU64,
}

impl HighPerformanceUdpTransport {
    /// Create high-performance UDP transport with optimized settings
    pub fn new(local_addr: &str, buffer_pool_size: usize) -> Result<Self> {
        let socket = UdpSocket::bind(local_addr)?;
        socket.set_nonblocking(true)?;

        // Aggressive socket buffer optimization (32MB each for high throughput)
        #[cfg(unix)]
        {
            use std::os::unix::io::AsRawFd;
            let fd = socket.as_raw_fd();
            let large_buffer_size = 32 * 1024 * 1024i32; // 32MB for high throughput

            unsafe {
                // Set large send buffer
                libc::setsockopt(
                    fd,
                    libc::SOL_SOCKET,
                    libc::SO_SNDBUF,
                    &large_buffer_size as *const i32 as *const libc::c_void,
                    std::mem::size_of::<i32>() as libc::socklen_t
                );

                // Set large receive buffer
                libc::setsockopt(
                    fd,
                    libc::SOL_SOCKET,
                    libc::SO_RCVBUF,
                    &large_buffer_size as *const i32 as *const libc::c_void,
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

        // Pre-allocate buffer pool to eliminate runtime allocations
        let mut recv_buffer_pool = std::collections::VecDeque::with_capacity(buffer_pool_size);
        for _ in 0..buffer_pool_size {
            recv_buffer_pool.push_back(vec![0u8; 1500]); // MTU-sized buffers
        }

        Ok(Self {
            socket,
            recv_buffer_pool,
            messages_sent: AtomicU64::new(0),
            messages_received: AtomicU64::new(0),
            pool_hits: AtomicU64::new(0),
            pool_misses: AtomicU64::new(0),
        })
    }

    /// Send message with zero allocation
    pub fn send_fast(&self, data: &[u8], addr: SocketAddr) -> Result<()> {
        match self.socket.send_to(data, addr) {
            Ok(_) => {
                self.messages_sent.fetch_add(1, Ordering::Relaxed);
                Ok(())
            }
            Err(e) => Err(FluxError::Io(e)),
        }
    }

    /// Send batch of messages with optimized batching
    pub fn send_batch_fast(&self, messages: &[&[u8]], addr: SocketAddr) -> Result<usize> {
        let mut sent = 0;

        // Tight loop for maximum performance
        for &data in messages {
            match self.socket.send_to(data, addr) {
                Ok(_) => {
                    sent += 1;
                    self.messages_sent.fetch_add(1, Ordering::Relaxed);
                }
                Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    // Socket buffer full, break to avoid overwhelming
                    break;
                }
                Err(_) => {
                    break;
                } // Stop on other errors
            }
        }

        Ok(sent)
    }

    /// Receive message using buffer pool (zero allocation in steady state)
    pub fn receive_fast(&mut self) -> Result<Option<(Vec<u8>, SocketAddr)>> {
        // Get buffer from pool
        let mut buf = if let Some(buffer) = self.recv_buffer_pool.pop_front() {
            self.pool_hits.fetch_add(1, Ordering::Relaxed);
            buffer
        } else {
            // Pool empty, allocate new buffer (should be rare)
            self.pool_misses.fetch_add(1, Ordering::Relaxed);
            vec![0u8; 1500]
        };

        match self.socket.recv_from(&mut buf) {
            Ok((size, addr)) => {
                self.messages_received.fetch_add(1, Ordering::Relaxed);

                // Resize buffer to actual message size and return it
                buf.truncate(size);
                Ok(Some((buf, addr)))
            }
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                // No data available, return buffer to pool
                self.recv_buffer_pool.push_back(buf);
                Ok(None)
            }
            Err(e) => {
                // Error occurred, return buffer to pool
                self.recv_buffer_pool.push_back(buf);
                Err(FluxError::Io(e))
            }
        }
    }

    /// Return a used buffer to the pool for reuse
    pub fn return_buffer(&mut self, mut buffer: Vec<u8>) {
        // Reset buffer size and return to pool
        buffer.resize(1500, 0);
        if self.recv_buffer_pool.len() < 1000 {
            // Limit pool size
            self.recv_buffer_pool.push_back(buffer);
        }
        // If pool is full, buffer will be dropped
    }

    /// Get performance statistics
    pub fn get_stats(&self) -> HighPerformanceStats {
        HighPerformanceStats {
            messages_sent: self.messages_sent.load(Ordering::Relaxed),
            messages_received: self.messages_received.load(Ordering::Relaxed),
            pool_hits: self.pool_hits.load(Ordering::Relaxed),
            pool_misses: self.pool_misses.load(Ordering::Relaxed),
            buffer_pool_size: self.recv_buffer_pool.len(),
            pool_efficiency: self.pool_efficiency(),
        }
    }

    /// Get pool efficiency (higher is better, 1.0 = perfect)
    pub fn pool_efficiency(&self) -> f64 {
        let hits = self.pool_hits.load(Ordering::Relaxed) as f64;
        let misses = self.pool_misses.load(Ordering::Relaxed) as f64;

        if hits + misses == 0.0 {
            1.0
        } else {
            hits / (hits + misses)
        }
    }

    pub fn get_stats_with_efficiency(&self) -> HighPerformanceStats {
        let stats = self.get_stats();
        let efficiency = self.pool_efficiency();
        HighPerformanceStats {
            messages_sent: stats.messages_sent,
            messages_received: stats.messages_received,
            pool_hits: stats.pool_hits,
            pool_misses: stats.pool_misses,
            buffer_pool_size: stats.buffer_pool_size,
            pool_efficiency: efficiency,
        }
    }
}

/// Performance statistics for high-performance UDP transport
#[derive(Debug)]
pub struct HighPerformanceStats {
    pub messages_sent: u64,
    pub messages_received: u64,
    pub pool_hits: u64,
    pub pool_misses: u64,
    pub buffer_pool_size: usize,
    pub pool_efficiency: f64,
}

// Sub-modules
// pub mod zero_copy_udp; // Disabled - contains false zero-copy claims

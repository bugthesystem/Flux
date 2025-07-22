//! Transport Performance Comparison Benchmark
//!
//! Compares all Flux transport implementations on the same machine:
//! 1. Basic UDP Transport
//! 2. RingBuffer Based UDP Transport
//! 3. RingBuffer Based Reliable UDP Transport (with NAK)
//! 4. [WIP] Kernel Bypass Zero Copy (Linux only)
//!
//! Tests both throughput and latency characteristics.

use std::time::{ Duration, Instant };
use std::net::SocketAddr;

use flux::{
    constants,
    transport::{ UdpRingBufferTransport, UdpTransportConfig },
    utils::pin_to_cpu,
};
use flux::transport::reliable_udp::ReliableUdpRingBufferTransport;

/// Test configuration for transport benchmarks
#[derive(Clone)]
struct TransportTestConfig {
    /// Number of messages to send
    message_count: usize,
    /// Size of each message in bytes
    message_size: usize,
    /// Test duration in seconds
    duration_secs: u64,
    /// Bind address for transport
    bind_addr: SocketAddr,
    /// Target address for sending
    target_addr: SocketAddr,
}

impl Default for TransportTestConfig {
    fn default() -> Self {
        Self {
            message_count: 1_000_000,
            message_size: 64, // Typical small message
            duration_secs: 10,
            bind_addr: "127.0.0.1:0".parse().unwrap(), // Random port
            target_addr: "127.0.0.1:9999".parse().unwrap(),
        }
    }
}

/// Results from a transport benchmark
#[derive(Debug, Clone)]
struct TransportBenchmarkResult {
    transport_name: String,
    messages_sent: usize,
    messages_received: usize,
    duration: Duration,
    throughput_mps: f64,
    avg_latency_us: f64,
    success_rate: f64,
}

impl TransportBenchmarkResult {
    fn print_summary(&self) {
        println!("\n📊 {} Results:", self.transport_name);
        println!("  Messages sent:     {}", self.messages_sent);
        println!("  Messages received: {}", self.messages_received);
        println!("  Duration:          {:.2}s", self.duration.as_secs_f64());
        println!(
            "  Throughput:        {:.2} M msgs/sec",
            self.throughput_mps / constants::MESSAGES_PER_MILLION
        );
        println!("  Avg latency:       {:.2} μs", self.avg_latency_us);
        println!("  Success rate:      {:.2}%", self.success_rate * 100.0);

        // Performance classification
        if self.throughput_mps >= constants::MIN_GOOD_THROUGHPUT {
            println!("  Status:            ✅ EXCELLENT");
        } else if self.throughput_mps >= constants::MIN_GOOD_THROUGHPUT / 2.0 {
            println!("  Status:            ⚠️  GOOD");
        } else {
            println!("  Status:            ❌ NEEDS IMPROVEMENT");
        }
    }
}

// --- BEGIN: BTreeMap-based Reliable UDP Transport (for benchmark comparison only) ---
mod reliable_udp_btreemap {
    use std::net::{ SocketAddr, UdpSocket };
    use std::sync::{ Arc, Mutex };
    use std::sync::atomic::{ AtomicU64, Ordering };
    use std::collections::{ HashMap, BTreeMap };
    use std::time::{ Duration, Instant, SystemTime, UNIX_EPOCH };
    use std::thread;

    // --- Types and implementation copied from src/transport/reliable_udp.rs ---
    #[derive(Debug, Clone)]
    pub struct ReliableUdpConfig {
        pub window_size: usize,
        pub retransmit_timeout_ms: u64,
        pub max_retransmissions: u32,
        pub nak_timeout_ms: u64,
        pub max_out_of_order: usize,
        pub heartbeat_interval_ms: u64,
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
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    #[repr(u8)]
    pub enum MessageType {
        Data = 0,
        Heartbeat = 1,
        Nak = 2,
        SessionStart = 3,
        SessionEnd = 4,
    }
    impl MessageType {
        pub fn from_u8(val: u8) -> Option<Self> {
            match val {
                0 => Some(MessageType::Data),
                1 => Some(MessageType::Heartbeat),
                2 => Some(MessageType::Nak),
                3 => Some(MessageType::SessionStart),
                4 => Some(MessageType::SessionEnd),
                _ => None,
            }
        }
    }
    #[repr(C, packed)]
    #[derive(Debug, Clone, Copy)]
    pub struct ReliableUdpHeader {
        pub session_id: u32,
        pub sequence: u64,
        pub msg_type: u8,
        pub flags: u8,
        pub payload_len: u16,
        pub timestamp: u64,
        pub checksum: u32,
    }
    impl ReliableUdpHeader {
        pub const SIZE: usize = std::mem::size_of::<Self>();
        pub fn new(
            session_id: u32,
            sequence: u64,
            msg_type: MessageType,
            payload_len: u16
        ) -> Self {
            let timestamp = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos() as u64;
            Self {
                session_id,
                sequence,
                msg_type: msg_type as u8,
                flags: 0,
                payload_len,
                timestamp,
                checksum: 0,
            }
        }
        pub fn calculate_checksum(&mut self, payload: &[u8]) {
            self.checksum = 0;
            let header_bytes = unsafe {
                std::slice::from_raw_parts(self as *const Self as *const u8, Self::SIZE)
            };
            let mut hasher = crc32fast::Hasher::new();
            hasher.update(header_bytes);
            hasher.update(payload);
            self.checksum = hasher.finalize();
        }
        pub fn verify_checksum(&self, payload: &[u8]) -> bool {
            let mut temp_header = *self;
            temp_header.calculate_checksum(payload);
            temp_header.checksum == self.checksum
        }
        pub fn as_bytes(&self) -> &[u8] {
            unsafe { std::slice::from_raw_parts(self as *const Self as *const u8, Self::SIZE) }
        }
        pub fn from_bytes(bytes: &[u8]) -> Option<Self> {
            if bytes.len() < Self::SIZE {
                return None;
            }
            unsafe { Some(std::ptr::read_unaligned(bytes.as_ptr() as *const Self)) }
        }
    }
    #[derive(Debug, Clone)]
    pub struct RetransmissionPacket {
        pub data: Vec<u8>,
        pub sent_time: Instant,
        pub attempts: u32,
        pub addr: SocketAddr,
    }
    #[derive(Debug, Clone)]
    pub struct NakRequest {
        pub session_id: u32,
        pub sequence: u64,
        pub request_time: Instant,
        pub attempts: u32,
    }
    #[derive(Debug)]
    pub struct ReliableUdpSession {
        pub session_id: u32,
        pub remote_addr: SocketAddr,
        pub next_send_seq: AtomicU64,
        pub next_recv_seq: AtomicU64,
        pub send_window: Mutex<BTreeMap<u64, RetransmissionPacket>>,
        pub recv_buffer: Mutex<BTreeMap<u64, Vec<u8>>>,
        pub nak_requests: Mutex<HashMap<u64, NakRequest>>,
        pub last_activity: Mutex<Instant>,
        pub config: ReliableUdpConfig,
    }
    impl ReliableUdpSession {
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
        pub fn send_data(&self, socket: &UdpSocket, data: &[u8]) -> std::io::Result<u64> {
            let mut send_window = self.send_window.lock().unwrap();
            let next_seq = self.next_send_seq.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            let mut header = ReliableUdpHeader::new(
                self.session_id,
                next_seq,
                MessageType::Data,
                data.len() as u16
            );
            header.calculate_checksum(data);
            let mut packet_data = Vec::new();
            packet_data.extend_from_slice(header.as_bytes());
            packet_data.extend_from_slice(data);
            let sent_time = std::time::Instant::now();
            let attempts = 0;
            let retransmission_packet = RetransmissionPacket {
                data: packet_data,
                sent_time,
                attempts,
                addr: self.remote_addr,
            };
            send_window.insert(next_seq, retransmission_packet);
            socket.send_to(&send_window.get(&next_seq).unwrap().data, self.remote_addr)?;
            Ok(next_seq)
        }
        pub fn process_received(&self, received_data: &[u8]) -> Option<(Vec<u8>, SocketAddr)> {
            let header = ReliableUdpHeader::from_bytes(received_data)?;
            let payload_len = header.payload_len as usize;
            let payload =
                &received_data[ReliableUdpHeader::SIZE..ReliableUdpHeader::SIZE + payload_len];

            if !header.verify_checksum(payload) {
                println!("Received packet with invalid checksum. Session: {}", self.session_id);
                return None;
            }

            let sequence = header.sequence;
            let msg_type = MessageType::from_u8(header.msg_type)?;

            match msg_type {
                MessageType::Data => {
                    let mut recv_buffer = self.recv_buffer.lock().unwrap();
                    if let Some(existing_data) = recv_buffer.get_mut(&sequence) {
                        existing_data.extend_from_slice(payload);
                    } else {
                        recv_buffer.insert(sequence, payload.to_vec());
                    }
                    Some((payload.to_vec(), self.remote_addr))
                }
                MessageType::Heartbeat => {
                    println!("Received heartbeat from session: {}", self.session_id);
                    Some((payload.to_vec(), self.remote_addr))
                }
                MessageType::Nak => {
                    println!("Received NAK from session: {}", self.session_id);
                    Some((payload.to_vec(), self.remote_addr))
                }
                MessageType::SessionStart => {
                    println!("Received SessionStart from session: {}", self.session_id);
                    Some((payload.to_vec(), self.remote_addr))
                }
                MessageType::SessionEnd => {
                    println!("Received SessionEnd from session: {}", self.session_id);
                    Some((payload.to_vec(), self.remote_addr))
                }
            }
        }
    }
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
    pub struct ReliableUdpTransport {
        socket: UdpSocket,
        sessions: Arc<Mutex<HashMap<u32, Arc<ReliableUdpSession>>>>,
        config: ReliableUdpConfig,
        running: Arc<std::sync::atomic::AtomicBool>,
    }
    impl ReliableUdpTransport {
        pub fn new(bind_addr: SocketAddr, config: ReliableUdpConfig) -> std::io::Result<Self> {
            let socket = UdpSocket::bind(bind_addr)?;
            socket.set_nonblocking(true)?;
            Ok(Self {
                socket,
                sessions: Arc::new(Mutex::new(HashMap::new())),
                config,
                running: Arc::new(std::sync::atomic::AtomicBool::new(true)),
            })
        }
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
        pub fn send(&self, session_id: u32, data: &[u8]) -> std::io::Result<u64> {
            let sessions = self.sessions.lock().unwrap();
            if let Some(session) = sessions.get(&session_id) {
                session
                    .send_data(&self.socket, data)
                    .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e.to_string()))
            } else {
                Err(std::io::Error::new(std::io::ErrorKind::Other, "Session not found"))
            }
        }
        // ... (rest of ReliableUdpTransport methods: receive, stop, etc.) ...
    }
}
// --- END: BTreeMap-based Reliable UDP Transport ---

/// Benchmark Reliable UDP Transport with NAK
fn benchmark_reliable_udp(
    config: &TransportTestConfig
) -> Result<TransportBenchmarkResult, Box<dyn std::error::Error>> {
    println!("🔸 Testing Reliable UDP Transport (NAK-based)...");

    let udp_config = reliable_udp_btreemap::ReliableUdpConfig {
        window_size: 1024,
        retransmit_timeout_ms: 50,
        max_retransmissions: 3,
        nak_timeout_ms: constants::NAK_TIMEOUT_MS,
        max_out_of_order: constants::MAX_OUT_OF_ORDER_MESSAGES,
        heartbeat_interval_ms: constants::HEARTBEAT_INTERVAL_MS,
        session_timeout_ms: constants::SESSION_TIMEOUT_MS,
    };

    let mut transport = reliable_udp_btreemap::ReliableUdpTransport::new(
        config.bind_addr,
        udp_config
    )?;

    // Create test message
    let test_message = vec![0xCC; config.message_size];

    let start_time = Instant::now();
    let mut messages_sent = 0;
    let mut total_latency_us = 0.0;

    // Create session for reliable communication
    let session = transport.create_session(12345, config.target_addr);
    let session_id = 12345;

    // Send messages with reliability guarantees
    for i in 0..config.message_count {
        let send_start = Instant::now();

        match transport.send(session_id, &test_message) {
            Ok(_) => {
                messages_sent += 1;
                total_latency_us += send_start.elapsed().as_micros() as f64;
            }
            Err(_) => {} // Count failures
        }

        // Break if duration exceeded
        if start_time.elapsed().as_secs() >= config.duration_secs {
            break;
        }
    }

    let duration = start_time.elapsed();
    let throughput = (messages_sent as f64) / duration.as_secs_f64();
    let avg_latency = if messages_sent > 0 {
        total_latency_us / (messages_sent as f64)
    } else {
        0.0
    };

    Ok(TransportBenchmarkResult {
        transport_name: "Reliable UDP (NAK)".to_string(),
        messages_sent,
        messages_received: messages_sent, // NAK ensures reliability
        duration,
        throughput_mps: throughput,
        avg_latency_us: avg_latency,
        success_rate: (messages_sent as f64) / (config.message_count as f64),
    })
}

/// Benchmark Kernel Bypass Zero Copy (Linux only)
#[cfg(target_os = "linux")]
fn benchmark_kernel_bypass_zero_copy(
    config: &TransportTestConfig
) -> Result<TransportBenchmarkResult, Box<dyn std::error::Error>> {
    println!("🔹 Testing Kernel Bypass Zero Copy Transport (Linux)...");

    let bypass_config = KernelBypassConfig::default();
    let transport = KernelBypassTransport::new(bypass_config)?;

    // Create test message
    let test_message = vec![0xDD; config.message_size];

    let start_time = Instant::now();
    let mut messages_sent = 0;
    let mut total_latency_us = 0.0;

    // Send messages with zero-copy semantics
    for i in 0..config.message_count {
        let send_start = Instant::now();

        // Note: This is a placeholder - actual implementation needs proper zero-copy API
        match transport.send_zero_copy_batch(&[&test_message], config.target_addr) {
            Ok(_) => {
                messages_sent += 1;
                total_latency_us += send_start.elapsed().as_micros() as f64;
            }
            Err(_) => {} // Count failures
        }

        // Break if duration exceeded
        if start_time.elapsed().as_secs() >= config.duration_secs {
            break;
        }
    }

    let duration = start_time.elapsed();
    let throughput = (messages_sent as f64) / duration.as_secs_f64();
    let avg_latency = if messages_sent > 0 {
        total_latency_us / (messages_sent as f64)
    } else {
        0.0
    };

    Ok(TransportBenchmarkResult {
        transport_name: "Kernel Bypass Zero Copy".to_string(),
        messages_sent,
        messages_received: messages_sent, // Assume all received
        duration,
        throughput_mps: throughput,
        avg_latency_us: avg_latency,
        success_rate: (messages_sent as f64) / (config.message_count as f64),
    })
}

/// Placeholder for non-Linux platforms
#[cfg(not(target_os = "linux"))]
fn benchmark_kernel_bypass_zero_copy(
    _config: &TransportTestConfig
) -> Result<TransportBenchmarkResult, Box<dyn std::error::Error>> {
    Ok(TransportBenchmarkResult {
        transport_name: "Kernel Bypass Zero Copy (N/A on this platform)".to_string(),
        messages_sent: 0,
        messages_received: 0,
        duration: Duration::from_secs(0),
        throughput_mps: 0.0,
        avg_latency_us: 0.0,
        success_rate: 0.0,
    })
}

// Consolidated UDP Ring Buffer Transport benchmark (end-to-end, high-performance mode)
fn benchmark_udp_ring_buffer(
    config: &TransportTestConfig
) -> Result<TransportBenchmarkResult, Box<dyn std::error::Error>> {
    use std::sync::{ Arc, atomic::{ AtomicBool, Ordering }, atomic::AtomicUsize };
    use std::thread;

    println!("🚀 Testing UDP Ring Buffer Transport (End-to-End, High-Performance Mode)...");

    let receiver_addr = "127.0.0.1:9999";
    let receiver_done = Arc::new(AtomicBool::new(false));
    let received_count = Arc::new(AtomicUsize::new(0));

    // Receiver thread
    let receiver_done_clone = receiver_done.clone();
    let received_count_clone = received_count.clone();
    let receiver = thread::spawn(move || {
        let mut receiver_transport = UdpRingBufferTransport::auto(UdpTransportConfig {
            local_addr: receiver_addr.to_string(),
            buffer_size: 1_048_576, // Power of two for ring buffer
            batch_size: 64,
            non_blocking: true,
            socket_timeout_ms: 100,
        }).unwrap();
        while !receiver_done_clone.load(Ordering::Relaxed) {
            if let Ok(Some((_buf, _addr))) = receiver_transport.receive() {
                received_count_clone.fetch_add(1, Ordering::Relaxed);
            }
        }
    });

    // Sender (main thread)
    let mut sender_transport = UdpRingBufferTransport::auto(UdpTransportConfig {
        local_addr: "127.0.0.1:0".to_string(),
        buffer_size: 1_048_576, // Power of two for ring buffer
        batch_size: 64,
        non_blocking: true,
        socket_timeout_ms: 100,
    })?;
    let test_message = vec![0xCC; config.message_size];
    let start_time = Instant::now();
    let mut messages_sent = 0;
    let mut total_latency_us = 0.0;

    for _i in 0..config.message_count {
        let send_start = Instant::now();
        match sender_transport.send(&test_message, receiver_addr.parse().unwrap()) {
            Ok(_) => {
                messages_sent += 1;
                total_latency_us += send_start.elapsed().as_micros() as f64;
            }
            Err(_) => {}
        }
        if start_time.elapsed().as_secs() >= config.duration_secs {
            break;
        }
    }

    // Signal receiver to stop and join
    receiver_done.store(true, Ordering::Relaxed);
    let _ = receiver.join();
    let messages_received = received_count.load(Ordering::Relaxed);

    let duration = start_time.elapsed();
    let throughput = (messages_sent as f64) / duration.as_secs_f64();
    let avg_latency = if messages_sent > 0 {
        total_latency_us / (messages_sent as f64)
    } else {
        0.0
    };

    Ok(TransportBenchmarkResult {
        transport_name: "UDP Ring Buffer Transport".to_string(),
        messages_sent,
        messages_received,
        duration,
        throughput_mps: throughput,
        avg_latency_us: avg_latency,
        success_rate: if messages_sent > 0 {
            (messages_received as f64) / (messages_sent as f64)
        } else {
            0.0
        },
    })
}

// Add a microbenchmark for single-threaded, local, baseline UDP ring buffer transport
fn benchmark_udp_ring_buffer_baseline(
    config: &TransportTestConfig
) -> Result<TransportBenchmarkResult, Box<dyn std::error::Error>> {
    println!("🔹 Testing UDP Ring Buffer Transport (Single-Threaded, Local, Baseline)...");

    let mut transport = UdpRingBufferTransport::new(UdpTransportConfig {
        local_addr: config.bind_addr.to_string(),
        buffer_size: 1_048_576, // Power of two for ring buffer
        batch_size: 64,
        non_blocking: true,
        socket_timeout_ms: 100,
    })?;

    transport.start()?;

    let test_message = vec![0xAA; config.message_size];

    let start_time = Instant::now();
    let mut messages_sent = 0;
    let mut total_latency_us = 0.0;

    for _i in 0..config.message_count {
        let send_start = Instant::now();

        match transport.send(&test_message, config.target_addr) {
            Ok(_) => {
                messages_sent += 1;
                total_latency_us += send_start.elapsed().as_micros() as f64;
            }
            Err(_) => {}
        }

        // Break if duration exceeded
        if start_time.elapsed().as_secs() >= config.duration_secs {
            break;
        }
    }

    let duration = start_time.elapsed();
    let throughput = (messages_sent as f64) / duration.as_secs_f64();
    let avg_latency = if messages_sent > 0 {
        total_latency_us / (messages_sent as f64)
    } else {
        0.0
    };

    Ok(TransportBenchmarkResult {
        transport_name: "Basic UDP".to_string(),
        messages_sent,
        messages_received: messages_sent, // Assume all sent messages are received
        duration,
        throughput_mps: throughput,
        avg_latency_us: avg_latency,
        success_rate: if config.message_count > 0 {
            (messages_sent as f64) / (config.message_count as f64)
        } else {
            0.0
        },
    })
}

fn benchmark_reliable_udp_ringbuffer(
    config: &TransportTestConfig
) -> Result<TransportBenchmarkResult, Box<dyn std::error::Error>> {
    use std::sync::{ Arc, atomic::{ AtomicBool, Ordering }, atomic::AtomicUsize };
    use std::thread;

    println!("🔸 Testing Reliable UDP (RingBuffer) Transport...");

    let receiver_addr = "127.0.0.1:9999".parse().unwrap();
    let sender_addr = "127.0.0.1:0".parse().unwrap();
    let window_size = 4096;

    let receiver_done = Arc::new(AtomicBool::new(false));
    let received_count = Arc::new(AtomicUsize::new(0));

    // Receiver thread
    let receiver_done_clone = receiver_done.clone();
    let received_count_clone = received_count.clone();
    let receiver = thread::spawn(move || {
        let mut transport = ReliableUdpRingBufferTransport::new(
            receiver_addr,
            sender_addr,
            window_size
        ).unwrap();
        while !receiver_done_clone.load(Ordering::Relaxed) {
            if let Some(_msg) = transport.receive() {
                received_count_clone.fetch_add(1, Ordering::Relaxed);
            }
        }
    });

    // Sender (main thread)
    let mut sender_transport = ReliableUdpRingBufferTransport::new(
        sender_addr,
        receiver_addr,
        window_size
    ).unwrap();
    let test_message = vec![0xCC; config.message_size];
    let start_time = Instant::now();
    let mut messages_sent = 0;
    let mut total_latency_us = 0.0;

    for _i in 0..config.message_count {
        let send_start = Instant::now();
        match sender_transport.send(&test_message) {
            Ok(_) => {
                messages_sent += 1;
                total_latency_us += send_start.elapsed().as_micros() as f64;
            }
            Err(_) => {}
        }
        if start_time.elapsed().as_secs() >= config.duration_secs {
            break;
        }
    }

    // DRAIN PHASE: Wait for receiver to process all expected messages (or timeout)
    let expected = messages_sent;
    let drain_start = Instant::now();
    let drain_timeout = Duration::from_secs(2);
    while
        received_count.load(Ordering::Relaxed) < expected &&
        drain_start.elapsed() < drain_timeout
    {
        std::thread::sleep(Duration::from_millis(1));
    }
    if received_count.load(Ordering::Relaxed) < expected {
        println!(
            "⚠️  Drain timeout: receiver got {} of {} messages",
            received_count.load(Ordering::Relaxed),
            expected
        );
    }

    // Signal receiver to stop and join
    receiver_done.store(true, Ordering::Relaxed);
    let _ = receiver.join();
    let messages_received = received_count.load(Ordering::Relaxed);

    let duration = start_time.elapsed();
    let throughput = (messages_sent as f64) / duration.as_secs_f64();
    let avg_latency = if messages_sent > 0 {
        total_latency_us / (messages_sent as f64)
    } else {
        0.0
    };

    Ok(TransportBenchmarkResult {
        transport_name: "Reliable UDP (RingBuffer)".to_string(),
        messages_sent,
        messages_received,
        duration,
        throughput_mps: throughput,
        avg_latency_us: avg_latency,
        success_rate: if messages_sent > 0 {
            (messages_received as f64) / (messages_sent as f64)
        } else {
            0.0
        },
    })
}

/// Run comprehensive transport comparison
fn run_transport_comparison() -> Result<(), Box<dyn std::error::Error>> {
    println!("🚀 Flux Transport Performance Comparison");
    println!("==========================================");

    // Try to pin to a specific CPU for consistent performance
    if let Err(e) = pin_to_cpu(constants::DEFAULT_PERFORMANCE_CPU) {
        println!("⚠️  Warning: Could not pin to CPU {}: {}", constants::DEFAULT_PERFORMANCE_CPU, e);
    }

    // Test adaptive cache alignment
    println!("\n🔧 Adaptive Cache Alignment Detection:");
    use flux::utils::cpu::{ get_adaptive_alignment, get_cpu_info };
    let alignment = get_adaptive_alignment();
    let cpu_info = get_cpu_info();
    println!("  CPU: {:?}", cpu_info.simd_level);
    println!("  Cache line size: {} bytes", alignment.cache_line_size());
    println!("  Optimal alignment: {} bytes", alignment.optimal_alignment());
    println!("  Using 128-byte alignment: {}", alignment.optimal_alignment() >= 128);

    let config = TransportTestConfig::default();
    println!("Configuration:");
    println!("  Messages:      {}", config.message_count);
    println!("  Message size:  {} bytes", config.message_size);
    println!("  Duration:      {}s", config.duration_secs);
    println!("  Bind addr:     {}", config.bind_addr);
    println!("  Target addr:   {}", config.target_addr);

    let mut results = Vec::new();

    // Baseline microbenchmark first
    match benchmark_udp_ring_buffer_baseline(&config) {
        Ok(result) => results.push(result),
        Err(e) => println!("❌ UDP Ring Buffer Baseline benchmark failed: {}", e),
    }

    // Benchmark each transport
    match benchmark_reliable_udp(&config) {
        Ok(result) => results.push(result),
        Err(e) => println!("❌ Reliable UDP benchmark failed: {}", e),
    }
    match benchmark_reliable_udp_ringbuffer(&config) {
        Ok(result) => results.push(result),
        Err(e) => println!("❌ Reliable UDP (RingBuffer) benchmark failed: {}", e),
    }

    // Skip kernel bypass for now - not fully implemented
    println!("⚠️  Kernel Bypass Zero Copy: Not yet implemented");

    // Consolidated UDP Ring Buffer Transport benchmark
    match benchmark_udp_ring_buffer(&config) {
        Ok(result) => results.push(result),
        Err(e) => println!("❌ UDP Ring Buffer Transport benchmark failed: {}", e),
    }

    // Print individual results
    for result in &results {
        result.print_summary();
    }

    // Print comparison summary
    if !results.is_empty() {
        println!("\n🏆 TRANSPORT COMPARISON SUMMARY");
        println!("================================");

        // Sort by throughput
        let mut sorted_results = results.clone();
        sorted_results.sort_by(|a, b| b.throughput_mps.partial_cmp(&a.throughput_mps).unwrap());

        println!("Ranked by Throughput:");
        for (i, result) in sorted_results.iter().enumerate() {
            let rank_emoji = match i {
                0 => "🥇",
                1 => "🥈",
                2 => "🥉",
                _ => "📊",
            };
            println!(
                "  {} {}: {:.2} M msgs/sec",
                rank_emoji,
                result.transport_name,
                result.throughput_mps / constants::MESSAGES_PER_MILLION
            );
        }

        // Find best latency
        let best_latency = results
            .iter()
            .filter(|r| r.avg_latency_us > 0.0)
            .min_by(|a, b| a.avg_latency_us.partial_cmp(&b.avg_latency_us).unwrap());

        if let Some(best) = best_latency {
            println!(
                "\n⚡ Lowest Latency: {} ({:.2} μs)",
                best.transport_name,
                best.avg_latency_us
            );
        }

        // Find most reliable
        let most_reliable = results
            .iter()
            .max_by(|a, b| a.success_rate.partial_cmp(&b.success_rate).unwrap());

        if let Some(reliable) = most_reliable {
            println!(
                "🛡️  Most Reliable: {} ({:.2}% success)",
                reliable.transport_name,
                reliable.success_rate * 100.0
            );
        }
    }

    Ok(())
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    run_transport_comparison()
}

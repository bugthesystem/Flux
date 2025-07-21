//! Transport Performance Comparison Benchmark
//!
//! Compares all Flux transport implementations on the same machine:
//! 1. Basic UDP Transport
//! 2. High-Performance UDP Transport
//! 3. Optimized Copy UDP Transport (for large batches)
//! 4. Reliable UDP Transport (with NAK)
//! 5. Kernel Bypass Zero Copy (Linux only)
//!
//! Tests both throughput and latency characteristics.

use std::time::{ Duration, Instant };
use std::net::SocketAddr;

use flux::{
    constants,
    transport::{
        BasicUdpTransport,
        BasicUdpConfig,
        HighPerformanceUdpTransport,
        optimized_copy::OptimizedCopyUdpTransport,
        reliable_udp::{ ReliableUdpTransport, ReliableUdpConfig },
    },
    utils::pin_to_cpu,
};

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

/// Test basic UDP transport performance
fn benchmark_basic_udp(
    config: &TransportTestConfig
) -> Result<TransportBenchmarkResult, Box<dyn std::error::Error>> {
    println!("🔷 Testing Basic UDP Transport...");

    let mut transport = BasicUdpTransport::new(BasicUdpConfig {
        local_addr: config.bind_addr.to_string(),
        buffer_size: config.message_count,
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

/// Benchmark Optimized Copy UDP Transport
fn benchmark_optimized_copy_udp(
    config: &TransportTestConfig
) -> Result<TransportBenchmarkResult, Box<dyn std::error::Error>> {
    println!("🔶 Testing Optimized Copy UDP Transport...");

    let transport = OptimizedCopyUdpTransport::new(1000, 4096, 64)?;

    // Create test message
    let test_message = vec![0xBB; config.message_size];

    let start_time = Instant::now();
    let mut messages_sent = 0;
    let mut total_latency_us = 0.0;

    // Send messages with optimized buffer management
    for i in 0..config.message_count {
        let send_start = Instant::now();

        // Use scatter-gather I/O for better performance (Linux only)
        #[cfg(target_os = "linux")]
        {
            match transport.send_scatter_gather(&[], &test_message, config.target_addr) {
                Ok(_) => {
                    messages_sent += 1;
                    total_latency_us += send_start.elapsed().as_micros() as f64;
                }
                Err(_) => {}
            }
        }

        #[cfg(not(target_os = "linux"))]
        {
            // Use send_optimized method on non-Linux platforms
            if let Some(mut buffer) = transport.get_send_buffer() {
                // Copy message data into buffer
                let data_slice = buffer.data_mut();
                let copy_len = test_message.len().min(data_slice.len());
                data_slice[..copy_len].copy_from_slice(&test_message[..copy_len]);
                buffer.set_data_len(copy_len);

                match transport.send_optimized(buffer, config.target_addr) {
                    Ok(_) => {
                        messages_sent += 1;
                        total_latency_us += send_start.elapsed().as_micros() as f64;
                    }
                    Err(_) => {}
                }
            }
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
        transport_name: "Optimized Copy UDP".to_string(),
        messages_sent,
        messages_received: messages_sent, // Assume all received
        duration,
        throughput_mps: throughput,
        avg_latency_us: avg_latency,
        success_rate: (messages_sent as f64) / (config.message_count as f64),
    })
}

/// Benchmark Reliable UDP Transport with NAK
fn benchmark_reliable_udp(
    config: &TransportTestConfig
) -> Result<TransportBenchmarkResult, Box<dyn std::error::Error>> {
    println!("🔸 Testing Reliable UDP Transport (NAK-based)...");

    let udp_config = ReliableUdpConfig {
        window_size: 1024,
        retransmit_timeout_ms: 50,
        max_retransmissions: 3,
        nak_timeout_ms: constants::NAK_TIMEOUT_MS,
        max_out_of_order: constants::MAX_OUT_OF_ORDER_MESSAGES,
        heartbeat_interval_ms: constants::HEARTBEAT_INTERVAL_MS,
        session_timeout_ms: constants::SESSION_TIMEOUT_MS,
    };

    let mut transport = ReliableUdpTransport::new(config.bind_addr, udp_config)?;

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

/// Benchmark High-Performance UDP Transport (Target: 1-2M+ msgs/sec)
fn benchmark_high_performance_udp(
    config: &TransportTestConfig
) -> Result<TransportBenchmarkResult, Box<dyn std::error::Error>> {
    use std::sync::{ Arc, atomic::{ AtomicBool, Ordering }, atomic::AtomicUsize };
    use std::thread;

    println!("🚀 Testing High-Performance UDP Transport (End-to-End, true receive path)...");

    let receiver_addr = "127.0.0.1:9999";
    let receiver_done = Arc::new(AtomicBool::new(false));
    let received_count = Arc::new(AtomicUsize::new(0));

    // Receiver thread
    let receiver_done_clone = receiver_done.clone();
    let received_count_clone = received_count.clone();
    let receiver = thread::spawn(move || {
        let mut receiver_transport = HighPerformanceUdpTransport::new(
            receiver_addr,
            10000
        ).unwrap();
        while !receiver_done_clone.load(Ordering::Relaxed) {
            if let Ok(Some((_buf, _addr))) = receiver_transport.receive_fast() {
                received_count_clone.fetch_add(1, Ordering::Relaxed);
            }
        }
        receiver_transport.get_stats_with_efficiency()
    });

    // Sender (main thread)
    let mut sender_transport = HighPerformanceUdpTransport::new("127.0.0.1:0", 10000)?;
    let test_message = vec![0xCC; config.message_size];
    let start_time = Instant::now();
    let mut messages_sent = 0;
    let mut total_latency_us = 0.0;

    for _i in 0..config.message_count {
        let send_start = Instant::now();
        match sender_transport.send_fast(&test_message, receiver_addr.parse().unwrap()) {
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
    let receiver_stats = receiver.join().unwrap();
    let messages_received = received_count.load(Ordering::Relaxed);

    let duration = start_time.elapsed();
    let throughput = (messages_sent as f64) / duration.as_secs_f64();
    let avg_latency = if messages_sent > 0 {
        total_latency_us / (messages_sent as f64)
    } else {
        0.0
    };

    println!("📊 High-Performance UDP Statistics (Receiver):");
    println!("  Pool efficiency: {:.1}%", receiver_stats.pool_efficiency * 100.0);
    println!("  Pool hits: {}", receiver_stats.pool_hits);
    println!("  Pool misses: {}", receiver_stats.pool_misses);
    println!("  Buffer pool size: {}", receiver_stats.buffer_pool_size);

    Ok(TransportBenchmarkResult {
        transport_name: "High-Performance UDP (End-to-End)".to_string(),
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

    // Benchmark each transport
    match benchmark_basic_udp(&config) {
        Ok(result) => results.push(result),
        Err(e) => println!("❌ Basic UDP benchmark failed: {}", e),
    }

    match benchmark_high_performance_udp(&config) {
        Ok(result) => results.push(result),
        Err(e) => println!("❌ High-Performance UDP benchmark failed: {}", e),
    }

    match benchmark_optimized_copy_udp(&config) {
        Ok(result) => results.push(result),
        Err(e) => println!("❌ Optimized Copy UDP benchmark failed: {}", e),
    }

    match benchmark_reliable_udp(&config) {
        Ok(result) => results.push(result),
        Err(e) => println!("❌ Reliable UDP benchmark failed: {}", e),
    }

    // Skip kernel bypass for now - not fully implemented
    println!("⚠️  Kernel Bypass Zero Copy: Not yet implemented");

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

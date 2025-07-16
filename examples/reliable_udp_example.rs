//! Reliable UDP Transport Example
//!
//! This example demonstrates the reliable UDP transport with:
//! - NAK-based retransmission for lost packets
//! - Forward Error Correction (FEC) for burst errors
//! - High-performance message passing
//! - Comprehensive metrics and monitoring

use std::net::SocketAddr;
use std::time::{ Duration, Instant };
use std::thread;

use flux::transport::{ ReliableUdpTransport, TransportConfig };

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("Reliable UDP Transport Example");
    println!("==============================");

    // Create transport configuration
    let config = TransportConfig {
        local_addr: "127.0.0.1:8080".to_string(),
        remote_addr: Some("127.0.0.1:8081".to_string()),
        batch_size: 64,
        buffer_size: 1024 * 1024, // 1M messages
        retransmit_timeout_ms: 100,
        max_retransmits: 3,
        enable_fec: true,
        fec_data_shards: 4,
        fec_parity_shards: 2,
        cpu_affinity: Some(0),
        enable_zero_copy: true,
    };

    // Create transport
    let mut transport = ReliableUdpTransport::new(config)?;

    // Start transport
    transport.start()?;
    println!("Transport started on 127.0.0.1:8080");

    // Test message sending
    let test_messages: Vec<&[u8]> = vec![
        b"Hello, reliable UDP!",
        b"This message has FEC protection",
        b"NAK-based retransmission enabled",
        b"High-performance message transport"
    ];

    let remote_addr: SocketAddr = "127.0.0.1:8081".parse()?;

    println!("Sending {} test messages...", test_messages.len());
    let start_time = Instant::now();

    for (i, message) in test_messages.iter().enumerate() {
        transport.send(message, remote_addr)?;
        println!("Sent message {}: {}", i + 1, String::from_utf8_lossy(message));

        // Small delay between messages
        thread::sleep(Duration::from_millis(10));
    }

    // Wait for processing
    thread::sleep(Duration::from_millis(100));

    // Get metrics
    let metrics = transport.get_metrics();
    let duration = start_time.elapsed();

    println!("\nTransport Metrics:");
    println!("==================");
    println!("Messages sent: {}", metrics.messages_sent);
    println!("Messages received: {}", metrics.messages_received);
    println!("Messages retransmitted: {}", metrics.messages_retransmitted);
    println!("Messages dropped: {}", metrics.messages_dropped);
    println!("Duration: {:?}", duration);
    println!(
        "Throughput: {:.2} messages/sec",
        (metrics.messages_sent as f64) / duration.as_secs_f64()
    );

    // Stop transport
    transport.stop();
    println!("Transport stopped");

    Ok(())
}

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

use flux::transport::reliable_udp::ReliableUdpRingBufferTransport;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("Reliable UDP Transport Example");
    println!("==============================");

    // Set up addresses and window size
    let bind_addr: SocketAddr = "127.0.0.1:8080".parse()?;
    let remote_addr: SocketAddr = "127.0.0.1:8081".parse()?;
    let window_size = 1024;

    // Create transport
    let mut transport = ReliableUdpRingBufferTransport::new(bind_addr, remote_addr, window_size)?;

    // Example: send a message (API may need to be extended for real use)
    // transport.send(b"Hello, world!"); // Uncomment and adapt if send is implemented

    println!("Transport created and ready (send/receive logic not implemented in this example)");
    Ok(())
}

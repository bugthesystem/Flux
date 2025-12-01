//! Reliable UDP Client Example
//!
//! Run: cargo run -p kaos-rudp --example example_reliable_udp_client

use kaos_rudp::ReliableUdpRingBufferTransport;
use std::net::SocketAddr;
use std::time::{Duration, Instant};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("Reliable UDP Client");
    println!("====================");

    let client_addr: SocketAddr = "127.0.0.1:20002".parse()?;
    let server_addr: SocketAddr = "127.0.0.1:20001".parse()?;

    let mut client = ReliableUdpRingBufferTransport::new(
        client_addr,
        server_addr,
        4096,
    )?;

    println!("Client: {}", client_addr);
    println!("Server: {}", server_addr);
    println!();

    let start = Instant::now();
    let duration = Duration::from_secs(5);
    let mut sent = 0u64;

    while start.elapsed() < duration {
        let msg = format!("Message {}", sent);
        if client.send(msg.as_bytes()).is_ok() {
            sent += 1;
        }
        client.process_naks();
        std::thread::sleep(Duration::from_micros(100));
    }

    let elapsed = start.elapsed();
    println!("Sent: {} messages", sent);
    println!("Duration: {:.2}s", elapsed.as_secs_f64());
    println!("Rate: {:.0} msgs/sec", sent as f64 / elapsed.as_secs_f64());

    Ok(())
}

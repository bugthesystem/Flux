//! Reliable UDP Server Example
//!
//! Run: cargo run -p kaos-rudp --example example_reliable_udp_server

use kaos_rudp::ReliableUdpRingBufferTransport;
use std::net::SocketAddr;
use std::time::Duration;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("Reliable UDP Server");
    println!("====================");

    let server_addr: SocketAddr = "127.0.0.1:20001".parse()?;
    let client_addr: SocketAddr = "127.0.0.1:20002".parse()?;

    let mut server = ReliableUdpRingBufferTransport::new(
        server_addr,
        client_addr,
        4096,
    )?;

    println!("Listening on: {}", server_addr);
    println!("Expecting client: {}", client_addr);
    println!();

    let mut count = 0u64;

    loop {
        server.receive_batch_with(64, |data| {
            count += 1;
            let msg = String::from_utf8_lossy(data);
            println!("[{}] {}", count, msg.trim());
        });

        server.process_naks();
        std::thread::sleep(Duration::from_millis(1));
    }
}

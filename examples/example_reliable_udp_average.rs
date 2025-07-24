use flux::{ ReliableUdpRingBufferTransport };
use flux::transport::reliable_udp::ReliableUdpConfig;
use std::thread;
use std::time::Instant;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("Flux Reliable UDP Average Calculator Example");
    println!("=========================================");

    let server_addr = "127.0.0.1:8084";
    let client_addr = "127.0.0.1:8085";

    println!("Server: {}", server_addr);
    println!("Client: {}", client_addr);

    let server_handle = thread::spawn(move || {
        if let Err(e) = run_server(server_addr, client_addr) {
            eprintln!("Server error: {}", e);
        }
    });

    thread::sleep(std::time::Duration::from_millis(100));

    let client_handle = thread::spawn(move || {
        if let Err(e) = run_client(client_addr, server_addr) {
            eprintln!("Client error: {}", e);
        }
    });

    client_handle.join().unwrap();
    server_handle.join().unwrap(); // Wait for server to finish
    println!("Reliable UDP Average example completed!");
    Ok(())
}

fn run_server(addr: &str, remote_addr: &str) -> Result<(), Box<dyn std::error::Error>> {
    println!("Starting Flux Reliable UDP Server on {}", addr);
    let config = ReliableUdpConfig {
        local_addr: addr.to_string(),
        remote_addr: remote_addr.to_string(),
        window_size: 1024,
    };
    let mut transport = ReliableUdpRingBufferTransport::auto(config)?;
    // Print the actual socket address after binding
    if let Ok(sock) = std::net::UdpSocket::bind(addr) {
        println!("[Server] Bound UDP socket: {}", sock.local_addr()?);
    }
    println!("[Server] Remote address: {}", remote_addr);
    println!("Server listening on {}", addr);

    let mut sum: u64 = 0;
    let mut count: u64 = 0;
    let start_time = Instant::now();
    let mut last_message_time = Instant::now();
    let mut end_received = false;

    println!("[SERVER INVESTIGATE] Entering receive loop");
    loop {
        let recv_result = transport.receive();
        match recv_result {
            Some(msg_box) => {
                let msg: &[u8] = &msg_box[..];
                // Find the first null byte (0) to determine the actual message length
                let msg_len = msg
                    .iter()
                    .position(|&b| b == 0)
                    .unwrap_or(msg.len());
                let msg = &msg[..msg_len];
                println!("[SERVER DEBUG] Raw message bytes: {:?}", msg);
                match std::str::from_utf8(msg) {
                    Ok(s) => {
                        println!("[SERVER DEBUG] Received string: '{}'", &s);
                        if s.trim() == "END" {
                            end_received = true;
                            break;
                        }
                        match s.trim().parse::<u64>() {
                            Ok(n) => {
                                sum += n;
                                count += 1;
                            }
                            Err(e) => {
                                println!("[SERVER DEBUG] Failed to parse number: {}", e);
                            }
                        }
                    }
                    Err(e) => {
                        println!("[SERVER DEBUG] Failed to parse utf8: {}", e);
                    }
                }
                last_message_time = Instant::now();
            }
            None => {
                println!("[SERVER INVESTIGATE] transport.receive() returned None");
                if last_message_time.elapsed().as_secs_f32() > 2.0 {
                    println!("[Server] Timeout: No message received for 2 seconds. Exiting.");
                    break;
                }
                // Sleep a bit to avoid busy loop
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
        }
    }

    let duration = start_time.elapsed();
    let average = if count > 0 { (sum as f64) / (count as f64) } else { 0.0 };
    println!("Server Statistics:");
    println!("  Numbers received: {}", count);
    println!("  Sum: {}", sum);
    println!("  Average: {:.2}", average);
    println!("  Duration: {:?}", duration);
    if !end_received {
        println!(
            "Warning: Server exited due to timeout, not END message. Some messages may be lost."
        );
    }
    Ok(())
}

fn run_client(local_addr: &str, server_addr: &str) -> Result<(), Box<dyn std::error::Error>> {
    println!("Starting Flux Reliable UDP Client");
    let config = ReliableUdpConfig {
        local_addr: local_addr.to_string(),
        remote_addr: server_addr.to_string(),
        window_size: 1024,
    };
    let mut transport = ReliableUdpRingBufferTransport::auto(config)?;
    // Print the actual socket address after binding
    if let Ok(sock) = std::net::UdpSocket::bind(local_addr) {
        println!("[Client] Bound UDP socket: {}", sock.local_addr()?);
    }
    println!("[Client] Remote address: {}", server_addr);
    println!("Client connected to {}", server_addr);

    let numbers_to_send = 1000;
    let start_time = Instant::now();
    for i in 1..=numbers_to_send {
        let message = i.to_string();
        println!("[Client] Sending: {}", message);
        if let Err(e) = transport.send(message.as_bytes()) {
            println!("[Client] Send error: {}", e);
        }
    }
    println!("[Client] Sending: END");
    if let Err(e) = transport.send(b"END") {
        println!("[Client] Send error: {}", e);
    }
    let duration = start_time.elapsed();
    println!("Client Statistics:");
    println!("  Numbers sent: {}", numbers_to_send);
    println!("  Duration: {:?}", duration);
    Ok(())
}

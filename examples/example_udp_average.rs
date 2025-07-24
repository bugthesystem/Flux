use flux::{ UdpRingBufferTransport, UdpTransportConfig };
use std::thread;
use std::time::Instant;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("Flux UDP Average Calculator Example");
    println!("===============================");

    let server_addr = "127.0.0.1:8086";
    let client_addr = "127.0.0.1:8087";

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
    server_handle.join().unwrap();
    println!("UDP Average example completed!");
    Ok(())
}

fn run_server(addr: &str, remote_addr: &str) -> Result<(), Box<dyn std::error::Error>> {
    println!("Starting Flux UDP Server on {}", addr);
    let config = UdpTransportConfig {
        local_addr: addr.to_string(),
        buffer_size: 4096,
        batch_size: 64,
        non_blocking: false,
        socket_timeout_ms: 100,
    };
    let mut transport = UdpRingBufferTransport::auto(config)?;
    println!("Server listening on {}", addr);

    let mut sum: u64 = 0;
    let mut count: u64 = 0;
    let start_time = Instant::now();
    let mut last_message_time = Instant::now();
    let mut end_received = false;

    loop {
        match transport.receive()? {
            Some((data, _client_addr)) => {
                let message = String::from_utf8_lossy(&data);
                let trimmed = message.trim();
                last_message_time = Instant::now();
                if trimmed == "END" {
                    end_received = true;
                    break;
                }
                if let Ok(num) = trimmed.parse::<u64>() {
                    sum += num;
                    count += 1;
                }
            }
            None => {
                if last_message_time.elapsed().as_secs_f32() > 2.0 {
                    println!("[Server] Timeout: No message received for 2 seconds. Exiting.");
                    break;
                }
                continue;
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
    println!("Starting Flux UDP Client");
    let config = UdpTransportConfig {
        local_addr: local_addr.to_string(),
        buffer_size: 4096,
        batch_size: 64,
        non_blocking: false,
        socket_timeout_ms: 100,
    };
    let mut transport = UdpRingBufferTransport::auto(config)?;
    let server_addr = server_addr.parse()?;
    println!("Client connected to {}", server_addr);

    let numbers_to_send = 1000;
    let start_time = Instant::now();
    for i in 1..=numbers_to_send {
        let message = i.to_string();
        transport.send(message.as_bytes(), server_addr)?;
    }
    transport.send(b"END", server_addr)?;
    let duration = start_time.elapsed();
    println!("Client Statistics:");
    println!("  Numbers sent: {}", numbers_to_send);
    println!("  Duration: {:?}", duration);
    Ok(())
}

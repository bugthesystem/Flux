use flux::{ ReliableUdpRingBufferTransport };
use flux::transport::reliable_udp::ReliableUdpConfig;
use std::time::Instant;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("Flux Reliable UDP Transport Example");
    println!("==============================");

    // Configuration
    let server_addr = "127.0.0.1:8082";
    let client_addr = "127.0.0.1:8083";

    println!("Server: {}", server_addr);
    println!("Client: {}", client_addr);

    // Start server and client
    let _server_handle = std::thread::spawn(move || {
        if let Err(e) = run_server(server_addr, client_addr) {
            eprintln!("Server error: {}", e);
        }
    });

    // Give server time to start
    std::thread::sleep(std::time::Duration::from_millis(100));

    let client_handle = std::thread::spawn(move || {
        if let Err(e) = run_client(client_addr, server_addr) {
            eprintln!("Client error: {}", e);
        }
    });

    // Wait for both to complete
    client_handle.join().unwrap();

    // Give server time to process remaining messages
    std::thread::sleep(std::time::Duration::from_millis(100));

    println!("Reliable UDP Transport example completed!");
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

    println!("Server listening on {}", addr);

    // Message processing loop
    let mut message_count = 0;
    let start_time = Instant::now();

    loop {
        match transport.receive() {
            Some(data_box) => {
                let data = &*data_box;
                let message = String::from_utf8_lossy(data);
                message_count += 1;

                println!("Received[{}]: {}", message_count, message.trim());

                // Echo the message back
                let response = format!("Echo: {}", message.trim());
                transport.send(response.as_bytes())?;

                // Check if we received the end signal
                if message.trim() == "END" {
                    break;
                }
            }
            None => {
                // No message received, continue
                continue;
            }
        }
    }

    let duration = start_time.elapsed();
    let throughput = (message_count as f64) / duration.as_secs_f64();

    println!("Server Statistics:");
    println!("  Messages processed: {}", message_count);
    println!("  Duration: {:?}", duration);
    println!("  Throughput: {:.0} messages/sec", throughput);

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

    let server_addr = server_addr;

    println!("Client connected to {}", server_addr);

    // Send test messages
    let messages_to_send = 1000;
    let start_time = Instant::now();

    for i in 0..messages_to_send {
        let message = format!("Message {}: Hello from Flux client!", i);

        // Send message
        transport.send(message.as_bytes())?;

        // Wait for response
        match transport.receive() {
            Some(data_box) => {
                let data = &*data_box;
                let response = String::from_utf8_lossy(data);
                if i < 5 || i % 100 == 0 {
                    println!("Response[{}]: {}", i, response.trim());
                }
            }
            None => {
                println!("Timeout waiting for response {}", i);
            }
        }
    }

    // Send end signal
    transport.send(b"END")?;

    let duration = start_time.elapsed();
    let throughput = (messages_to_send as f64) / duration.as_secs_f64();

    println!("Client Statistics:");
    println!("  Messages sent: {}", messages_to_send);
    println!("  Duration: {:?}", duration);
    println!("  Throughput: {:.0} messages/sec", throughput);

    Ok(())
}

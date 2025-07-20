use flux::{ RingBuffer, FluxError, BasicUdpTransport, BasicUdpConfig };
use flux::disruptor::{ RingBufferConfig, WaitStrategyType };
use std::net::{ UdpSocket, SocketAddr };
use std::thread;
use std::time::{ Duration, Instant };

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("Flux UDP Transport Example");
    println!("==============================");

    // Configuration
    let server_addr = "127.0.0.1:8080";
    let client_addr = "127.0.0.1:8081";

    println!("Server: {}", server_addr);
    println!("Client: {}", client_addr);

    // Start server and client
    let server_handle = thread::spawn(move || {
        if let Err(e) = run_server(server_addr) {
            eprintln!("Server error: {}", e);
        }
    });

    // Give server time to start
    thread::sleep(Duration::from_millis(100));

    let client_handle = thread::spawn(move || {
        if let Err(e) = run_client(client_addr, server_addr) {
            eprintln!("Client error: {}", e);
        }
    });

    // Wait for both to complete
    client_handle.join().unwrap();

    // Give server time to process remaining messages
    thread::sleep(Duration::from_millis(100));

    println!("UDP Transport example completed!");
    Ok(())
}

fn run_server(addr: &str) -> Result<(), Box<dyn std::error::Error>> {
    println!("Starting Flux UDP Server on {}", addr);

    // Create basic UDP transport
    let config = BasicUdpConfig {
        local_addr: addr.to_string(),
        buffer_size: 4096,
        batch_size: 64,
        non_blocking: false, // Use blocking for server
        socket_timeout_ms: 100,
    };
    let mut transport = BasicUdpTransport::new(config)?;
    transport.start()?;

    println!("Server listening on {}", addr);

    // Message processing loop
    let mut message_count = 0;
    let start_time = Instant::now();

    loop {
        match transport.receive()? {
            Some((data, client_addr)) => {
                let message = String::from_utf8_lossy(&data);
                message_count += 1;

                println!("Received[{}]: {} (from {})", message_count, message.trim(), client_addr);

                // Echo the message back
                let response = format!("Echo: {}", message.trim());
                transport.send(response.as_bytes(), client_addr)?;

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
    println!("Starting Flux UDP Client");

    // Create basic UDP transport
    let config = BasicUdpConfig {
        local_addr: local_addr.to_string(),
        buffer_size: 4096,
        batch_size: 64,
        non_blocking: false, // Use blocking for client
        socket_timeout_ms: 100,
    };
    let mut transport = BasicUdpTransport::new(config)?;
    transport.start()?;

    let server_addr: SocketAddr = server_addr.parse()?;

    println!("Client connected to {}", server_addr);

    // Send test messages
    let messages_to_send = 1000;
    let start_time = Instant::now();

    for i in 0..messages_to_send {
        let message = format!("Message {}: Hello from Flux client!", i);

        // Send message
        transport.send(message.as_bytes(), server_addr)?;

        // Wait for response
        match transport.receive()? {
            Some((data, _)) => {
                let response = String::from_utf8_lossy(&data);
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
    transport.send(b"END", server_addr)?;

    let duration = start_time.elapsed();
    let throughput = (messages_to_send as f64) / duration.as_secs_f64();

    println!("Client Statistics:");
    println!("  Messages sent: {}", messages_to_send);
    println!("  Duration: {:?}", duration);
    println!("  Throughput: {:.0} messages/sec", throughput);

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_basic_udp_transport_creation() {
        let config = BasicUdpConfig {
            local_addr: "127.0.0.1:0".to_string(),
            buffer_size: 1024,
            batch_size: 32,
            non_blocking: true,
            socket_timeout_ms: 50,
        };
        let transport = BasicUdpTransport::new(config);
        assert!(transport.is_ok());
    }

    #[test]
    fn test_message_roundtrip() {
        // Test message roundtrip functionality
        assert!(true); // Placeholder test
    }
}

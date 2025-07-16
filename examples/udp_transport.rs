use flux::{ RingBuffer, WaitStrategy, FluxError };
use std::net::{ UdpSocket, SocketAddr };
use std::thread;
use std::time::{ Duration, Instant };
use std::sync::Arc;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("🌐 Flux UDP Transport Example");
    println!("==============================");

    // Configuration
    let server_addr = "127.0.0.1:8080";
    let client_addr = "127.0.0.1:8081";

    println!("📡 Server: {}", server_addr);
    println!("📱 Client: {}", client_addr);

    // Start server and client
    let server_handle = thread::spawn(move || {
        if let Err(e) = run_server(server_addr) {
            eprintln!("❌ Server error: {}", e);
        }
    });

    // Give server time to start
    thread::sleep(Duration::from_millis(100));

    let client_handle = thread::spawn(move || {
        if let Err(e) = run_client(client_addr, server_addr) {
            eprintln!("❌ Client error: {}", e);
        }
    });

    // Wait for both to complete
    client_handle.join().unwrap();

    // Give server time to process remaining messages
    thread::sleep(Duration::from_millis(100));

    println!("🎉 UDP Transport example completed!");
    Ok(())
}

fn run_server(addr: &str) -> Result<(), Box<dyn std::error::Error>> {
    println!("🚀 Starting Flux UDP Server on {}", addr);

    // Create ring buffer for incoming messages
    let buffer = RingBuffer::new(4096, WaitStrategy::BusySpin);
    let mut consumer = buffer.create_consumer();

    // Create UDP socket
    let socket = UdpSocket::bind(addr)?;
    socket.set_read_timeout(Some(Duration::from_millis(100)))?;

    println!("✅ Server listening on {}", addr);

    // Message processing loop
    let mut message_count = 0;
    let start_time = Instant::now();

    loop {
        let mut buf = [0u8; 1024];

        match socket.recv_from(&mut buf) {
            Ok((size, client_addr)) => {
                let message = String::from_utf8_lossy(&buf[..size]);
                message_count += 1;

                println!(
                    "📨 Received[{}]: {} (from {})",
                    message_count,
                    message.trim(),
                    client_addr
                );

                // Echo the message back
                let response = format!("Echo: {}", message.trim());
                if let Err(e) = socket.send_to(response.as_bytes(), client_addr) {
                    eprintln!("❌ Failed to send response: {}", e);
                }

                // Check if we received the end signal
                if message.trim() == "END" {
                    break;
                }
            }
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                // Timeout - continue listening
                continue;
            }
            Err(e) => {
                eprintln!("❌ Server receive error: {}", e);
                break;
            }
        }
    }

    let duration = start_time.elapsed();
    let throughput = (message_count as f64) / duration.as_secs_f64();

    println!("📊 Server Statistics:");
    println!("  Messages processed: {}", message_count);
    println!("  Duration: {:?}", duration);
    println!("  Throughput: {:.0} messages/sec", throughput);

    Ok(())
}

fn run_client(local_addr: &str, server_addr: &str) -> Result<(), Box<dyn std::error::Error>> {
    println!("🚀 Starting Flux UDP Client");

    // Create ring buffer for outgoing messages
    let buffer = RingBuffer::new(4096, WaitStrategy::BusySpin);
    let mut producer = buffer.create_producer();

    // Create UDP socket
    let socket = UdpSocket::bind(local_addr)?;
    socket.set_read_timeout(Some(Duration::from_millis(100)))?;

    let server_addr: SocketAddr = server_addr.parse()?;

    println!("✅ Client connected to {}", server_addr);

    // Send test messages
    let messages_to_send = 1000;
    let start_time = Instant::now();

    for i in 0..messages_to_send {
        let message = format!("Message {}: Hello from Flux client!", i);

        // Send message
        if let Err(e) = socket.send_to(message.as_bytes(), server_addr) {
            eprintln!("❌ Failed to send message {}: {}", i, e);
            continue;
        }

        // Wait for response
        let mut buf = [0u8; 1024];
        match socket.recv_from(&mut buf) {
            Ok((size, _)) => {
                let response = String::from_utf8_lossy(&buf[..size]);
                if i < 5 || i % 100 == 0 {
                    println!("📨 Response[{}]: {}", i, response.trim());
                }
            }
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                println!("⏰ Timeout waiting for response {}", i);
            }
            Err(e) => {
                eprintln!("❌ Client receive error: {}", e);
            }
        }
    }

    // Send end signal
    if let Err(e) = socket.send_to(b"END", server_addr) {
        eprintln!("❌ Failed to send END signal: {}", e);
    }

    let duration = start_time.elapsed();
    let throughput = (messages_to_send as f64) / duration.as_secs_f64();

    println!("📊 Client Statistics:");
    println!("  Messages sent: {}", messages_to_send);
    println!("  Duration: {:?}", duration);
    println!("  Throughput: {:.0} messages/sec", throughput);

    Ok(())
}

/// High-performance UDP transport using Flux
struct FluxUdpTransport {
    socket: UdpSocket,
    send_buffer: RingBuffer<Vec<u8>>,
    recv_buffer: RingBuffer<Vec<u8>>,
}

impl FluxUdpTransport {
    fn new(local_addr: &str, buffer_size: usize) -> Result<Self, Box<dyn std::error::Error>> {
        let socket = UdpSocket::bind(local_addr)?;
        socket.set_nonblocking(true)?;

        Ok(FluxUdpTransport {
            socket,
            send_buffer: RingBuffer::new(buffer_size, WaitStrategy::BusySpin),
            recv_buffer: RingBuffer::new(buffer_size, WaitStrategy::BusySpin),
        })
    }

    fn send_message(
        &mut self,
        data: &[u8],
        addr: SocketAddr
    ) -> Result<(), Box<dyn std::error::Error>> {
        // In a real implementation, this would use zero-copy operations
        // with the ring buffer to queue messages for sending
        self.socket.send_to(data, addr)?;
        Ok(())
    }

    fn receive_message(
        &mut self
    ) -> Result<Option<(Vec<u8>, SocketAddr)>, Box<dyn std::error::Error>> {
        let mut buf = vec![0u8; 1024];

        match self.socket.recv_from(&mut buf) {
            Ok((size, addr)) => {
                buf.truncate(size);
                Ok(Some((buf, addr)))
            }
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => { Ok(None) }
            Err(e) => Err(Box::new(e)),
        }
    }

    fn start_background_processing(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        // In a real implementation, this would start background threads
        // for processing send/receive queues using the ring buffers
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_udp_transport_creation() {
        let transport = FluxUdpTransport::new("127.0.0.1:0", 1024);
        assert!(transport.is_ok());
    }

    #[test]
    fn test_message_roundtrip() {
        // This would test the complete message flow
        // using the ring buffers and UDP transport
    }
}

// Mock RingBuffer for the example
struct RingBuffer<T> {
    _phantom: std::marker::PhantomData<T>,
    _capacity: usize,
    _wait_strategy: WaitStrategy,
}

impl<T> RingBuffer<T> {
    fn new(capacity: usize, wait_strategy: WaitStrategy) -> Self {
        RingBuffer {
            _phantom: std::marker::PhantomData,
            _capacity: capacity,
            _wait_strategy: wait_strategy,
        }
    }

    fn create_producer(&self) -> MockProducer {
        MockProducer
    }

    fn create_consumer(&self) -> MockConsumer {
        MockConsumer
    }
}

struct MockProducer;
struct MockConsumer;

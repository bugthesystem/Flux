use flux::{ ReliableUdpRingBufferTransport };
use flux::transport::reliable_udp::ReliableUdpConfig;
use std::thread;
use std::time::Instant;
use std::collections::HashSet;
use std::fs::File;
use std::io::Write;

const TOTAL_NUMBERS: u64 = 100;
const BATCH_SIZE: usize = 10;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("Flux Reliable UDP Batch Average Example");
    println!("======================================");

    let server_addr = "127.0.0.1:8090";
    let client_addr = "127.0.0.1:8091";

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
    println!("Reliable UDP Batch Average example completed!");
    Ok(())
}

fn run_server(addr: &str, remote_addr: &str) -> Result<(), Box<dyn std::error::Error>> {
    println!("Starting Flux Reliable UDP Server on {}", addr);
    let config = ReliableUdpConfig {
        local_addr: addr.to_string(),
        remote_addr: remote_addr.to_string(),
        window_size: 4096,
    };
    let mut transport = ReliableUdpRingBufferTransport::auto(config)?;
    println!("Server listening on {}", addr);

    let mut sum: u64 = 0;
    let mut count: u64 = 0;
    let start_time = Instant::now();
    let mut end_received = false;
    let mut received_seqs = HashSet::new();

    while !end_received {
        let batch = transport.receive_batch(BATCH_SIZE);
        if batch.is_empty() {
            std::thread::sleep(std::time::Duration::from_millis(10));
            // Timeout: break if no messages for 2 seconds
            if start_time.elapsed().as_secs() > 2 {
                println!("[SERVER DEBUG] Timeout in receive loop");
                break;
            }
            continue;
        }
        for msg_box in batch {
            // Trim to first null byte (0) for robustness
            let msg: &[u8] = &msg_box[..];
            let msg_len = msg
                .iter()
                .position(|&b| b == 0)
                .unwrap_or(msg.len());
            let msg = &msg[..msg_len];
            match std::str::from_utf8(msg) {
                Ok(s) => {
                    if s.trim() == "END" {
                        println!("[SERVER DEBUG] Received END message");
                        end_received = true;
                        break;
                    }
                    match s.trim().parse::<u64>() {
                        Ok(n) => {
                            sum += n;
                            count += 1;
                            received_seqs.insert(n);
                        }
                        Err(e) => {
                            println!("[SERVER DEBUG] Failed to parse number: {}", e);
                        }
                    }
                }
                Err(e) => {
                    println!("[SERVER DEBUG] Failed to parse UTF-8: {}", e);
                }
            }
        }
    }
    // Drain phase: continue receiving for 500ms to allow for late/retransmitted packets, but break if no messages for 2 seconds
    let drain_start = Instant::now();
    let mut last_msg_time = Instant::now();
    while drain_start.elapsed().as_millis() < 500 {
        let batch = transport.receive_batch(BATCH_SIZE);
        if batch.is_empty() {
            std::thread::sleep(std::time::Duration::from_millis(10));
            if last_msg_time.elapsed().as_secs() > 2 {
                println!("[SERVER DEBUG] Timeout in drain phase");
                break;
            }
            continue;
        }
        last_msg_time = Instant::now();
        for msg_box in batch {
            let msg: &[u8] = &msg_box[..];
            let msg_len = msg
                .iter()
                .position(|&b| b == 0)
                .unwrap_or(msg.len());
            let msg = &msg[..msg_len];
            match std::str::from_utf8(msg) {
                Ok(s) => {
                    if s.trim() == "END" {
                        continue;
                    }
                    match s.trim().parse::<u64>() {
                        Ok(n) => {
                            sum += n;
                            count += 1;
                            received_seqs.insert(n);
                        }
                        Err(_) => {}
                    }
                }
                Err(_) => {}
            }
        }
    }
    let duration = start_time.elapsed();
    let average = if count > 0 { (sum as f64) / (count as f64) } else { 0.0 };
    // Print sequence info to log file
    let mut log = File::create("server_seq_log.txt").unwrap();
    let mut seqs: Vec<u64> = received_seqs.iter().copied().collect();
    seqs.sort_unstable();
    writeln!(log, "Received sequence numbers: {:?}", seqs).unwrap();
    if let Some(&max_seq) = seqs.iter().max() {
        writeln!(log, "Highest received sequence: {}", max_seq).unwrap();
    }
    writeln!(log, "Expected next sequence: {}", seqs.len() + 1).unwrap();
    writeln!(log, "Total received: {}", seqs.len()).unwrap();
    println!("Server Statistics:");
    println!("  Numbers received: {}", count);
    println!("  Sum: {}", sum);
    println!("  Average: {:.2}", average);
    println!("  Duration: {:.2?}", duration);
    Ok(())
}

fn run_client(local_addr: &str, server_addr: &str) -> Result<(), Box<dyn std::error::Error>> {
    println!("Starting Flux Reliable UDP Client");
    let config = ReliableUdpConfig {
        local_addr: local_addr.to_string(),
        remote_addr: server_addr.to_string(),
        window_size: 4096,
    };
    let mut transport = ReliableUdpRingBufferTransport::auto(config)?;
    println!("Client connected to {}", server_addr);
    let start_time = Instant::now();

    let mut numbers_sent = 0;
    let mut batch = Vec::with_capacity(BATCH_SIZE);
    for n in 1..=TOTAL_NUMBERS {
        batch.push(n.to_string().into_bytes());
        if batch.len() == BATCH_SIZE {
            let batch_refs: Vec<&[u8]> = batch
                .iter()
                .map(|v| v.as_slice())
                .collect();
            transport.send_batch(&batch_refs)?;
            println!(
                "[CLIENT DEBUG] Sent batch: {:?}",
                &batch_refs
                    .iter()
                    .map(|b| std::str::from_utf8(b).unwrap_or(""))
                    .collect::<Vec<_>>()
            );
            numbers_sent += batch.len();
            batch.clear();
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
    }
    // Send any remaining numbers
    if !batch.is_empty() {
        let batch_refs: Vec<&[u8]> = batch
            .iter()
            .map(|v| v.as_slice())
            .collect();
        transport.send_batch(&batch_refs)?;
        println!(
            "[CLIENT DEBUG] Sent final batch: {:?}",
            &batch_refs
                .iter()
                .map(|b| std::str::from_utf8(b).unwrap_or(""))
                .collect::<Vec<_>>()
        );
        numbers_sent += batch.len();
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    // Send END message to signal completion
    let end_seq = transport.get_next_send_seq();
    transport.send_batch(&[b"END"])?;
    println!("[CLIENT DEBUG] Sent END message with seq {}", end_seq);
    let duration = start_time.elapsed();
    println!("Client Statistics:");
    println!("  Numbers sent: {}", numbers_sent);
    println!("  Duration: {:.2?}", duration);
    Ok(())
}

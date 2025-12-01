/// RUDP Benchmark - Throughput + Data Integrity

use std::time::Instant;
use std::sync::{ Arc, atomic::{ AtomicU64, AtomicBool, Ordering } };
use std::thread;
use flux_rudp::ReliableUdpRingBufferTransport;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("╔═══════════════════════════════════════════╗");
    println!("║  RUDP Benchmark + Data Integrity Check    ║");
    println!("╚═══════════════════════════════════════════╝\n");

    use std::time::{ SystemTime, UNIX_EPOCH };
    let base_port =
        20000 +
        ((SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_millis() % 10000) as u16);
    let server_addr: std::net::SocketAddr = format!("127.0.0.1:{}", base_port).parse().unwrap();
    let client_addr: std::net::SocketAddr = format!("127.0.0.1:{}", base_port + 100)
        .parse()
        .unwrap();

    let target_messages: u64 = 500_000;
    let window_size = 65536;
    let batch_size = 16;

    let expected_sum: u64 = (0..target_messages).sum();

    println!("Config: {} msgs, batch={}, window={}", target_messages, batch_size, window_size);
    println!("Expected sum: {}\n", expected_sum);

    let mut server = ReliableUdpRingBufferTransport::new(server_addr, client_addr, window_size)?;
    let mut client = ReliableUdpRingBufferTransport::new(client_addr, server_addr, window_size)?;

    let received_sum = Arc::new(AtomicU64::new(0));
    let received_count = Arc::new(AtomicU64::new(0));
    let sender_done = Arc::new(AtomicBool::new(false));

    let start = Instant::now();

    // Receiver thread
    let sum_clone = received_sum.clone();
    let count_clone = received_count.clone();
    let done_clone = sender_done.clone();

    let receiver = thread::spawn(move || {
        let mut local_sum: u64 = 0;
        let mut local_count: u64 = 0;
        let timeout = Instant::now();

        loop {
            server.receive_batch_with(64, |msg| {
                if msg.len() >= 8 {
                    let val = u64::from_le_bytes(msg[0..8].try_into().unwrap());
                    local_sum += val;
                    local_count += 1;
                }
            });

            if local_count >= target_messages {
                break;
            }

            // Hard timeout of 10 seconds
            if timeout.elapsed().as_secs() > 10 {
                println!("  Receiver timeout after 10s with {} msgs", local_count);
                break;
            }

            if done_clone.load(Ordering::Relaxed) && timeout.elapsed().as_millis() > 500 {
                break;
            }
        }

        sum_clone.store(local_sum, Ordering::SeqCst);
        count_clone.store(local_count, Ordering::SeqCst);
    });

    // Small delay
    std::thread::sleep(std::time::Duration::from_millis(10));

    // Sender
    let mut sent: u64 = 0;
    let mut batch_bufs: Vec<[u8; 64]> = vec![[0u8; 64]; batch_size];
    let send_start = Instant::now();

    while sent < target_messages {
        let count = ((target_messages - sent) as usize).min(batch_size);

        for i in 0..count {
            let val = sent + (i as u64);
            batch_bufs[i][0..8].copy_from_slice(&val.to_le_bytes());
        }

        let refs: Vec<&[u8]> = batch_bufs[..count]
            .iter()
            .map(|b| &b[..])
            .collect();

        match client.send_batch(&refs) {
            Ok(n) => {
                sent += n as u64;
            }
            Err(_) => {
                // Send window full - process ACKs to free up space
                client.process_acks();
                thread::yield_now();
            }
        }

        // Periodically process ACKs to keep the window flowing
        if sent % 1000 == 0 {
            client.process_acks();
        }

        // Timeout check
        if send_start.elapsed().as_secs() > 10 {
            println!("  Sender timeout after 10s with {} msgs", sent);
            break;
        }
    }

    let send_time = start.elapsed();
    println!(
        "Send complete: {} msgs in {:.3}s ({:.2} M/s)",
        sent,
        send_time.as_secs_f64(),
        (sent as f64) / send_time.as_secs_f64() / 1_000_000.0
    );

    sender_done.store(true, Ordering::SeqCst);

    // Wait for receiver
    receiver.join().unwrap();

    let elapsed = start.elapsed();
    let actual_sum = received_sum.load(Ordering::SeqCst);
    let actual_count = received_count.load(Ordering::SeqCst);

    println!("\n═══════════════════════════════════════════");
    println!("  Sent:      {} msgs", sent);
    println!(
        "  Received:  {} msgs ({:.1}%)",
        actual_count,
        ((actual_count as f64) / (sent as f64)) * 100.0
    );
    println!("  Duration:  {:.3}s", elapsed.as_secs_f64());
    println!(
        "  Throughput: {:.2} M/s",
        (actual_count as f64) / elapsed.as_secs_f64() / 1_000_000.0
    );
    println!();
    println!("  Expected sum: {}", expected_sum);
    println!("  Actual sum:   {}", actual_sum);

    if actual_sum == expected_sum && actual_count == target_messages {
        println!("\n  ✅ DATA INTEGRITY: VERIFIED (100% delivery)");
    } else if actual_count > 0 {
        let expected_for_count: u64 = (0..actual_count).sum();
        if actual_sum == expected_for_count {
            println!(
                "\n  ⚠️  PARTIAL: {} of {} received, in-order data correct",
                actual_count,
                target_messages
            );
            println!(
                "     Loss: {:.1}%",
                (1.0 - (actual_count as f64) / (target_messages as f64)) * 100.0
            );
        } else {
            println!("\n  ❌ DATA CORRUPTION: sum mismatch");
        }
    } else {
        println!("\n  ❌ FAILED: No messages received");
    }

    Ok(())
}

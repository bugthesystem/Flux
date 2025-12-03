//! RUDP Network Benchmark - tests kaos-rudp over real UDP
//!
//! Terminal 1: cargo run -p kaos-driver --release --features reliable --example bench_rudp -- recv
//! Terminal 2: cargo run -p kaos-driver --release --features reliable --example bench_rudp -- send

#[cfg(feature = "reliable")]
fn main() {
    use kaos_rudp::ReliableUdpRingBufferTransport;
    use std::net::SocketAddr;
    use std::time::Instant;
    use std::thread;

    let args: Vec<String> = std::env::args().collect();
    if args.len() < 2 {
        println!("Usage: bench_rudp [recv|send]");
        println!("  Start recv first, then send in another terminal");
        return;
    }

    const N: u64 = 100_000;
    
    match args[1].as_str() {
        "recv" => {
            let bind: SocketAddr = "127.0.0.1:19010".parse().unwrap();  // +19011 for NAK
            let peer: SocketAddr = "127.0.0.1:19000".parse().unwrap();  // sender main
            println!("RUDP RECV {} ← {}", bind, peer);
            
            let mut t = ReliableUdpRingBufferTransport::new(bind, peer, 65536).unwrap();
            let (mut recv, mut start): (u64, Option<Instant>) = (0, None);
            
            while recv < N {
                t.receive_batch_with(64, |_| {
                    if start.is_none() { start = Some(Instant::now()); }
                    recv += 1;
                });
                t.process_acks();
                if recv > 0 && recv % 20000 == 0 { println!("  recv: {}", recv); }
                thread::yield_now();
            }
            
            let e = start.unwrap().elapsed();
            println!("RECV {} in {:?} ({:.2} M/s)", recv, e, recv as f64 / e.as_secs_f64() / 1e6);
        }
        
        "send" => {
            let bind: SocketAddr = "127.0.0.1:19000".parse().unwrap();  // +19001 for NAK
            let peer: SocketAddr = "127.0.0.1:19010".parse().unwrap();  // receiver main
            println!("RUDP SEND {} → {}", bind, peer);
            
            let mut t = ReliableUdpRingBufferTransport::new(bind, peer, 65536).unwrap();
            let data: Vec<[u8; 8]> = (0..64).map(|i| [i as u8; 8]).collect();
            let batch: Vec<&[u8]> = data.iter().map(|d| d.as_slice()).collect();
            
            thread::sleep(std::time::Duration::from_millis(500));
            let start = Instant::now();
            let mut sent = 0u64;
            
            while sent < N {
                if let Ok(n) = t.send_batch(&batch) { sent += n as u64; }
                t.process_acks();
                if sent % 20000 == 0 && sent > 0 { println!("  sent: {}", sent); }
            }
            
            let e = start.elapsed();
            println!("SENT {} in {:?} ({:.2} M/s)", sent, e, sent as f64 / e.as_secs_f64() / 1e6);
        }
        
        _ => println!("Usage: bench_rudp [recv|send]"),
    }
}

#[cfg(not(feature = "reliable"))]
fn main() {
    println!("cargo run -p kaos-driver --release --features reliable --example bench_rudp -- recv");
}

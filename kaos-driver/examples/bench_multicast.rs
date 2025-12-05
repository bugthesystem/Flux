//! Multicast benchmark - all-in-one with message coalescing
//!
//! cargo run -p kaos-driver --release --example bench_multicast

use kaos_ipc::{ Publisher, Subscriber };
use std::net::{ Ipv4Addr, SocketAddr, UdpSocket };
use std::sync::atomic::{ AtomicBool, AtomicU64, Ordering };
use std::sync::Arc;
use std::thread;
use std::time::{ Duration, Instant };

const GROUP: Ipv4Addr = Ipv4Addr::new(239, 255, 0, 1);
const PORT: u16 = 40457;
const N: u64 = 1_000_000;

// Coalesce messages into larger UDP packets (like Aeron)
const COALESCE_SIZE: usize = 1400; // ~MTU
const MSGS_PER_PACKET: usize = COALESCE_SIZE / 8; // 175 msgs per packet

fn main() {
    println!("Kaos Multicast Benchmark (coalesced packets)");
    println!("=============================================");
    println!("Messages: {}, Coalesce: {} msgs/packet", N, MSGS_PER_PACKET);
    println!();

    let running = Arc::new(AtomicBool::new(true));
    let sent_count = Arc::new(AtomicU64::new(0));
    let recv_count = Arc::new(AtomicU64::new(0));

    // Clean up old IPC files
    let _ = std::fs::remove_file("/tmp/mcast-tx");
    let _ = std::fs::remove_file("/tmp/mcast-rx");

    // Create IPC channels
    let mut app_tx = Publisher::create("/tmp/mcast-tx", 64 * 1024).expect("create tx failed");
    let mut drv_rx = Publisher::create("/tmp/mcast-rx", 64 * 1024).expect("create rx failed");

    // Start driver thread
    let r = running.clone();
    let sc = sent_count.clone();
    let rc = recv_count.clone();
    let driver_handle = thread::spawn(move || {
        run_driver(&r, &sc, &rc, &mut drv_rx);
    });

    thread::sleep(Duration::from_millis(200));

    // Start receiver thread
    let r = running.clone();
    let rc = recv_count.clone();
    let receiver_handle = thread::spawn(move || {
        let mut sub = loop {
            match Subscriber::open("/tmp/mcast-rx") {
                Ok(s) => {
                    break s;
                }
                Err(_) => thread::sleep(Duration::from_millis(10)),
            }
        };

        let mut local_recv: u64 = 0;
        let mut start: Option<Instant> = None;
        let timeout = Instant::now();

        while local_recv < N && timeout.elapsed() < Duration::from_secs(30) {
            if let Some(_) = sub.try_receive() {
                if start.is_none() {
                    start = Some(Instant::now());
                }
                local_recv += 1;
                rc.store(local_recv, Ordering::Relaxed);
                if local_recv % 100000 == 0 {
                    println!("  recv: {}", local_recv);
                }
            } else {
                thread::yield_now();
            }
        }

        if let Some(start) = start {
            let elapsed = start.elapsed().as_secs_f64();
            let throughput = (local_recv as f64) / elapsed / 1e6;
            println!("\nRECV: {} in {:.2}s = {:.2} M/s", local_recv, elapsed, throughput);
        } else {
            println!("\nRECV: {} (no messages received)", local_recv);
        }
    });

    // Send messages
    println!("Sending {} messages...", N);
    thread::sleep(Duration::from_millis(300));

    let start = Instant::now();
    let mut local_sent: u64 = 0;

    while local_sent < N {
        if app_tx.send(local_sent).is_ok() {
            local_sent += 1;
            sent_count.store(local_sent, Ordering::Relaxed);
            if local_sent % 100000 == 0 {
                println!("  sent: {}", local_sent);
            }
        } else {
            thread::yield_now();
        }
    }

    let send_elapsed = start.elapsed().as_secs_f64();
    println!(
        "\nSEND: {} in {:.2}s = {:.2} M/s",
        local_sent,
        send_elapsed,
        (local_sent as f64) / send_elapsed / 1e6
    );

    // Wait for receiver
    println!("Waiting for receiver...");
    let _ = receiver_handle.join();

    running.store(false, Ordering::SeqCst);
    let _ = driver_handle.join();

    let _ = std::fs::remove_file("/tmp/mcast-tx");
    let _ = std::fs::remove_file("/tmp/mcast-rx");
}

fn run_driver(
    running: &Arc<AtomicBool>,
    sent_count: &Arc<AtomicU64>,
    recv_count: &Arc<AtomicU64>,
    to_app: &mut Publisher
) {
    let mut from_app = loop {
        match Subscriber::open("/tmp/mcast-tx") {
            Ok(s) => {
                break s;
            }
            Err(_) if running.load(Ordering::Relaxed) => thread::sleep(Duration::from_millis(10)),
            Err(_) => {
                return;
            }
        }
    };

    let socket = create_multicast_socket().expect("multicast socket failed");
    let dest = SocketAddr::new(GROUP.into(), PORT);

    println!("Driver ready (coalesced)");

    let mut driver_sent: u64 = 0;
    let mut driver_recv: u64 = 0;
    let mut batch: Vec<u64> = Vec::with_capacity(MSGS_PER_PACKET);
    let mut coalesce_buf = [0u8; COALESCE_SIZE];
    let mut recv_buf = [0u8; COALESCE_SIZE];

    loop {
        let target_sent = sent_count.load(Ordering::Relaxed);
        let done_sending = driver_sent >= target_sent && target_sent >= N;
        let done_receiving = driver_recv >= N;

        if done_sending && done_receiving {
            break;
        }
        if !running.load(Ordering::Relaxed) && done_sending {
            break;
        }

        // Send: batch from app, coalesce into packet
        batch.clear();
        while batch.len() < MSGS_PER_PACKET && driver_sent < target_sent {
            if let Some(v) = from_app.try_receive() {
                batch.push(v);
            } else {
                break;
            }
        }

        if !batch.is_empty() {
            let packet_len = batch.len() * 8;
            for (i, &v) in batch.iter().enumerate() {
                coalesce_buf[i * 8..(i + 1) * 8].copy_from_slice(&v.to_le_bytes());
            }
            if socket.send_to(&coalesce_buf[..packet_len], dest).is_ok() {
                driver_sent += batch.len() as u64;
            }
        }

        // Recv: unpack coalesced packets
        loop {
            match socket.recv_from(&mut recv_buf) {
                Ok((len, _)) if len >= 8 => {
                    let msg_count = len / 8;
                    for i in 0..msg_count {
                        let v = u64::from_le_bytes(
                            recv_buf[i * 8..(i + 1) * 8].try_into().unwrap()
                        );
                        let _ = to_app.send(v);
                        driver_recv += 1;
                        recv_count.store(driver_recv, Ordering::Relaxed);
                    }
                }
                _ => {
                    break;
                }
            }
        }

        if batch.is_empty() {
            thread::yield_now();
        }
    }

    println!("Driver done: sent={} recv={}", driver_sent, driver_recv);
}

fn create_multicast_socket() -> std::io::Result<UdpSocket> {
    use std::net::SocketAddrV4;

    let socket2 = socket2::Socket::new(
        socket2::Domain::IPV4,
        socket2::Type::DGRAM,
        Some(socket2::Protocol::UDP)
    )?;
    socket2.set_reuse_address(true)?;
    socket2.set_send_buffer_size(8 * 1024 * 1024)?;
    socket2.set_recv_buffer_size(8 * 1024 * 1024)?;

    let addr = SocketAddrV4::new(Ipv4Addr::UNSPECIFIED, PORT);
    socket2.bind(&addr.into())?;

    let socket: UdpSocket = socket2.into();
    socket.join_multicast_v4(&GROUP, &Ipv4Addr::UNSPECIFIED)?;
    socket.set_multicast_loop_v4(true)?;
    socket.set_multicast_ttl_v4(1)?;
    socket.set_nonblocking(true)?;

    Ok(socket)
}

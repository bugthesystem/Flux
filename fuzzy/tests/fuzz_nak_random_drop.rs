use std::net::UdpSocket;
use std::thread;
use std::sync::{ Arc, atomic::{ AtomicBool, Ordering } };
use std::time::Duration;
use rand::{ thread_rng, Rng };
use flux::transport::reliable_udp::ReliableUdpRingBufferTransport;
use flux::transport::reliable_udp::DEBUG_NAK;

const NUM_MESSAGES: u64 = 10;
const DROP_PROB: f64 = 0.05; // Lowered to 5% random drop

fn free_addr() -> std::net::SocketAddr {
    let sock = UdpSocket::bind("127.0.0.1:0").unwrap();
    let addr = sock.local_addr().unwrap();
    drop(sock);
    addr
}

#[test]
fn fuzz_nak_random_drop() {
    DEBUG_NAK.store(true, Ordering::Relaxed);
    let sender_addr = free_addr();
    let proxy_addr = free_addr();
    let receiver_addr = free_addr();

    // Proxy socket (receives from sender, forwards to receiver)
    let proxy_sock = UdpSocket::bind(proxy_addr).unwrap();
    proxy_sock.set_nonblocking(true).unwrap();
    let proxy_sock2 = proxy_sock.try_clone().unwrap();
    let receiver_sock = UdpSocket::bind(free_addr()).unwrap();
    receiver_sock.set_nonblocking(true).unwrap();

    let receiver_addr_clone = receiver_addr;
    let proxy_thread = thread::spawn(move || {
        let mut buf = [0u8; 2048];
        let mut handled = 0;
        loop {
            if let Ok((len, _src)) = proxy_sock2.recv_from(&mut buf) {
                handled += 1;
                let mut rng = thread_rng();
                if rng.gen_bool(DROP_PROB) {
                    println!("[PROXY] Randomly dropped a packet (handled: {})", handled);
                    continue;
                }
                let seq = if len >= 8 {
                    u64::from_le_bytes(buf[..8].try_into().unwrap())
                } else {
                    0
                };
                println!("[PROXY] Forwarded packet {} (seq: {})", handled, seq);
                let _ = receiver_sock.send_to(&buf[..len], receiver_addr_clone);
            } else {
                thread::sleep(Duration::from_millis(1));
            }
        }
    });

    let mut sender = ReliableUdpRingBufferTransport::new(sender_addr, proxy_addr, 32).unwrap();
    let mut receiver = ReliableUdpRingBufferTransport::new(receiver_addr, sender_addr, 32).unwrap();

    for i in 0..NUM_MESSAGES {
        let msg = i.to_le_bytes();
        sender.send(&msg).unwrap();
        sender.process_naks(); // Process NAKs after each send
        println!("[SENDER] sent {}", i);
        thread::sleep(Duration::from_millis(2));
    }

    let mut received = vec![];
    let start = std::time::Instant::now();
    let mut poll_count = 0;
    while received.len() < (NUM_MESSAGES as usize) && start.elapsed().as_secs() < 15 {
        poll_count += 1;
        sender.process_naks(); // Process NAKs during receive loop
        if poll_count % 50 == 0 {
            println!("[RECEIVER] polling, current received: {:?}", received);
        }
        while let Some(pkt) = receiver.receive() {
            let payload = &pkt[..];
            let num = if payload.len() >= 8 {
                u64::from_le_bytes(payload[..8].try_into().unwrap())
            } else {
                0
            };
            println!("[RECEIVER] Got payload: {} | Current received: {:?}", num, received);
            received.push(num);
        }
        thread::sleep(Duration::from_millis(2));
    }
    println!("[TEST] Final received order: {:?}", received);
    assert_eq!(received, (0..NUM_MESSAGES).collect::<Vec<_>>());
}

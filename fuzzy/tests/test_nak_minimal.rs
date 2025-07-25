use std::net::UdpSocket;
use std::thread;
use std::sync::{ Arc, atomic::{ AtomicBool, Ordering } };
use std::time::Duration;
use flux::transport::reliable_udp::ReliableUdpRingBufferTransport;
use flux::transport::reliable_udp::DEBUG_NAK;

#[test]
fn test_nak_retransmission_minimal() {
    DEBUG_NAK.store(true, Ordering::Relaxed);
    fn free_addr() -> std::net::SocketAddr {
        let sock = UdpSocket::bind("127.0.0.1:0").unwrap();
        let addr = sock.local_addr().unwrap();
        drop(sock);
        addr
    }

    let sender_addr = free_addr();
    let proxy_addr = free_addr();
    let receiver_addr = free_addr();

    let proxy_sock = UdpSocket::bind(proxy_addr).unwrap();
    proxy_sock.set_nonblocking(true).unwrap();
    let proxy_sock2 = proxy_sock.try_clone().unwrap();
    let receiver_sock = UdpSocket::bind(free_addr()).unwrap();
    receiver_sock.set_nonblocking(true).unwrap();

    let dropped = Arc::new(AtomicBool::new(false));
    let dropped2 = dropped.clone();
    let receiver_addr_clone = receiver_addr;
    let proxy_thread = thread::spawn(move || {
        let mut buf = [0u8; 2048];
        let mut count = 0;
        loop {
            if let Ok((len, _src)) = proxy_sock2.recv_from(&mut buf) {
                count += 1;
                if count % 2 == 0 {
                    println!("[PROXY] handled {} packets", count);
                }
                let seq = if len >= 8 + 24 {
                    u64::from_le_bytes(buf[24..32].try_into().unwrap())
                } else if len >= 8 {
                    u64::from_le_bytes(buf[..8].try_into().unwrap())
                } else {
                    0
                };
                if seq == 2 && !dropped2.swap(true, Ordering::SeqCst) {
                    println!("[PROXY] Dropping packet with seq {}", seq);
                    continue;
                }
                let _ = receiver_sock.send_to(&buf[..len], receiver_addr_clone);
            } else {
                thread::sleep(Duration::from_millis(1));
            }
        }
    });

    let mut sender = ReliableUdpRingBufferTransport::new(sender_addr, proxy_addr, 32).unwrap();
    let mut receiver = ReliableUdpRingBufferTransport::new(receiver_addr, sender_addr, 32).unwrap();

    for i in 0..5u64 {
        let msg = i.to_le_bytes();
        sender.send(&msg).unwrap();
        sender.process_naks(); // Process NAKs after each send
        if i % 2 == 0 {
            println!("[SENDER] sent {}", i);
        }
        thread::sleep(Duration::from_millis(2));
    }

    let mut received = vec![];
    let start = std::time::Instant::now();
    while received.len() < 5 && start.elapsed().as_secs() < 5 {
        sender.process_naks(); // Process NAKs during receive loop
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
        if received.len() % 2 == 0 && received.len() > 0 {
            println!("[RECEIVER] received {} messages", received.len());
        }
        thread::sleep(Duration::from_millis(2));
    }
    println!("[TEST] Final received order: {:?}", received);
    assert_eq!(received, vec![0, 1, 2, 3, 4]);
}

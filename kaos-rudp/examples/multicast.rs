//! UDP multicast example.
//!
//! Terminal 1: cargo run -p kaos-rudp --example multicast -- recv
//! Terminal 2: cargo run -p kaos-rudp --example multicast -- send

use kaos_rudp::MulticastSocket;
use std::net::Ipv4Addr;

const GROUP: Ipv4Addr = Ipv4Addr::new(239, 255, 0, 1);
const PORT: u16 = 5555;

fn main() {
    let mode = std::env::args().nth(1).unwrap_or_else(|| "recv".into());

    match mode.as_str() {
        "send" => {
            let socket = MulticastSocket::join(("0.0.0.0", PORT), GROUP).unwrap();
            socket.set_loopback(true).unwrap(); // For local testing
            
            for i in 0..10 {
                let msg = format!("message {}", i);
                socket.broadcast(msg.as_bytes()).unwrap();
                println!("sent: {}", msg);
                std::thread::sleep(std::time::Duration::from_millis(100));
            }
        }
        "recv" => {
            let socket = MulticastSocket::join(("0.0.0.0", PORT), GROUP).unwrap();
            socket.set_loopback(true).unwrap();
            
            println!("listening on {}:{}", GROUP, PORT);
            let mut buf = [0u8; 1024];
            loop {
                match socket.recv(&mut buf) {
                    Ok((len, from)) => {
                        let msg = std::str::from_utf8(&buf[..len]).unwrap_or("<binary>");
                        println!("from {}: {}", from, msg);
                    }
                    Err(e) => eprintln!("recv error: {}", e),
                }
            }
        }
        _ => eprintln!("usage: multicast [send|recv]"),
    }
}


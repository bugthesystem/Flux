//! Multicast benchmark - uses sendmmsg batching like kaos-driver.
//!
//! Receiver: cargo run -p kaos-rudp --release --example multicast_bench -- recv
//! Sender:   cargo run -p kaos-rudp --release --example multicast_bench -- send

use std::net::{Ipv4Addr, SocketAddr, UdpSocket};
use std::time::Instant;

const GROUP: Ipv4Addr = Ipv4Addr::new(239, 255, 0, 1);
const PORT: u16 = 40457;
const N: u64 = 1_000_000;
const MSG_SIZE: usize = 64;
const BATCH_SIZE: usize = 64;

fn main() {
    let mode = std::env::args().nth(1).unwrap_or_else(|| "help".into());

    match mode.as_str() {
        "send" => send(),
        "recv" => recv(),
        _ => {
            println!("Kaos Multicast Benchmark");
            println!("========================");
            println!("Usage: multicast_bench [recv|send]");
            println!();
            println!("  recv - start receiver FIRST");
            println!("  send - start sender AFTER receiver");
            println!();
            println!("Conditions: {} messages, {} bytes each", N, MSG_SIZE);
            println!("Batch size: {} (like kaos-driver)", BATCH_SIZE);
        }
    }
}

fn create_multicast_socket(port: u16, is_sender: bool) -> std::io::Result<UdpSocket> {
    use std::net::SocketAddrV4;
    
    let socket2 = socket2::Socket::new(
        socket2::Domain::IPV4,
        socket2::Type::DGRAM,
        Some(socket2::Protocol::UDP),
    )?;
    socket2.set_reuse_address(true)?;
    socket2.set_send_buffer_size(8 * 1024 * 1024)?;
    socket2.set_recv_buffer_size(8 * 1024 * 1024)?;
    
    let bind_port = if is_sender { 0 } else { port };
    let addr = SocketAddrV4::new(Ipv4Addr::UNSPECIFIED, bind_port);
    socket2.bind(&addr.into())?;
    
    let socket: UdpSocket = socket2.into();
    socket.join_multicast_v4(&GROUP, &Ipv4Addr::UNSPECIFIED)?;
    socket.set_multicast_loop_v4(true)?;
    socket.set_multicast_ttl_v4(1)?;
    socket.set_nonblocking(true)?;
    
    Ok(socket)
}

#[cfg(target_os = "linux")]
fn send() {
    use std::mem::MaybeUninit;
    use std::os::unix::io::AsRawFd;
    
    println!("KAOS MULTICAST SEND → {}:{} (sendmmsg)", GROUP, PORT);
    println!("Messages: {}, Size: {}B, Batch: {}", N, MSG_SIZE, BATCH_SIZE);
    
    let socket = create_multicast_socket(PORT, true).unwrap();
    let fd = socket.as_raw_fd();
    let dest = SocketAddr::new(GROUP.into(), PORT);
    let dest_addr = socket2::SockAddr::from(dest);
    
    println!("Starting in 500ms...");
    std::thread::sleep(std::time::Duration::from_millis(500));
    
    // Pre-allocate buffers
    let mut send_bufs = vec![[b'X'; MSG_SIZE]; BATCH_SIZE];
    let mut send_iovecs: Vec<libc::iovec> = vec![unsafe { MaybeUninit::zeroed().assume_init() }; BATCH_SIZE];
    let mut send_msgs: Vec<libc::mmsghdr> = vec![unsafe { MaybeUninit::zeroed().assume_init() }; BATCH_SIZE];
    
    // Setup iovecs and msgs
    for i in 0..BATCH_SIZE {
        send_iovecs[i].iov_base = send_bufs[i].as_mut_ptr() as *mut _;
        send_iovecs[i].iov_len = MSG_SIZE;
        send_msgs[i].msg_hdr.msg_iov = &mut send_iovecs[i];
        send_msgs[i].msg_hdr.msg_iovlen = 1;
        send_msgs[i].msg_hdr.msg_name = dest_addr.as_ptr() as *mut _;
        send_msgs[i].msg_hdr.msg_namelen = dest_addr.len();
    }
    
    let start = Instant::now();
    let mut sent: u64 = 0;
    
    while sent < N {
        let batch = ((N - sent) as usize).min(BATCH_SIZE);
        let n = unsafe { libc::sendmmsg(fd, send_msgs.as_mut_ptr(), batch as u32, 0) };
        if n > 0 {
            sent += n as u64;
            if sent % 100000 < BATCH_SIZE as u64 {
                println!("  sent: {}", sent);
            }
        } else if n < 0 {
            let err = std::io::Error::last_os_error();
            if err.raw_os_error() != Some(libc::EAGAIN) {
                eprintln!("sendmmsg error: {}", err);
                break;
            }
            std::thread::yield_now();
        }
    }
    
    let elapsed = start.elapsed().as_secs_f64();
    let throughput = sent as f64 / elapsed / 1e6;
    println!("\nRESULT: {} messages in {:.2}s = {:.2} M/s", sent, elapsed, throughput);
}

#[cfg(target_os = "linux")]
fn recv() {
    use std::mem::MaybeUninit;
    use std::os::unix::io::AsRawFd;
    
    println!("KAOS MULTICAST RECV ({}:{}) (recvmmsg)", GROUP, PORT);
    println!("Messages: {}, Size: {}B, Batch: {}", N, MSG_SIZE, BATCH_SIZE);
    
    let socket = create_multicast_socket(PORT, false).unwrap();
    let fd = socket.as_raw_fd();
    
    println!("Waiting for messages...");
    
    // Pre-allocate buffers
    let mut recv_bufs = vec![[0u8; MSG_SIZE + 64]; BATCH_SIZE];
    let mut recv_iovecs: Vec<libc::iovec> = vec![unsafe { MaybeUninit::zeroed().assume_init() }; BATCH_SIZE];
    let mut recv_msgs: Vec<libc::mmsghdr> = vec![unsafe { MaybeUninit::zeroed().assume_init() }; BATCH_SIZE];
    
    for i in 0..BATCH_SIZE {
        recv_iovecs[i].iov_base = recv_bufs[i].as_mut_ptr() as *mut _;
        recv_iovecs[i].iov_len = recv_bufs[i].len();
        recv_msgs[i].msg_hdr.msg_iov = &mut recv_iovecs[i];
        recv_msgs[i].msg_hdr.msg_iovlen = 1;
    }
    
    let mut received: u64 = 0;
    let mut start: Option<Instant> = None;
    
    // Set blocking for initial receive
    socket.set_nonblocking(false).unwrap();
    
    while received < N {
        let n = unsafe {
            libc::recvmmsg(
                fd,
                recv_msgs.as_mut_ptr(),
                BATCH_SIZE as u32,
                0, // blocking
                std::ptr::null_mut(),
            )
        };
        
        if n > 0 {
            if start.is_none() {
                start = Some(Instant::now());
            }
            received += n as u64;
            if received % 100000 < BATCH_SIZE as u64 {
                println!("  recv: {}", received);
            }
        } else if n < 0 {
            let err = std::io::Error::last_os_error();
            eprintln!("recvmmsg error: {}", err);
            break;
        }
    }
    
    if let Some(start) = start {
        let elapsed = start.elapsed().as_secs_f64();
        let throughput = received as f64 / elapsed / 1e6;
        println!("\nRESULT: {} messages in {:.2}s = {:.2} M/s", received, elapsed, throughput);
    }
}

// macOS fallback - no sendmmsg
#[cfg(not(target_os = "linux"))]
fn send() {
    println!("KAOS MULTICAST SEND → {}:{}", GROUP, PORT);
    println!("Messages: {}, Size: {}B", N, MSG_SIZE);
    println!("NOTE: macOS doesn't have sendmmsg, using single sends");
    
    let socket = create_multicast_socket(PORT, true).unwrap();
    let dest = SocketAddr::new(GROUP.into(), PORT);
    
    println!("Starting in 500ms...");
    std::thread::sleep(std::time::Duration::from_millis(500));
    
    let msg = vec![b'X'; MSG_SIZE];
    let start = Instant::now();
    let mut sent: u64 = 0;
    
    while sent < N {
        match socket.send_to(&msg, dest) {
            Ok(_) => {
                sent += 1;
                if sent % 100000 == 0 {
                    println!("  sent: {}", sent);
                }
            }
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock 
                   || e.raw_os_error() == Some(55) => {
                std::thread::yield_now();
            }
            Err(e) => {
                eprintln!("send error: {}", e);
                break;
            }
        }
    }
    
    let elapsed = start.elapsed().as_secs_f64();
    let throughput = sent as f64 / elapsed / 1e6;
    println!("\nRESULT: {} messages in {:.2}s = {:.2} M/s", sent, elapsed, throughput);
}

#[cfg(not(target_os = "linux"))]
fn recv() {
    println!("KAOS MULTICAST RECV ({}:{})", GROUP, PORT);
    println!("Messages: {}, Size: {}B", N, MSG_SIZE);
    
    let socket = create_multicast_socket(PORT, false).unwrap();
    socket.set_nonblocking(false).unwrap();
    
    println!("Waiting for messages...");
    
    let mut buf = [0u8; 2048];
    let mut received: u64 = 0;
    let mut start: Option<Instant> = None;
    
    while received < N {
        match socket.recv_from(&mut buf) {
            Ok((len, _)) if len > 0 => {
                if start.is_none() {
                    start = Some(Instant::now());
                }
                received += 1;
                if received % 100000 == 0 {
                    println!("  recv: {}", received);
                }
            }
            Ok(_) => {}
            Err(e) => {
                eprintln!("recv error: {}", e);
                break;
            }
        }
    }
    
    if let Some(start) = start {
        let elapsed = start.elapsed().as_secs_f64();
        let throughput = received as f64 / elapsed / 1e6;
        println!("\nRESULT: {} messages in {:.2}s = {:.2} M/s", received, elapsed, throughput);
    }
}

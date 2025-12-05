//! Kaos Media Driver - Zero-syscall messaging via shared memory.
//!
//! Unicast:   kaos-driver <bind> <peer> [send_path] [recv_path]
//! Multicast: kaos-driver <bind> <multicast_group> --multicast [send_path] [recv_path]
//! Echo:      kaos-driver <bind> --echo
//!
//! Features: --features reliable (kaos-rudp), --features uring (io_uring)

use kaos_ipc::{Publisher, Subscriber};
use std::net::{Ipv4Addr, SocketAddr, UdpSocket};
#[cfg(target_os = "linux")]
use std::os::fd::AsRawFd;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

#[cfg(all(target_os = "linux", feature = "uring"))]
mod uring;
#[cfg(all(target_os = "linux", feature = "xdp"))]
mod xdp;

/// IPC ring buffer size (64K slots, must be power of 2)
const RING_SIZE: usize = 64 * 1024;

/// Batch size for internal batching (64 = good amortization)
const BATCH_SIZE: usize = 64;

/// Message size for multicast
const MSG_SIZE: usize = 64;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let echo = args.iter().any(|a| a == "--echo" || a == "-e");
    let multicast = args.iter().any(|a| a == "--multicast" || a == "-m");

    if args.len() < 2 {
        eprintln!("Kaos Media Driver");
        eprintln!("=================");
        eprintln!("Unicast:   kaos-driver <bind> <peer> [send_path] [recv_path]");
        eprintln!("Multicast: kaos-driver <bind> <group:port> --multicast [send_path] [recv_path]");
        eprintln!("Echo:      kaos-driver <bind> --echo");
        eprintln!();
        eprintln!("Features: --features reliable, --features uring");
        std::process::exit(1);
    }

    let bind: SocketAddr = args[1].parse().expect("invalid bind address");
    
    // Parse peer/group (skip flags)
    let peer_arg = args.iter()
        .skip(2)
        .find(|a| !a.starts_with('-'))
        .map(|s| s.as_str());
    
    let peer: SocketAddr = if echo {
        bind
    } else {
        peer_arg.expect("missing peer/group address").parse().expect("invalid peer address")
    };

    // Get IPC paths (skip flags)
    let paths: Vec<&str> = args
        .iter()
        .skip(2)
        .filter(|a| !a.starts_with('-') && a.parse::<SocketAddr>().is_err())
        .map(|s| s.as_str())
        .collect();
    let (send_path, recv_path) = (
        paths.first().copied().unwrap_or("/tmp/kaos-send"),
        paths.get(1).copied().unwrap_or("/tmp/kaos-recv"),
    );

    let mode = if multicast { "[MULTICAST]" } 
               else if cfg!(feature = "reliable") { "[RUDP]" } 
               else { "" };
    println!("kaos-driver{} {} → {}", mode, bind, peer);
    println!("IPC: {} / {}", send_path, recv_path);

    let mut from_app = wait_for_ipc(send_path);
    let mut to_app = Publisher::create(recv_path, RING_SIZE).expect("create Publisher failed");
    let running = Arc::new(AtomicBool::new(true));
    let r = running.clone();
    ctrlc::set_handler(move || r.store(false, Ordering::SeqCst)).ok();

    if multicast {
        run_multicast(bind, peer, &mut from_app, &mut to_app, &running);
        return;
    }

    #[cfg(feature = "reliable")]
    return run_reliable(bind, peer, &mut from_app, &mut to_app, &running);

    #[cfg(not(feature = "reliable"))]
    {
        let socket = UdpSocket::bind(bind).expect("bind failed");
        socket.set_nonblocking(true).unwrap();
        if !echo {
            socket.connect(peer).unwrap();
        }

        #[cfg(all(target_os = "linux", feature = "uring"))]
        return run_uring(&socket, &mut from_app, &mut to_app, &running);
        #[cfg(all(target_os = "linux", not(feature = "uring")))]
        return run_linux(&socket, &mut from_app, &mut to_app, &running);
        #[cfg(not(target_os = "linux"))]
        run_portable(&socket, &mut from_app, &mut to_app, &running, echo);
    }
}

fn wait_for_ipc(path: &str) -> Subscriber {
    println!("Waiting for app to create {}...", path);
    loop {
        if let Ok(s) = Subscriber::open(path) {
            println!("Connected to {}", path);
            return s;
        }
        thread::sleep(Duration::from_millis(100));
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// MULTICAST - batched sends to multicast group
// ═══════════════════════════════════════════════════════════════════════════

fn create_multicast_socket(bind_port: u16, group: Ipv4Addr) -> std::io::Result<UdpSocket> {
    use std::net::SocketAddrV4;
    
    let socket2 = socket2::Socket::new(
        socket2::Domain::IPV4,
        socket2::Type::DGRAM,
        Some(socket2::Protocol::UDP),
    )?;
    socket2.set_reuse_address(true)?;
    socket2.set_send_buffer_size(8 * 1024 * 1024)?;
    socket2.set_recv_buffer_size(8 * 1024 * 1024)?;
    
    let addr = SocketAddrV4::new(Ipv4Addr::UNSPECIFIED, bind_port);
    socket2.bind(&addr.into())?;
    
    let socket: UdpSocket = socket2.into();
    socket.join_multicast_v4(&group, &Ipv4Addr::UNSPECIFIED)?;
    socket.set_multicast_loop_v4(true)?;
    socket.set_multicast_ttl_v4(1)?;
    socket.set_nonblocking(true)?;
    
    Ok(socket)
}

fn run_multicast(
    bind: SocketAddr,
    group_addr: SocketAddr,
    from_app: &mut Subscriber,
    to_app: &mut Publisher,
    running: &Arc<AtomicBool>,
) {
    let group = match group_addr.ip() {
        std::net::IpAddr::V4(ip) => ip,
        _ => panic!("multicast requires IPv4"),
    };
    
    println!("Joining multicast group {}", group);
    let socket = create_multicast_socket(bind.port(), group).expect("multicast socket failed");
    
    #[cfg(target_os = "linux")]
    run_multicast_linux(&socket, group_addr, from_app, to_app, running);
    
    #[cfg(not(target_os = "linux"))]
    run_multicast_portable(&socket, group_addr, from_app, to_app, running);
}

#[cfg(target_os = "linux")]
fn run_multicast_linux(
    socket: &UdpSocket,
    dest: SocketAddr,
    from_app: &mut Subscriber,
    to_app: &mut Publisher,
    running: &Arc<AtomicBool>,
) {
    use std::mem::MaybeUninit;
    let fd = socket.as_raw_fd();
    let dest_addr = socket2::SockAddr::from(dest);
    
    let (mut sent, mut recvd, mut last) = (0u64, 0u64, Instant::now());
    
    // Pre-allocate buffers
    let mut send_bufs = [[0u8; MSG_SIZE]; BATCH_SIZE];
    let mut send_iovecs: [libc::iovec; BATCH_SIZE] = unsafe { MaybeUninit::zeroed().assume_init() };
    let mut send_msgs: [libc::mmsghdr; BATCH_SIZE] = unsafe { MaybeUninit::zeroed().assume_init() };
    let mut recv_bufs = [[0u8; MSG_SIZE]; BATCH_SIZE];
    let mut recv_iovecs: [libc::iovec; BATCH_SIZE] = unsafe { MaybeUninit::zeroed().assume_init() };
    let mut recv_msgs: [libc::mmsghdr; BATCH_SIZE] = unsafe { MaybeUninit::zeroed().assume_init() };
    
    // Setup send msgs with multicast destination
    for i in 0..BATCH_SIZE {
        send_iovecs[i].iov_base = send_bufs[i].as_mut_ptr() as *mut _;
        send_iovecs[i].iov_len = MSG_SIZE;
        send_msgs[i].msg_hdr.msg_iov = &mut send_iovecs[i];
        send_msgs[i].msg_hdr.msg_iovlen = 1;
        send_msgs[i].msg_hdr.msg_name = dest_addr.as_ptr() as *mut _;
        send_msgs[i].msg_hdr.msg_namelen = dest_addr.len();
    }
    
    // Setup recv msgs
    for i in 0..BATCH_SIZE {
        recv_iovecs[i].iov_base = recv_bufs[i].as_mut_ptr() as *mut _;
        recv_iovecs[i].iov_len = MSG_SIZE;
        recv_msgs[i].msg_hdr.msg_iov = &mut recv_iovecs[i];
        recv_msgs[i].msg_hdr.msg_iovlen = 1;
    }
    
    println!("Running multicast driver (sendmmsg/recvmmsg, batch={})", BATCH_SIZE);
    
    while running.load(Ordering::Relaxed) {
        // Batch messages from app
        let mut n_send = 0;
        while n_send < BATCH_SIZE {
            if let Some(v) = from_app.try_receive() {
                send_bufs[n_send][..8].copy_from_slice(&v.to_le_bytes());
                n_send += 1;
            } else {
                break;
            }
        }
        
        // Send batch to multicast group
        if n_send > 0 {
            let n = unsafe { libc::sendmmsg(fd, send_msgs.as_mut_ptr(), n_send as u32, 0) };
            if n > 0 {
                sent += n as u64;
            }
        }
        
        // Receive batch from multicast
        let n = unsafe {
            libc::recvmmsg(
                fd,
                recv_msgs.as_mut_ptr(),
                BATCH_SIZE as u32,
                libc::MSG_DONTWAIT,
                std::ptr::null_mut(),
            )
        };
        if n > 0 {
            for i in 0..n as usize {
                if recv_msgs[i].msg_len >= 8 {
                    let v = u64::from_le_bytes(recv_bufs[i][..8].try_into().unwrap());
                    let _ = to_app.send(v);
                    recvd += 1;
                }
            }
        }
        
        if last.elapsed() > Duration::from_secs(5) {
            println!("  tx={} rx={}", sent, recvd);
            last = Instant::now();
        }
        
        if n_send == 0 && n <= 0 {
            thread::yield_now();
        }
    }
    println!("done tx={} rx={}", sent, recvd);
}

#[cfg(not(target_os = "linux"))]
fn run_multicast_portable(
    socket: &UdpSocket,
    dest: SocketAddr,
    from_app: &mut Subscriber,
    to_app: &mut Publisher,
    running: &Arc<AtomicBool>,
) {
    let (mut sent, mut recvd, mut last) = (0u64, 0u64, Instant::now());
    let mut buf = [0u8; MSG_SIZE];
    let mut send_buf = [0u8; MSG_SIZE];
    
    // Batch buffer for coalescing sends
    let mut batch: Vec<u64> = Vec::with_capacity(BATCH_SIZE);
    
    println!("Running multicast driver (batched sends, batch={})", BATCH_SIZE);
    
    while running.load(Ordering::Relaxed) {
        // Collect batch from app
        batch.clear();
        while batch.len() < BATCH_SIZE {
            if let Some(v) = from_app.try_receive() {
                batch.push(v);
            } else {
                break;
            }
        }
        
        // Send batch (amortizes syscall overhead by batching app reads)
        for &v in &batch {
            send_buf[..8].copy_from_slice(&v.to_le_bytes());
            if socket.send_to(&send_buf, dest).is_ok() {
                sent += 1;
            }
        }
        
        // Receive (non-blocking)
        loop {
            match socket.recv_from(&mut buf) {
                Ok((len, _)) if len >= 8 => {
                    let v = u64::from_le_bytes(buf[..8].try_into().unwrap());
                    let _ = to_app.send(v);
                    recvd += 1;
                }
                _ => break,
            }
        }
        
        if last.elapsed() > Duration::from_secs(5) {
            println!("  tx={} rx={}", sent, recvd);
            last = Instant::now();
        }
        
        if batch.is_empty() {
            thread::yield_now();
        }
    }
    println!("done tx={} rx={}", sent, recvd);
}

// ═══════════════════════════════════════════════════════════════════════════
// RELIABLE UDP
// ═══════════════════════════════════════════════════════════════════════════

#[cfg(feature = "reliable")]
fn run_reliable(
    bind: SocketAddr,
    peer: SocketAddr,
    from_app: &mut Subscriber,
    to_app: &mut Publisher,
    running: &Arc<AtomicBool>,
) {
    use kaos_rudp::ReliableUdpRingBufferTransport;
    let mut transport = ReliableUdpRingBufferTransport::new(bind, peer, 65536).unwrap();
    let (mut sent, mut recvd, mut last) = (0u64, 0u64, Instant::now());

    while running.load(Ordering::Relaxed) {
        let mut batch_data: Vec<[u8; 8]> = Vec::with_capacity(64);
        while batch_data.len() < 64 {
            if let Some(v) = from_app.try_receive() {
                batch_data.push(v.to_le_bytes());
            } else {
                break;
            }
        }
        if !batch_data.is_empty() {
            let batch: Vec<&[u8]> = batch_data.iter().map(|d| d.as_slice()).collect();
            if let Ok(n) = transport.send_batch(&batch) {
                sent += n as u64;
            }
        }
        transport.receive_batch_with(64, |msg| {
            if msg.len() >= 8 {
                let _ = to_app.send(u64::from_le_bytes(msg[..8].try_into().unwrap_or_default()));
                recvd += 1;
            }
        });
        transport.process_acks();
        if last.elapsed() > Duration::from_secs(5) {
            println!("  tx={} rx={}", sent, recvd);
            last = Instant::now();
        }
        thread::yield_now();
    }
    println!("done tx={} rx={}", sent, recvd);
}

// ═══════════════════════════════════════════════════════════════════════════
// UNICAST - io_uring (Linux)
// ═══════════════════════════════════════════════════════════════════════════

#[cfg(all(target_os = "linux", feature = "uring"))]
fn run_uring(
    socket: &UdpSocket,
    from_app: &mut Subscriber,
    to_app: &mut Publisher,
    running: &Arc<AtomicBool>,
) {
    use crate::uring::UringDriver;
    let mut driver = match UringDriver::new(socket) {
        Ok(d) => d,
        Err(e) => {
            eprintln!("uring failed: {}", e);
            return run_linux(socket, from_app, to_app, running);
        }
    };
    let (mut sent, mut recvd, mut last) = (0u64, 0u64, Instant::now());

    while running.load(Ordering::Relaxed) {
        let mut batch = Vec::with_capacity(64);
        while batch.len() < 64 {
            if let Some(v) = from_app.try_receive() {
                batch.push(v.to_le_bytes());
            } else {
                break;
            }
        }
        if !batch.is_empty() {
            if let Ok(n) = driver.submit_sends(&batch) {
                sent += n as u64;
            }
        }
        let _ = driver.queue_recvs();
        driver.poll_completions(|v| {
            let _ = to_app.send(v);
            recvd += 1;
        });
        if last.elapsed() > Duration::from_secs(5) {
            println!("  tx={} rx={}", sent, recvd);
            last = Instant::now();
        }
        thread::yield_now();
    }
    println!("done tx={} rx={}", sent, recvd);
}

// ═══════════════════════════════════════════════════════════════════════════
// UNICAST - sendmmsg/recvmmsg (Linux)
// ═══════════════════════════════════════════════════════════════════════════

#[cfg(target_os = "linux")]
fn run_linux(
    socket: &UdpSocket,
    from_app: &mut Subscriber,
    to_app: &mut Publisher,
    running: &Arc<AtomicBool>,
) {
    use std::mem::MaybeUninit;
    let fd = socket.as_raw_fd();
    let (mut sent, mut recvd, mut last) = (0u64, 0u64, Instant::now());
    let mut send_bufs = [[0u8; 8]; BATCH_SIZE];
    let mut send_iovecs: [libc::iovec; BATCH_SIZE] = unsafe { MaybeUninit::zeroed().assume_init() };
    let mut send_msgs: [libc::mmsghdr; BATCH_SIZE] = unsafe { MaybeUninit::zeroed().assume_init() };
    let mut recv_bufs = [[0u8; 8]; BATCH_SIZE];
    let mut recv_iovecs: [libc::iovec; BATCH_SIZE] = unsafe { MaybeUninit::zeroed().assume_init() };
    let mut recv_msgs: [libc::mmsghdr; BATCH_SIZE] = unsafe { MaybeUninit::zeroed().assume_init() };

    for i in 0..BATCH_SIZE {
        recv_iovecs[i].iov_base = recv_bufs[i].as_mut_ptr() as *mut _;
        recv_iovecs[i].iov_len = 8;
        recv_msgs[i].msg_hdr.msg_iov = &mut recv_iovecs[i];
        recv_msgs[i].msg_hdr.msg_iovlen = 1;
    }

    while running.load(Ordering::Relaxed) {
        let mut n_send = 0;
        while n_send < BATCH_SIZE {
            if let Some(v) = from_app.try_receive() {
                send_bufs[n_send] = v.to_le_bytes();
                send_iovecs[n_send].iov_base = send_bufs[n_send].as_mut_ptr() as *mut _;
                send_iovecs[n_send].iov_len = 8;
                send_msgs[n_send].msg_hdr.msg_iov = &mut send_iovecs[n_send];
                send_msgs[n_send].msg_hdr.msg_iovlen = 1;
                n_send += 1;
            } else {
                break;
            }
        }
        if n_send > 0 {
            let n = unsafe { libc::sendmmsg(fd, send_msgs.as_mut_ptr(), n_send as u32, 0) };
            if n > 0 {
                sent += n as u64;
            }
        }
        let n = unsafe {
            libc::recvmmsg(
                fd,
                recv_msgs.as_mut_ptr(),
                BATCH_SIZE as u32,
                libc::MSG_DONTWAIT,
                std::ptr::null_mut(),
            )
        };
        if n > 0 {
            for i in 0..n as usize {
                if recv_msgs[i].msg_len >= 8 {
                    let _ = to_app.send(u64::from_le_bytes(recv_bufs[i]));
                    recvd += 1;
                }
            }
        }
        if last.elapsed() > Duration::from_secs(5) {
            println!("  tx={} rx={}", sent, recvd);
            last = Instant::now();
        }
        thread::yield_now();
    }
    println!("done tx={} rx={}", sent, recvd);
}

// ═══════════════════════════════════════════════════════════════════════════
// UNICAST - Portable (macOS, etc)
// ═══════════════════════════════════════════════════════════════════════════

#[cfg(not(target_os = "linux"))]
fn run_portable(
    socket: &UdpSocket,
    from_app: &mut Subscriber,
    to_app: &mut Publisher,
    running: &Arc<AtomicBool>,
    echo: bool,
) {
    let mut buf = [0u8; 8];
    let (mut sent, mut recvd, mut last) = (0u64, 0u64, Instant::now());

    while running.load(Ordering::Relaxed) {
        while let Some(v) = from_app.try_receive() {
            if socket.send(&v.to_le_bytes()).is_ok() {
                sent += 1;
            }
        }
        if let Ok((len, src)) = socket.recv_from(&mut buf) {
            if len >= 8 {
                if echo {
                    let _ = socket.send_to(&buf[..len], src);
                }
                let _ = to_app.send(u64::from_le_bytes(buf));
                recvd += 1;
            }
        }
        if last.elapsed() > Duration::from_secs(5) {
            println!("  tx={} rx={}", sent, recvd);
            last = Instant::now();
        }
        thread::yield_now();
    }
    println!("done tx={} rx={}", sent, recvd);
}

//! Kaos Media Driver - Zero-syscall messaging via shared memory.
//!
//! Usage: kaos-driver <bind> <peer> [send_path] [recv_path] [--echo]
//! Features: --features reliable (kaos-rudp), --features uring (io_uring)

use std::net::{ SocketAddr, UdpSocket };
#[cfg(target_os = "linux")]
use std::os::fd::AsRawFd;
use std::sync::atomic::{ AtomicBool, Ordering };
use std::sync::Arc;
use std::time::{ Duration, Instant };
use std::thread;
use kaos_ipc::{ Publisher, Subscriber };

#[cfg(all(target_os = "linux", feature = "uring"))]
mod uring;

const RING_SIZE: usize = 64 * 1024;
#[cfg(target_os = "linux")]
const BATCH_SIZE: usize = 64;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let echo = args.iter().any(|a| (a == "--echo" || a == "-e"));

    if args.len() < 2 || (args.len() < 3 && !echo) {
        eprintln!("Usage: kaos-driver <bind> <peer> [--echo] [send_path] [recv_path]");
        std::process::exit(1);
    }

    let bind: SocketAddr = args[1].parse().expect("invalid bind");
    let peer: SocketAddr = if echo { bind } else { args[2].parse().expect("invalid peer") };
    let paths: Vec<&str> = args
        .iter()
        .skip(if echo { 2 } else { 3 })
        .filter(|a| !a.starts_with('-'))
        .map(|s| s.as_str())
        .collect();
    let (send_path, recv_path) = (
        paths.first().copied().unwrap_or("/tmp/kaos-send"),
        paths.get(1).copied().unwrap_or("/tmp/kaos-recv"),
    );

    println!(
        "kaos-driver{} {} → {}",
        if cfg!(feature = "reliable") {
            " [RUDP]"
        } else {
            ""
        },
        bind,
        peer
    );

    let mut from_app = wait_for_ipc(send_path);
    let mut to_app = Publisher::create(recv_path, RING_SIZE).expect("create failed");
    let running = Arc::new(AtomicBool::new(true));
    let r = running.clone();
    ctrlc::set_handler(move || r.store(false, Ordering::SeqCst)).ok();

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
    loop {
        if let Ok(s) = Subscriber::open(path) {
            return s;
        }
        thread::sleep(Duration::from_millis(100));
    }
}

#[cfg(feature = "reliable")]
fn run_reliable(
    bind: SocketAddr,
    peer: SocketAddr,
    from_app: &mut Subscriber,
    to_app: &mut Publisher,
    running: &Arc<AtomicBool>
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
            let batch: Vec<&[u8]> = batch_data
                .iter()
                .map(|d| d.as_slice())
                .collect();
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

#[cfg(all(target_os = "linux", feature = "uring"))]
fn run_uring(
    socket: &UdpSocket,
    from_app: &mut Subscriber,
    to_app: &mut Publisher,
    running: &Arc<AtomicBool>
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

#[cfg(target_os = "linux")]
fn run_linux(
    socket: &UdpSocket,
    from_app: &mut Subscriber,
    to_app: &mut Publisher,
    running: &Arc<AtomicBool>
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
                std::ptr::null_mut()
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

#[cfg(not(target_os = "linux"))]
fn run_portable(
    socket: &UdpSocket,
    from_app: &mut Subscriber,
    to_app: &mut Publisher,
    running: &Arc<AtomicBool>,
    echo: bool
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

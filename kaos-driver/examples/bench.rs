//! kaos-driver benchmarks
//!
//! cargo run -p kaos-driver --release --example bench -- [ipc|syscall|recv|send]

use kaos_ipc::{ Publisher, Subscriber };
use std::time::{ Instant, Duration };
use std::net::UdpSocket;
use std::thread;
use std::fs;

const N: u64 = 1_000_000;

fn main() {
    let mode = std::env
        ::args()
        .nth(1)
        .unwrap_or_else(|| "ipc".into());
    match mode.as_str() {
        "ipc" => bench_ipc(),
        "syscall" => bench_syscall(),
        "recv" => two_process_recv(),
        "send" => two_process_send(),
        _ => println!("Usage: bench [ipc|syscall|recv|send]"),
    }
}

fn bench_ipc() {
    println!("=== IPC Benchmark (same process) ===");
    let path = "/tmp/kaos-bench";
    let _ = fs::remove_file(path);

    let mut pub_ = Publisher::create(path, 64 * 1024).unwrap();
    let mut sub = Subscriber::open(path).unwrap();

    let start = Instant::now();
    let (mut sent, mut recv) = (0u64, 0u64);

    // Interleave send/receive to avoid buffer full
    while sent < N || recv < N {
        // Send batch
        for _ in 0..1000 {
            if sent < N && pub_.send(sent).is_ok() {
                sent += 1;
            } else {
                break;
            }
        }
        // Receive batch
        for _ in 0..1000 {
            if recv < N && sub.try_receive().is_some() {
                recv += 1;
            } else {
                break;
            }
        }
    }

    let elapsed = start.elapsed();
    println!(
        "Sent {} in {:?} ({:.1} M/s, {:.1} ns/op)",
        N,
        elapsed,
        (N as f64) / elapsed.as_secs_f64() / 1e6,
        (elapsed.as_nanos() as f64) / (N as f64)
    );
    println!("Verified {} received", recv);
    let _ = fs::remove_file(path);
}

fn bench_syscall() {
    println!("=== Syscall Benchmark (baseline) ===");
    let socket = UdpSocket::bind("127.0.0.1:0").unwrap();
    socket.connect("127.0.0.1:9999").unwrap();
    socket.set_nonblocking(true).unwrap();

    let start = Instant::now();
    for i in 0..N {
        let _ = socket.send(&i.to_le_bytes());
    }
    let elapsed = start.elapsed();

    println!(
        "Sent {} in {:?} ({:.2} M/s, {:.0} ns/op)",
        N,
        elapsed,
        (N as f64) / elapsed.as_secs_f64() / 1e6,
        (elapsed.as_nanos() as f64) / (N as f64)
    );
}

fn two_process_recv() {
    println!("=== Two-Process Receiver ===");
    let path = "/tmp/kaos-ipc-bench";
    while !std::path::Path::new(path).exists() {
        thread::sleep(Duration::from_millis(50));
    }
    thread::sleep(Duration::from_millis(100));

    let mut sub = Subscriber::open(path).unwrap();
    let (mut recv, mut start): (u64, Option<Instant>) = (0, None);

    loop {
        while let Some(v) = sub.try_receive() {
            if start.is_none() {
                start = Some(Instant::now());
            }
            if (v >> 56) == 0xff {
                // end marker
                let elapsed = start.unwrap().elapsed();
                println!(
                    "Received {} in {:?} ({:.1} M/s)",
                    recv,
                    elapsed,
                    (recv as f64) / elapsed.as_secs_f64() / 1e6
                );
                let _ = fs::remove_file(path);
                return;
            }
            recv += 1;
        }
        thread::yield_now();
    }
}

fn two_process_send() {
    println!("=== Two-Process Sender ===");
    let path = "/tmp/kaos-ipc-bench";
    let _ = fs::remove_file(path);

    let mut pub_ = Publisher::create(path, 256 * 1024).unwrap();
    thread::sleep(Duration::from_millis(500));

    let start = Instant::now();
    for i in 0..N {
        while pub_.send(i).is_err() {
            thread::yield_now();
        }
    }
    while pub_.send(0xff << 56).is_err() {
        thread::yield_now();
    }
    let elapsed = start.elapsed();

    println!("Sent {} in {:?} ({:.1} M/s)", N, elapsed, (N as f64) / elapsed.as_secs_f64() / 1e6);
}

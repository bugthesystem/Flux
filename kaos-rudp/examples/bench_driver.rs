//! Benchmark: Direct syscalls vs Media Driver (full network round-trip)
//!
//! This tests REAL network I/O through the media driver:
//! App → IPC → Driver A → UDP → Driver B → IPC → App
//!
//! Run:
//!   Terminal 1: cargo run -p kaos-driver --release -- 127.0.0.1:9000 127.0.0.1:9001
//!   Terminal 2: cargo run -p kaos-driver --release -- 127.0.0.1:9001 127.0.0.1:9000
//!   Terminal 3: cargo run -p kaos-rudp --release --features driver --example bench_driver

#[cfg(feature = "driver")]
fn main() {
    use kaos_ipc::{Publisher, Subscriber};
    use std::fs;
    use std::net::UdpSocket;
    use std::thread;
    use std::time::{Duration, Instant};

    println!("=== Media Driver Network Benchmark ===\n");

    // Benchmark 1: Direct UDP syscalls (baseline)
    println!("1. Direct UDP syscalls (baseline):");
    {
        let socket = UdpSocket::bind("127.0.0.1:0").unwrap();
        socket.connect("127.0.0.1:9999").unwrap();
        socket.set_nonblocking(true).unwrap();

        const N: u64 = 1_000_000;
        let start = Instant::now();
        for i in 0..N {
            let _ = socket.send(&i.to_le_bytes());
        }
        let elapsed = start.elapsed();

        println!("   Messages: {}", N);
        println!(
            "   Throughput: {:.2} M msg/s",
            N as f64 / elapsed.as_secs_f64() / 1_000_000.0
        );
        println!(
            "   Latency: {:.0} ns/msg (syscall overhead)\n",
            elapsed.as_nanos() as f64 / N as f64
        );
    }

    // Benchmark 2: IPC only (what app thread sees)
    println!("2. Shared memory IPC only (app-side latency):");
    {
        let path = "/tmp/kaos-bench-ipc";
        let _ = fs::remove_file(path);

        let mut pub_ = Publisher::create(path, 64 * 1024).unwrap();
        let mut sub = Subscriber::open(path).unwrap();

        const N: u64 = 10_000_000;
        let start = Instant::now();
        let mut sent = 0u64;
        let mut received = 0u64;

        while received < N {
            while sent < N && pub_.send(sent).is_ok() {
                sent += 1;
            }
            while let Some(_) = sub.try_receive() {
                received += 1;
            }
        }
        let elapsed = start.elapsed();

        println!("   Messages: {}", N);
        println!(
            "   Throughput: {:.2} M msg/s",
            N as f64 / elapsed.as_secs_f64() / 1_000_000.0
        );
        println!(
            "   Latency: {:.1} ns/msg (zero syscalls!)\n",
            elapsed.as_nanos() as f64 / N as f64
        );
        let _ = fs::remove_file(path);
    }

    // Benchmark 3: Full network round-trip via media driver
    println!("3. Full network via media driver:");
    println!("   Checking if drivers are running...");

    let send_path = "/tmp/kaos-send";
    let recv_path = "/tmp/kaos-recv";

    // Clean up old files
    let _ = fs::remove_file(send_path);
    let _ = fs::remove_file(recv_path);

    // Create send ring (driver will detect this)
    let mut to_driver = match Publisher::create(send_path, 64 * 1024) {
        Ok(p) => p,
        Err(e) => {
            println!("   Failed to create send ring: {}", e);
            return;
        }
    };
    println!("   Created: {}", send_path);

    // Wait for driver to create recv ring
    println!("   Waiting for driver to create recv ring...");
    let mut from_driver = None;
    for _ in 0..50 {
        if let Ok(s) = Subscriber::open(recv_path) {
            from_driver = Some(s);
            break;
        }
        thread::sleep(Duration::from_millis(100));
    }

    let mut from_driver = match from_driver {
        Some(s) => {
            println!("   Connected: {}", recv_path);
            s
        }
        None => {
            println!("\n   ERROR: Driver not running!");
            println!("   Start two drivers first:");
            println!("     Terminal 1: cargo run -p kaos-driver --release -- 127.0.0.1:9000 127.0.0.1:9001");
            println!("     Terminal 2: cargo run -p kaos-driver --release -- 127.0.0.1:9001 127.0.0.1:9000");
            return;
        }
    };

    // Send messages and measure
    const N: u64 = 100_000;
    println!("   Sending {} messages through network...", N);

    let start = Instant::now();
    let mut sent = 0u64;
    let mut received = 0u64;
    let timeout = Duration::from_secs(10);

    while received < N && start.elapsed() < timeout {
        // Send
        while sent < N && to_driver.send(sent).is_ok() {
            sent += 1;
        }
        // Receive
        while let Some(_) = from_driver.try_receive() {
            received += 1;
        }

        if sent >= N && received == 0 {
            thread::sleep(Duration::from_millis(10));
        }
    }

    let elapsed = start.elapsed();

    if received > 0 {
        println!("   Sent: {}, Received: {}", sent, received);
        println!("   Time: {:?}", elapsed);
        println!(
            "   Throughput: {:.2} M msg/s",
            received as f64 / elapsed.as_secs_f64() / 1_000_000.0
        );
        println!(
            "   Round-trip: {:.0} ns/msg\n",
            elapsed.as_nanos() as f64 / received as f64
        );
    } else {
        println!("   Sent: {}, but received 0", sent);
        println!("   (Peer driver may not be running or connected)\n");
    }

    println!("Summary:");
    println!("  - Direct syscalls: ~0.4 M msg/s, ~2500 ns/msg");
    println!("  - IPC (app sees):  ~190 M msg/s, ~5 ns/msg");
    println!("  - Network:         Depends on peer driver");
}

#[cfg(not(feature = "driver"))]
fn main() {
    println!("Run with --features driver:");
    println!("  cargo run -p kaos-rudp --release --features driver --example bench_driver");
}

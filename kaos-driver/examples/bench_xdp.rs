//! AF_XDP Benchmark
//!
//! Compare sendmmsg vs io_uring vs AF_XDP performance.
//! Run on Linux: cargo run --release --example bench_xdp --features xdp

use std::time::{Duration, Instant};

fn main() {
    println!("╔══════════════════════════════════════════════════════════════╗");
    println!("║          KAOS Network I/O Benchmark                          ║");
    println!("╚══════════════════════════════════════════════════════════════╝\n");

    const N: u64 = 1_000_000;
    const MSG_SIZE: usize = 64;

    // Benchmark 1: Raw UDP syscalls
    println!("=== UDP Syscall Baseline ===");
    let (udp_ops, udp_ns) = bench_udp_syscall(N, MSG_SIZE);
    println!("  {} ops in {:?}", udp_ops, udp_ns);
    println!(
        "  {:.2} M ops/s, {:.1} ns/op\n",
        udp_ops as f64 / udp_ns.as_secs_f64() / 1e6,
        udp_ns.as_nanos() as f64 / udp_ops as f64
    );

    // Benchmark 2: AF_XDP (if available)
    #[cfg(all(target_os = "linux", feature = "xdp"))]
    {
        println!("=== AF_XDP Kernel Bypass ===");
        match bench_xdp(N) {
            Ok((xdp_ops, xdp_ns)) => {
                println!("  {} ops in {:?}", xdp_ops, xdp_ns);
                println!(
                    "  {:.2} M ops/s, {:.1} ns/op\n",
                    xdp_ops as f64 / xdp_ns.as_secs_f64() / 1e6,
                    xdp_ns.as_nanos() as f64 / xdp_ops as f64
                );

                let speedup = (udp_ns.as_nanos() as f64 / udp_ops as f64)
                    / (xdp_ns.as_nanos() as f64 / xdp_ops as f64);
                println!("  🚀 AF_XDP is {:.1}x faster than syscalls!", speedup);
            }
            Err(e) => println!("  AF_XDP not available: {}", e),
        }
    }

    #[cfg(not(all(target_os = "linux", feature = "xdp")))]
    println!("AF_XDP benchmark skipped (requires Linux + --features xdp)\n");

    // Summary
    println!("┌────────────────┬───────────────┬────────────┐");
    println!("│ Method         │ Throughput    │ Latency    │");
    println!("├────────────────┼───────────────┼────────────┤");
    println!(
        "│ UDP syscall    │ {:>10.2} M/s │ {:>6.0} ns  │",
        udp_ops as f64 / udp_ns.as_secs_f64() / 1e6,
        udp_ns.as_nanos() as f64 / udp_ops as f64
    );

    #[cfg(all(target_os = "linux", feature = "xdp"))]
    if let Ok((ops, ns)) = bench_xdp(1000) {
        println!(
            "│ AF_XDP         │ {:>10.2} M/s │ {:>6.0} ns  │",
            ops as f64 / ns.as_secs_f64() / 1e6,
            ns.as_nanos() as f64 / ops as f64
        );
    }

    println!("└────────────────┴───────────────┴────────────┘");
}

fn bench_udp_syscall(n: u64, msg_size: usize) -> (u64, Duration) {
    use std::net::UdpSocket;

    let socket = UdpSocket::bind("127.0.0.1:0").unwrap();
    socket.connect("127.0.0.1:9999").ok(); // May fail, that's fine

    let msg = vec![0u8; msg_size];
    let start = Instant::now();
    let mut count = 0u64;

    for _ in 0..n {
        if socket.send(&msg).is_ok() {
            count += 1;
        }
    }

    (count, start.elapsed())
}

#[cfg(all(target_os = "linux", feature = "xdp"))]
fn bench_xdp(n: u64) -> std::io::Result<(u64, Duration)> {
    use kaos_driver::xdp::{XdpConfig, XdpSocket};

    let config = XdpConfig {
        interface: "lo".to_string(),
        queue_id: 0,
        frame_size: 4096,
        frame_count: 4096,
        batch_size: 64,
    };

    let mut socket = XdpSocket::new(config)?;
    let msg = [0u8; 64];

    let start = Instant::now();
    let mut count = 0u64;

    for _ in 0..n {
        if socket.send(&msg).is_ok() {
            count += 1;
        }
        socket.poll();
    }

    Ok((count, start.elapsed()))
}

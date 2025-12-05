//! Multi-size IPC benchmark: 8B, 16B, 32B, 64B
//!
//! cargo run -p kaos-driver --release --example bench_sizes

use kaos::disruptor::{RingBufferEntry, SharedRingBuffer, Slot16, Slot32, Slot64, Slot8};
use std::fs;
use std::time::Instant;

const N: u64 = 10_000_000;
const RING_SIZE: usize = 64 * 1024;

fn main() {
    println!("\n╔═══════════════════════════════════════════╗");
    println!("║  Kaos IPC Multi-Size Benchmark             ║");
    println!("╚═══════════════════════════════════════════╝\n");

    bench_size::<Slot8>("8B", 8);
    bench_size::<Slot16>("16B", 16);
    bench_size::<Slot32>("32B", 32);
    bench_size::<Slot64>("64B", 64);

    println!("═══════════════════════════════════════════");
}

fn bench_size<T: RingBufferEntry + Default + Clone + Copy>(label: &str, size: usize) {
    let path = format!("/tmp/kaos-bench-{}", label);
    let _ = fs::remove_file(&path);

    println!("═══════════════════════════════════════════");
    println!("  IPC {} ({} bytes)", label, size);
    println!("═══════════════════════════════════════════\n");

    let mut ring: SharedRingBuffer<T> = SharedRingBuffer::create(&path, RING_SIZE).unwrap();
    let mut ring2: SharedRingBuffer<T> = SharedRingBuffer::open(&path).unwrap();

    // Create data buffer of the right size
    let data = vec![0xABu8; size];

    let start = Instant::now();
    let (mut sent, mut recv) = (0u64, 0u64);

    // Interleave send/receive (same process, single thread)
    while sent < N || recv < N {
        // Send batch
        for _ in 0..1000 {
            if sent < N {
                if ring.try_send(&data).is_ok() {
                    sent += 1;
                } else {
                    break;
                }
            }
        }
        // Receive batch
        for _ in 0..1000 {
            if recv < N {
                if ring2.try_receive().is_some() {
                    recv += 1;
                } else {
                    break;
                }
            }
        }
    }

    let elapsed = start.elapsed();
    let throughput = (N as f64) / elapsed.as_secs_f64() / 1e6;
    let ns_per_op = (elapsed.as_nanos() as f64) / (N as f64);
    let bandwidth_mb = throughput * (size as f64);

    println!("  Messages:   {:>12}", format_num(N));
    println!("  Throughput: {:>12.1} M/s", throughput);
    println!("  Latency:    {:>12.1} ns/op", ns_per_op);
    println!("  Bandwidth:  {:>12.1} MB/s", bandwidth_mb);
    println!();

    let _ = fs::remove_file(&path);
}

fn format_num(n: u64) -> String {
    if n >= 1_000_000_000 {
        format!("{:.2}B", n as f64 / 1e9)
    } else if n >= 1_000_000 {
        format!("{:.2}M", n as f64 / 1e6)
    } else if n >= 1_000 {
        format!("{:.2}K", n as f64 / 1e3)
    } else {
        format!("{}", n)
    }
}

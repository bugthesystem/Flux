// Multi-threaded Ring Buffer Benchmark
// SPSC and MPSC scenarios for RingBuffer

use std::sync::{ Arc, Mutex };
use std::thread;
use std::time::Instant;
use flux::disruptor::{ RingBuffer, RingBufferConfig, RingBufferEntry };

const MESSAGES: usize = 1_000_000;
const BUFFER_SIZE: usize = 65536;
const PRODUCERS: usize = 4;

fn spsc_benchmark() {
    println!("\n🔹 SPSC (Single Producer, Single Consumer)");
    let config = RingBufferConfig::new(BUFFER_SIZE).unwrap();
    let ring = Arc::new(Mutex::new(RingBuffer::new(config).unwrap()));
    let start = Instant::now();
    let producer = {
        let ring = ring.clone();
        thread::spawn(move || {
            for _ in 0..MESSAGES {
                loop {
                    let mut ring = ring.lock().unwrap();
                    if let Some((seq, slots)) = ring.try_claim_slots(1) {
                        slots[0].set_sequence(seq);
                        slots[0].set_data(&[0xaa; 64]);
                        ring.publish_batch(seq, 1);
                        break;
                    }
                }
            }
        })
    };
    let consumer = {
        let ring = ring.clone();
        thread::spawn(move || {
            let mut received = 0;
            while received < MESSAGES {
                let mut ring = ring.lock().unwrap();
                let msgs = ring.try_consume_batch(0, 64);
                if !msgs.is_empty() {
                    received += msgs.len();
                }
            }
        })
    };
    producer.join().unwrap();
    consumer.join().unwrap();
    let duration = start.elapsed();
    let throughput = (MESSAGES as f64) / duration.as_secs_f64();
    println!("  Messages: {}", MESSAGES);
    println!("  Duration: {:.2}s", duration.as_secs_f64());
    println!("  Throughput: {:.2} M msgs/sec", throughput / 1_000_000.0);
}

fn mpsc_benchmark() {
    println!("\n🔹 MPSC (Multi-Producer, Single Consumer)");
    let config = RingBufferConfig::new(BUFFER_SIZE).unwrap();
    let ring = Arc::new(Mutex::new(RingBuffer::new(config).unwrap()));
    let start = Instant::now();
    let per_producer = MESSAGES / PRODUCERS;
    let mut producers = Vec::new();
    for _ in 0..PRODUCERS {
        let ring = ring.clone();
        producers.push(
            thread::spawn(move || {
                for _ in 0..per_producer {
                    loop {
                        let mut ring = ring.lock().unwrap();
                        if let Some((seq, slots)) = ring.try_claim_slots(1) {
                            slots[0].set_sequence(seq);
                            slots[0].set_data(&[0xbb; 64]);
                            ring.publish_batch(seq, 1);
                            break;
                        }
                    }
                }
            })
        );
    }
    let consumer = {
        let ring = ring.clone();
        thread::spawn(move || {
            let mut received = 0;
            while received < MESSAGES {
                let mut ring = ring.lock().unwrap();
                let msgs = ring.try_consume_batch(0, 64);
                if !msgs.is_empty() {
                    received += msgs.len();
                }
            }
        })
    };
    for p in producers {
        p.join().unwrap();
    }
    consumer.join().unwrap();
    let duration = start.elapsed();
    let throughput = (MESSAGES as f64) / duration.as_secs_f64();
    println!("  Producers: {}", PRODUCERS);
    println!("  Messages: {}", MESSAGES);
    println!("  Duration: {:.2}s", duration.as_secs_f64());
    println!("  Throughput: {:.2} M msgs/sec", throughput / 1_000_000.0);
}

fn mpmc_benchmark() {
    println!("\n🔹 MPMC (Multi-Producer, Multi-Consumer)");
    const MESSAGES: usize = 1_000_000;
    const PRODUCERS: usize = 4;
    const CONSUMERS: usize = 4;
    const BUFFER_SIZE: usize = 65536;
    let config = RingBufferConfig::new(BUFFER_SIZE).unwrap();
    let ring = Arc::new(Mutex::new(RingBuffer::new(config).unwrap()));
    let start = Instant::now();
    let per_producer = MESSAGES / PRODUCERS;
    let total_sent = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let total_received = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let mut producers = Vec::new();
    for _ in 0..PRODUCERS {
        let ring = ring.clone();
        let total_sent = total_sent.clone();
        producers.push(
            thread::spawn(move || {
                for _ in 0..per_producer {
                    loop {
                        let mut ring = ring.lock().unwrap();
                        if let Some((seq, slots)) = ring.try_claim_slots(1) {
                            slots[0].set_sequence(seq);
                            slots[0].set_data(&[0xcc; 64]);
                            ring.publish_batch(seq, 1);
                            total_sent.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                            break;
                        }
                    }
                }
            })
        );
    }
    let mut consumers = Vec::new();
    for _ in 0..CONSUMERS {
        let ring = ring.clone();
        let total_received = total_received.clone();
        consumers.push(
            thread::spawn(move || {
                let mut received = 0;
                while total_received.load(std::sync::atomic::Ordering::Relaxed) < MESSAGES {
                    let mut ring = ring.lock().unwrap();
                    let msgs = ring.try_consume_batch(0, 64);
                    if !msgs.is_empty() {
                        received += msgs.len();
                        total_received.fetch_add(msgs.len(), std::sync::atomic::Ordering::Relaxed);
                    }
                }
            })
        );
    }
    for p in producers {
        p.join().unwrap();
    }
    for c in consumers {
        c.join().unwrap();
    }
    let duration = start.elapsed();
    let throughput = (MESSAGES as f64) / duration.as_secs_f64();
    println!("  Producers: {}", PRODUCERS);
    println!("  Consumers: {}", CONSUMERS);
    println!("  Messages: {}", MESSAGES);
    println!("  Duration: {:.2}s", duration.as_secs_f64());
    println!("  Throughput: {:.2} M msgs/sec", throughput / 1_000_000.0);
}

fn main() {
    println!("🚀 Flux Multi-Threaded Ring Buffer Benchmark");
    println!("==============================================");
    spsc_benchmark();
    mpsc_benchmark();
    mpmc_benchmark();
}

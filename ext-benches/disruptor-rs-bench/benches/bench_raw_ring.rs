//! RAW ring buffer test - minimal implementation to isolate performance
//! This implements a ring buffer similar to disruptor-rs but in Rust style

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

const BATCH_SIZE: usize = 8192;
const TEST_DURATION_SECS: u64 = 5;
const RING_SIZE: usize = 1024 * 1024;

// Minimal 8-byte slot
#[repr(C, align(8))]
#[derive(Clone, Copy)]
struct Slot {
    value: u64,
}

fn main() {
    println!("\n╔══════════════════════════════════════════╗");
    println!("║  RAW Ring Buffer - Minimal Test         ║");
    println!("╚══════════════════════════════════════════╝\n");

    let throughput = benchmark_raw_ring();
    println!("  Raw Ring: {:.2}M msgs/sec\n", throughput);

    let disruptor_throughput = benchmark_disruptor();
    println!("  disruptor-rs: {:.2}M msgs/sec\n", disruptor_throughput);

    println!("╔══════════════════════════════════════════╗");
    println!("║  COMPARISON                              ║");
    println!("╚══════════════════════════════════════════╝");
    println!("  Raw Ring:     {:.2}M msgs/sec", throughput);
    println!("  disruptor-rs: {:.2}M msgs/sec", disruptor_throughput);

    if throughput < disruptor_throughput {
        let gap = ((disruptor_throughput - throughput) / throughput) * 100.0;
        println!("  Gap:          {:.1}% slower", gap);
    } else {
        let gain = ((throughput - disruptor_throughput) / disruptor_throughput) * 100.0;
        println!("  Gap:          {:.1}% FASTER!", gain);
    }
}

fn benchmark_raw_ring() -> f64 {
    // Allocate ring buffer
    let buffer: Vec<Slot> = vec![Slot { value: 0 }; RING_SIZE];
    let buffer = Arc::new(buffer);

    let producer_cursor = Arc::new(AtomicU64::new(0)); // Start at 0
    let consumer_cursor = Arc::new(AtomicU64::new(0));
    let messages_consumed = Arc::new(AtomicU64::new(0));
    let messages_sent = Arc::new(AtomicU64::new(0));
    let producer_sum = Arc::new(AtomicU64::new(0));
    let consumer_sum = Arc::new(AtomicU64::new(0));
    let stop_flag = Arc::new(AtomicBool::new(false));

    // Producer
    let buf_prod = buffer.clone();
    let prod_cursor = producer_cursor.clone();
    let cons_cursor = consumer_cursor.clone();
    let sent = messages_sent.clone();
    let prod_sum = producer_sum.clone();
    let stop_prod = stop_flag.clone();

    let producer = thread::spawn(move || {
        let mask = RING_SIZE - 1;
        let mut value = 1u64;
        let mut local_cursor = 0u64; // Start at 0
        let mut local_sum = 0u64;
        let mut local_sent = 0u64;

        while !stop_prod.load(Ordering::Relaxed) {
            let next = local_cursor + BATCH_SIZE as u64;

            // Check space - simple: next must not lap consumer
            let consumer_seq = cons_cursor.load(Ordering::Relaxed);
            if next - consumer_seq > RING_SIZE as u64 {
                continue; // Ring full
            }

            // Write slots
            for i in 0..BATCH_SIZE {
                let seq = local_cursor + i as u64;
                let idx = (seq as usize) & mask;
                unsafe {
                    let slot_ptr = buf_prod.as_ptr().add(idx) as *mut Slot;
                    std::ptr::write_volatile(slot_ptr, Slot { value }); // Force write!
                }
                local_sum = local_sum.wrapping_add(value);
                value = value.wrapping_add(1);
            }
            local_sent += BATCH_SIZE as u64;

            // Update local cursor
            local_cursor = next;

            // Publish with Release (fence ensures writes are visible)
            std::sync::atomic::fence(Ordering::Release);
            prod_cursor.store(next, Ordering::Relaxed);
        }

        sent.store(local_sent, Ordering::Release);
        prod_sum.store(local_sum, Ordering::Release);
    });

    // Consumer
    let buf_cons = buffer.clone();
    let prod_cursor_cons = producer_cursor.clone();
    let cons_cursor_cons = consumer_cursor.clone();
    let consumed = messages_consumed.clone();
    let cons_sum = consumer_sum.clone();
    let stop_cons = stop_flag.clone();

    let consumer = thread::spawn(move || {
        let mask = RING_SIZE - 1;
        let mut local_cursor = 0u64; // Start at 0
        let mut local_sum = 0u64;
        let mut local_consumed = 0u64;
        let mut stop_seen = false;

        loop {
            let producer_seq = prod_cursor_cons.load(Ordering::Acquire);
            let available = producer_seq.saturating_sub(local_cursor);

            if available > 0 {
                let to_consume = available.min(BATCH_SIZE as u64) as usize;

                // Read slots with acquire fence
                std::sync::atomic::fence(Ordering::Acquire);
                for i in 0..to_consume {
                    let seq = local_cursor + i as u64;
                    let idx = (seq as usize) & mask;
                    unsafe {
                        let slot_ptr = buf_cons.as_ptr().add(idx);
                        let slot = std::ptr::read_volatile(slot_ptr); // Force read!
                        local_sum = local_sum.wrapping_add(slot.value);
                    }
                }

                local_cursor += to_consume as u64;
                cons_cursor_cons.store(local_cursor, Ordering::Release);
                local_consumed += to_consume as u64;
            } else {
                // No data available
                if !stop_seen && stop_cons.load(Ordering::Acquire) {
                    stop_seen = true;
                    // Wait for producer to finish publishing
                    thread::sleep(Duration::from_millis(10));
                    continue;
                }
                
                if stop_seen {
                    // Double-check we got everything
                    std::sync::atomic::fence(Ordering::SeqCst);
                    let final_producer_seq = prod_cursor_cons.load(Ordering::Acquire);
                    if final_producer_seq == local_cursor {
                        break;
                    }
                }
                
                std::hint::spin_loop();
            }
        }

        consumed.store(local_consumed, Ordering::Release);
        cons_sum.store(local_sum, Ordering::Release);
    });

    // Run
    let start = Instant::now();
    thread::sleep(Duration::from_secs(TEST_DURATION_SECS));
    let duration = start.elapsed().as_secs_f64();

    stop_flag.store(true, Ordering::Release);
    producer.join().unwrap();
    thread::sleep(Duration::from_secs(1));
    consumer.join().unwrap();

    let total_sent = messages_sent.load(Ordering::Relaxed);
    let total_consumed = messages_consumed.load(Ordering::Relaxed);
    let prod_sum_val = producer_sum.load(Ordering::Relaxed);
    let cons_sum_val = consumer_sum.load(Ordering::Relaxed);

    let throughput = (total_consumed as f64 / duration) / 1_000_000.0;

    println!(
        "    Sent:     {} msgs ({:.2}M)",
        total_sent,
        total_sent as f64 / 1_000_000.0
    );
    println!(
        "    Consumed: {} msgs ({:.2}M)",
        total_consumed,
        total_consumed as f64 / 1_000_000.0
    );
    println!(
        "    Delivery: {:.2}%",
        (total_consumed as f64 / total_sent as f64) * 100.0
    );
    
    // Calculate expected sum for the consumed messages
    // Sum of 1..N = N*(N+1)/2
    // Use u128 to avoid overflow in the multiplication
    let expected_sum = ((total_consumed as u128 * (total_consumed as u128 + 1)) / 2) as u64;
    
    println!("    Producer sum: {}", prod_sum_val);
    println!("    Consumer sum: {}", cons_sum_val);
    println!("    Expected sum: {} (for {} messages)", expected_sum, total_consumed);
    
    let integrity_ok = if total_consumed == total_sent {
        prod_sum_val == cons_sum_val && cons_sum_val == expected_sum
    } else {
        // If counts don't match, just check consumer got what it should have
        cons_sum_val == expected_sum
    };
    
    println!(
        "    Data integrity: {}",
        if integrity_ok {
            "✅ VERIFIED"
        } else {
            "❌ CORRUPTED"
        }
    );

    throughput
}

fn benchmark_disruptor() -> f64 {
    use disruptor::*;

    let messages_consumed = Arc::new(AtomicU64::new(0));
    let stop_flag = Arc::new(AtomicBool::new(false));

    let consumed = messages_consumed.clone();
    let stop_consumer = stop_flag.clone();

    let factory = || 0u64;

    let mut producer = build_single_producer(RING_SIZE, factory, BusySpin)
        .handle_events_with(move |_data: &u64, _seq: Sequence, _eob: bool| {
            if stop_consumer.load(Ordering::Relaxed) {
                return;
            }
            consumed.fetch_add(1, Ordering::Relaxed);
        })
        .build();

    let stop_producer = stop_flag.clone();

    let producer_thread = thread::spawn(move || {
        let mut value = 1u64;

        while !stop_producer.load(Ordering::Relaxed) {
            for _ in 0..BATCH_SIZE {
                producer.publish(|slot| {
                    *slot = value;
                });
                value = value.wrapping_add(1);
            }
        }
    });

    // Run
    let start = Instant::now();
    thread::sleep(Duration::from_secs(TEST_DURATION_SECS));
    let duration = start.elapsed().as_secs_f64();

    stop_flag.store(true, Ordering::Relaxed);
    producer_thread.join().unwrap();
    thread::sleep(Duration::from_secs(1));

    let total = messages_consumed.load(Ordering::Relaxed);
    (total as f64 / duration) / 1_000_000.0
}

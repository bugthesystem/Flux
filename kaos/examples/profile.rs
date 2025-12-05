//! Kaos Profiler - Find performance issues with Tracy
//!
//! Usage:
//!   cargo run -p kaos --example profile --features tracy --release -- [scenario]
//!
//! Scenarios:
//!   batch      - Batch operations (optimal)
//!   per-event  - Per-event operations (compare overhead)
//!   contention - High contention stress test
//!   backpressure - Trigger backpressure events
//!   all        - Run all scenarios

fn main() {
    #[cfg(not(feature = "tracy"))]
    {
        eprintln!("This example requires the 'tracy' feature:");
        eprintln!("  cargo run -p kaos --example profile --features tracy --release");
        std::process::exit(1);
    }

    #[cfg(feature = "tracy")]
    run();
}

#[cfg(feature = "tracy")]
fn run() {
    use kaos::disruptor::{RingBuffer, RingBufferEntry, Slot64};
    use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
    use std::sync::Arc;
    use std::thread;
    use std::time::{Duration, Instant};
    use tracing::trace_span;

    const RING_SIZE: usize = 64 * 1024;
    const WARMUP: u64 = 100_000;
    const ITERATIONS: u64 = 1_000_000;

    kaos::init_tracy();

    let scenario = std::env::args().nth(1).unwrap_or_else(|| "all".into());

    println!("╔═══════════════════════════════════════════╗");
    println!("║  Kaos Profiler - Connect Tracy Now        ║");
    println!("╚═══════════════════════════════════════════╝\n");
    println!("Scenario: {}\n", scenario);

    thread::sleep(Duration::from_secs(2));

    match scenario.as_str() {
        "batch" => run_batch(RING_SIZE, WARMUP, ITERATIONS),
        "per-event" => run_per_event(RING_SIZE, WARMUP, ITERATIONS),
        "contention" => run_contention(RING_SIZE, ITERATIONS),
        "backpressure" => run_backpressure(),
        "all" => {
            run_batch(RING_SIZE, WARMUP, ITERATIONS);
            thread::sleep(Duration::from_secs(1));
            run_per_event(RING_SIZE, WARMUP, ITERATIONS);
            thread::sleep(Duration::from_secs(1));
            run_contention(RING_SIZE, ITERATIONS);
            thread::sleep(Duration::from_secs(1));
            run_backpressure();
        }
        _ => {
            eprintln!("Unknown scenario: {}", scenario);
            eprintln!("Options: batch, per-event, contention, backpressure, all");
        }
    }

    println!("\n✓ Done - Check Tracy for analysis");

    fn run_batch(ring_size: usize, warmup: u64, iterations: u64) {
        println!("═══ Batch Operations ═══");

        let ring = Arc::new(RingBuffer::<Slot64>::new(ring_size).unwrap());
        let ring2 = ring.clone();
        let consumer_seq = Arc::new(AtomicU64::new(0));
        let consumer_seq2 = consumer_seq.clone();
        let done = Arc::new(AtomicBool::new(false));
        let done2 = done.clone();

        let consumer = thread::spawn(move || {
            let mut local_seq = 0u64;
            while !done2.load(Ordering::Relaxed) || local_seq < warmup + iterations {
                let _span = trace_span!("batch_consume").entered();
                let batch = ring2.get_read_batch(local_seq, 64);
                if !batch.is_empty() {
                    kaos::record_receive((batch.len() * 64) as u64);
                    local_seq += batch.len() as u64;
                    consumer_seq2.store(local_seq, Ordering::Release);
                } else {
                    std::hint::spin_loop();
                }
            }
        });

        let mut seq = 0u64;
        while seq < warmup {
            let cursor = consumer_seq.load(Ordering::Acquire);
            if let Some((claimed_seq, slots)) = ring.try_claim_slots(64, cursor) {
                for (i, slot) in slots.iter_mut().enumerate() {
                    slot.set_sequence(seq + i as u64);
                }
                ring.publish(claimed_seq + slots.len() as u64);
                seq += slots.len() as u64;
            }
        }

        let start = Instant::now();
        while seq < warmup + iterations {
            let _span = trace_span!("batch_publish").entered();
            let cursor = consumer_seq.load(Ordering::Acquire);
            if let Some((claimed_seq, slots)) = ring.try_claim_slots(64, cursor) {
                for (i, slot) in slots.iter_mut().enumerate() {
                    slot.set_sequence(seq + i as u64);
                }
                ring.publish(claimed_seq + slots.len() as u64);
                seq += slots.len() as u64;
                kaos::record_send((slots.len() * 64) as u64);
            } else {
                kaos::record_backpressure();
                std::hint::spin_loop();
            }
        }
        let elapsed = start.elapsed();

        done.store(true, Ordering::Relaxed);
        consumer.join().unwrap();

        println!(
            "  {} events in {:?} ({:.1} M/s)",
            iterations,
            elapsed,
            iterations as f64 / elapsed.as_secs_f64() / 1e6
        );
    }

    fn run_per_event(ring_size: usize, warmup: u64, iterations: u64) {
        println!("═══ Per-Event Operations ═══");

        let ring = Arc::new(RingBuffer::<Slot64>::new(ring_size).unwrap());
        let ring2 = ring.clone();
        let consumer_seq = Arc::new(AtomicU64::new(0));
        let consumer_seq2 = consumer_seq.clone();
        let done = Arc::new(AtomicBool::new(false));
        let done2 = done.clone();

        let consumer = thread::spawn(move || {
            let mut local_seq = 0u64;
            while !done2.load(Ordering::Relaxed) || local_seq < warmup + iterations {
                let _span = trace_span!("single_consume").entered();
                let batch = ring2.get_read_batch(local_seq, 1);
                if !batch.is_empty() {
                    kaos::record_receive(64);
                    local_seq += 1;
                    consumer_seq2.store(local_seq, Ordering::Release);
                } else {
                    std::hint::spin_loop();
                }
            }
        });

        let mut seq = 0u64;
        while seq < warmup {
            let cursor = consumer_seq.load(Ordering::Acquire);
            if let Some((claimed_seq, slots)) = ring.try_claim_slots(1, cursor) {
                slots[0].set_sequence(seq);
                ring.publish(claimed_seq + 1);
                seq += 1;
            }
        }

        let start = Instant::now();
        while seq < warmup + iterations {
            let _span = trace_span!("single_publish").entered();
            let cursor = consumer_seq.load(Ordering::Acquire);
            if let Some((claimed_seq, slots)) = ring.try_claim_slots(1, cursor) {
                slots[0].set_sequence(seq);
                ring.publish(claimed_seq + 1);
                seq += 1;
                kaos::record_send(64);
            } else {
                kaos::record_backpressure();
                std::hint::spin_loop();
            }
        }
        let elapsed = start.elapsed();

        done.store(true, Ordering::Relaxed);
        consumer.join().unwrap();

        println!(
            "  {} events in {:?} ({:.1} M/s)",
            iterations,
            elapsed,
            iterations as f64 / elapsed.as_secs_f64() / 1e6
        );
    }

    fn run_contention(ring_size: usize, iterations: u64) {
        println!("═══ Contention Test ═══");

        let ring = Arc::new(RingBuffer::<Slot64>::new(ring_size).unwrap());
        let ring2 = ring.clone();
        let consumer_seq = Arc::new(AtomicU64::new(0));
        let consumer_seq2 = consumer_seq.clone();
        let done = Arc::new(AtomicBool::new(false));
        let done2 = done.clone();

        let consumer = thread::spawn(move || {
            let mut local_seq = 0u64;
            while !done2.load(Ordering::Relaxed) {
                let _span = trace_span!("contention_consume").entered();
                let batch = ring2.get_read_batch(local_seq, 32);
                if !batch.is_empty() {
                    kaos::record_receive((batch.len() * 64) as u64);
                    local_seq += batch.len() as u64;
                    consumer_seq2.store(local_seq, Ordering::Release);
                    std::hint::spin_loop();
                } else {
                    std::hint::spin_loop();
                }
            }
            local_seq
        });

        let start = Instant::now();
        let mut seq = 0u64;

        while seq < iterations {
            let _span = trace_span!("contention_pub").entered();
            let cursor = consumer_seq.load(Ordering::Acquire);
            if let Some((claimed_seq, slots)) = ring.try_claim_slots(64, cursor) {
                for (i, slot) in slots.iter_mut().enumerate() {
                    slot.set_sequence(seq + i as u64);
                }
                ring.publish(claimed_seq + slots.len() as u64);
                seq += slots.len() as u64;
                kaos::record_send((slots.len() * 64) as u64);
            } else {
                kaos::record_backpressure();
                std::hint::spin_loop();
            }
        }

        done.store(true, Ordering::Relaxed);
        let consumed = consumer.join().unwrap();
        let elapsed = start.elapsed();

        println!(
            "  {} sent, {} consumed in {:?} ({:.1} M/s)",
            seq,
            consumed,
            elapsed,
            seq as f64 / elapsed.as_secs_f64() / 1e6
        );
    }

    fn run_backpressure() {
        println!("═══ Backpressure Test ═══");

        let ring = Arc::new(RingBuffer::<Slot64>::new(256).unwrap());
        let ring2 = ring.clone();
        let consumer_seq = Arc::new(AtomicU64::new(0));
        let consumer_seq2 = consumer_seq.clone();
        let done = Arc::new(AtomicBool::new(false));
        let done2 = done.clone();
        let backpressure_count = Arc::new(AtomicU64::new(0));
        let bp_count = backpressure_count.clone();

        let consumer = thread::spawn(move || {
            let mut local_seq = 0u64;
            while !done2.load(Ordering::Relaxed) {
                let batch = ring2.get_read_batch(local_seq, 8);
                if !batch.is_empty() {
                    kaos::record_receive((batch.len() * 64) as u64);
                    local_seq += batch.len() as u64;
                    consumer_seq2.store(local_seq, Ordering::Release);
                    thread::sleep(Duration::from_micros(100));
                }
            }
            local_seq
        });

        let start = Instant::now();
        let mut seq = 0u64;
        let target = 10_000u64;

        while seq < target {
            let _span = trace_span!("bp_publish").entered();
            let cursor = consumer_seq.load(Ordering::Acquire);
            if let Some((claimed_seq, slots)) = ring.try_claim_slots(32, cursor) {
                for (i, slot) in slots.iter_mut().enumerate() {
                    slot.set_sequence(seq + i as u64);
                }
                ring.publish(claimed_seq + slots.len() as u64);
                seq += slots.len() as u64;
                kaos::record_send((slots.len() * 64) as u64);
            } else {
                kaos::record_backpressure();
                bp_count.fetch_add(1, Ordering::Relaxed);
                std::hint::spin_loop();
            }
        }
        let elapsed = start.elapsed();

        done.store(true, Ordering::Relaxed);
        let consumed = consumer.join().unwrap();

        let bp = backpressure_count.load(Ordering::Relaxed);
        println!("  {} sent, {} consumed in {:?}", seq, consumed, elapsed);
        println!(
            "  Backpressure events: {} ({:.1}%)",
            bp,
            bp as f64 / (seq + bp) as f64 * 100.0
        );
    }
}

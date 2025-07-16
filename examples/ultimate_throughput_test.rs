use std::time::{ Duration, Instant };
use std::thread;
use std::sync::Arc;
use std::sync::atomic::{ AtomicU64, Ordering };
use flux::disruptor::{
    RingBufferConfig,
    ring_buffer::MappedRingBuffer,
    MessageSlot,
    RingBufferEntry,
};
use flux::utils::{ pin_to_cpu, macos_optimizations };

// Ultra-minimal message for maximum throughput
fn generate_ultra_minimal_message(seq: u64) -> Vec<u8> {
    let mut message = Vec::with_capacity(32); // 32 bytes only!
    message.extend_from_slice(&seq.to_le_bytes());
    message.extend_from_slice(b"ULTRA");
    message.resize(32, 0);
    message
}

fn main() {
    println!("🚀 ULTIMATE THROUGHPUT TEST - PUSHING BEYOND 100M!");
    println!("=====================================================");

    // Create ultimate configuration
    let config = RingBufferConfig {
        size: 2 * 1024 * 1024, // 2M slots for more headroom
        num_consumers: 8, // More consumers for parallel processing
        wait_strategy: flux::disruptor::WaitStrategyType::BusySpin,
        use_huge_pages: false,
        numa_node: None,
        optimal_batch_size: 131072, // 128K batch size!
        enable_cache_prefetch: true,
        enable_simd: true,
    };

    let buffer = Arc::new(MappedRingBuffer::new_mapped(config).unwrap());
    let start_time = Instant::now();

    // Shared atomic counter for coordination
    let total_sent = Arc::new(AtomicU64::new(0));
    let total_received = Arc::new(AtomicU64::new(0));

    // ULTIMATE PRODUCERS - 8 producers for maximum throughput
    let producer_handles: Vec<_> = (0..8)
        .map(|producer_id| {
            let buffer = Arc::clone(&buffer);
            let total_sent = Arc::clone(&total_sent);
            thread::spawn(move || {
                let _ = pin_to_cpu(producer_id);
                let _ = macos_optimizations::ThreadOptimizer::set_max_priority();

                let mut messages = 0u64;
                let batch_size = 131072; // 128K batch!

                while start_time.elapsed() < Duration::from_secs(10) {
                    if let Some((seq, slots)) = buffer.try_claim_slots(batch_size) {
                        for (i, slot) in slots.iter_mut().enumerate() {
                            let message = generate_ultra_minimal_message(messages + (i as u64));
                            slot.set_data(&message);
                            slot.set_sequence(seq + (i as u64));
                            messages += 1;
                        }
                        buffer.publish_batch(seq, slots.len());
                        total_sent.fetch_add(slots.len() as u64, Ordering::Relaxed);
                    }
                }
                messages
            })
        })
        .collect();

    // ULTIMATE CONSUMERS - 8 consumers for parallel processing
    let consumer_handles: Vec<_> = (0..8)
        .map(|consumer_id| {
            let buffer = Arc::clone(&buffer);
            let total_received = Arc::clone(&total_received);
            thread::spawn(move || {
                let _ = pin_to_cpu(consumer_id + 8); // Use cores 8-15
                let _ = macos_optimizations::ThreadOptimizer::set_max_priority();

                let mut messages = 0u64;
                let batch_size = 131072; // 128K batch!

                while start_time.elapsed() < Duration::from_secs(10) {
                    let slots = buffer.try_consume_batch(consumer_id, batch_size);
                    if !slots.is_empty() {
                        // Ultra-fast processing - just count valid sequences
                        for slot in slots {
                            if slot.sequence() > 0 {
                                messages += 1;
                            }
                        }
                        total_received.fetch_add(slots.len() as u64, Ordering::Relaxed);
                    }
                }
                messages
            })
        })
        .collect();

    // Wait for all threads
    let sent: u64 = producer_handles
        .into_iter()
        .map(|h| h.join().unwrap())
        .sum();

    let received: u64 = consumer_handles
        .into_iter()
        .map(|h| h.join().unwrap())
        .sum();

    let duration = start_time.elapsed();
    let final_sent = total_sent.load(Ordering::Relaxed);
    let final_received = total_received.load(Ordering::Relaxed);

    println!("🔥 ULTIMATE THROUGHPUT RESULTS:");
    println!("=================================");
    println!("Messages sent: {}", final_sent);
    println!("Messages received: {}", final_received);
    println!("Duration: {:?}", duration);
    println!(
        "Throughput: {:.0} K messages/second",
        (final_sent as f64) / duration.as_secs_f64() / 1000.0
    );
    println!("Processing rate: {:.1}%", ((final_received as f64) / (final_sent as f64)) * 100.0);
    println!("Average message size: 32 bytes");
    println!("Batch size: 128K messages");
    println!("Producers: 8, Consumers: 8");
    println!("Optimizations: P-core pinning, extreme batching, parallel processing");

    // Performance analysis
    let throughput_mps = (final_sent as f64) / duration.as_secs_f64();
    if throughput_mps > 100_000_000.0 {
        println!(
            "🎉 BREAKTHROUGH: {:.0}M messages/second - BEYOND 100M TARGET!",
            throughput_mps / 1_000_000.0
        );
    } else if throughput_mps > 50_000_000.0 {
        println!(
            "🚀 EXCELLENT: {:.0}M messages/second - AERON-LEVEL PERFORMANCE!",
            throughput_mps / 1_000_000.0
        );
    } else {
        println!(
            "✅ GOOD: {:.0}M messages/second - Still impressive!",
            throughput_mps / 1_000_000.0
        );
    }
}

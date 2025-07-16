use std::time::{ Duration, Instant };
use std::thread;
use std::sync::Arc;
use flux::disruptor::{
    RingBufferConfig,
    ring_buffer::MappedRingBuffer,
    MessageSlot,
    RingBufferEntry,
};
use flux::utils::{ pin_to_cpu, macos_optimizations };

// Minimal message generation for high throughput
fn generate_minimal_message(seq: u64) -> Vec<u8> {
    let mut message = Vec::with_capacity(64);
    message.extend_from_slice(&seq.to_le_bytes());
    message.extend_from_slice(b"FLUX_HIGH_THROUGHPUT_TEST_MESSAGE");
    message.resize(64, 0); // Pad to 64 bytes
    message
}

fn main() {
    println!("🚀 REALISTIC HIGH THROUGHPUT BENCHMARK");
    println!("========================================");

    // Create optimized configuration
    let config = RingBufferConfig {
        size: 1024 * 1024, // 1M slots
        num_consumers: 4,
        wait_strategy: flux::disruptor::WaitStrategyType::BusySpin,
        use_huge_pages: false,
        numa_node: None,
        optimal_batch_size: 8192, // 8K batch for good balance
        enable_cache_prefetch: true,
        enable_simd: true,
    };

    let buffer = Arc::new(MappedRingBuffer::new_mapped(config).unwrap());
    let start_time = Instant::now();

    // Multiple producers and consumers for maximum throughput
    let producer_handles: Vec<_> = (0..4)
        .map(|producer_id| {
            let buffer = Arc::clone(&buffer);
            thread::spawn(move || {
                let _ = pin_to_cpu(producer_id);
                let _ = macos_optimizations::ThreadOptimizer::set_max_priority();

                let mut messages = 0u64;
                let batch_size = 8192;

                while start_time.elapsed() < Duration::from_secs(10) {
                    if let Some((seq, slots)) = buffer.try_claim_slots(batch_size) {
                        for (i, slot) in slots.iter_mut().enumerate() {
                            let message = generate_minimal_message(messages + (i as u64));
                            slot.set_data(&message);
                            slot.set_sequence(seq + (i as u64));
                            messages += 1;
                        }
                        buffer.publish_batch(seq, slots.len());
                    }
                }
                messages
            })
        })
        .collect();

    let consumer_handles: Vec<_> = (0..4)
        .map(|consumer_id| {
            let buffer = Arc::clone(&buffer);
            thread::spawn(move || {
                let _ = pin_to_cpu(consumer_id + 4); // Use different cores
                let _ = macos_optimizations::ThreadOptimizer::set_max_priority();

                let mut messages = 0u64;
                let batch_size = 8192;

                while start_time.elapsed() < Duration::from_secs(10) {
                    let slots = buffer.try_consume_batch(consumer_id, batch_size);
                    if !slots.is_empty() {
                        // Minimal processing - just validate sequence
                        for slot in slots {
                            if slot.sequence() > 0 {
                                messages += 1;
                            }
                        }
                    }
                }
                messages
            })
        })
        .collect();

    let sent: u64 = producer_handles
        .into_iter()
        .map(|h| h.join().unwrap())
        .sum();

    let received: u64 = consumer_handles
        .into_iter()
        .map(|h| h.join().unwrap())
        .sum();

    let duration = start_time.elapsed();

    println!("📊 REALISTIC HIGH THROUGHPUT RESULTS:");
    println!("======================================");
    println!("Messages sent: {}", sent);
    println!("Messages received: {}", received);
    println!("Duration: {:?}", duration);
    println!(
        "Throughput: {:.0} K messages/second",
        (sent as f64) / duration.as_secs_f64() / 1000.0
    );
    println!("Processing rate: {:.1}%", ((received as f64) / (sent as f64)) * 100.0);
    println!("Average message size: 64 bytes");
    println!("Optimizations: P-core pinning, batch processing, memory mapping");
}

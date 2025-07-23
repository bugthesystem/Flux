use std::time::{ Duration, Instant };
use std::thread;
use std::sync::Arc;
use flux::disruptor::{ RingBufferConfig, ring_buffer::MappedRingBuffer, RingBufferEntry };
use flux::utils::{ pin_to_cpu };
use flux::optimizations::macos_optimizations;

fn main() {
    println!("🚀 MINIMAL THROUGHPUT TEST");
    println!("===========================");

    // Create minimal configuration
    let config = RingBufferConfig {
        size: 1024 * 1024, // 1M slots
        num_consumers: 1,
        wait_strategy: flux::disruptor::WaitStrategyType::BusySpin,
        use_huge_pages: false,
        numa_node: None,
        optimal_batch_size: 1024,
        enable_cache_prefetch: true,
        enable_simd: true,
    };

    let buffer = Arc::new(MappedRingBuffer::new_mapped(config).unwrap());
    let start_time = Instant::now();

    // Single producer, single consumer - minimal overhead
    let producer_handle = thread::spawn({
        let buffer = Arc::clone(&buffer);
        move || {
            let _ = pin_to_cpu(0);
            let _ = macos_optimizations::ThreadOptimizer::set_max_priority();

            let mut messages = 0u64;
            let batch_size = 1024;

            while start_time.elapsed() < Duration::from_secs(10) {
                if let Some((_seq, slots)) = buffer.try_claim_slots(batch_size) {
                    for slot in slots.iter_mut() {
                        slot.set_sequence(messages);
                        messages += 1;
                    }
                    buffer.publish_batch(0, slots.len());
                }
            }
            messages
        }
    });

    let consumer_handle = thread::spawn({
        let buffer = Arc::clone(&buffer);
        move || {
            let _ = pin_to_cpu(1);
            let _ = macos_optimizations::ThreadOptimizer::set_max_priority();

            let mut messages = 0u64;
            let batch_size = 1024;

            while start_time.elapsed() < Duration::from_secs(10) {
                let slots = buffer.try_consume_batch(0, batch_size);
                if !slots.is_empty() {
                    messages += slots.len() as u64;
                }
            }
            messages
        }
    });

    let sent = producer_handle.join().unwrap();
    let received = consumer_handle.join().unwrap();
    let duration = start_time.elapsed();

    println!("📊 MINIMAL THROUGHPUT RESULTS:");
    println!("================================");
    println!("Messages sent: {}", sent);
    println!("Messages received: {}", received);
    println!("Duration: {:?}", duration);
    println!(
        "Throughput: {:.0} K messages/second",
        (sent as f64) / duration.as_secs_f64() / 1000.0
    );
    println!("Processing rate: {:.1}%", ((received as f64) / (sent as f64)) * 100.0);
}

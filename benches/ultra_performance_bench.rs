use std::sync::Arc;
use std::sync::{ atomic::{ AtomicU64, Ordering }, Mutex };
use std::thread;
use std::time::{ Duration, Instant };
use flux::disruptor::{ RingBuffer, RingBufferConfig, WaitStrategyType, RingBufferEntry };

fn main() {
    println!("🚀 ULTRA PERFORMANCE BENCHMARK");
    println!("================================");
    println!("🎯 Target: 100M+ messages/second");
    println!("");

    // Ultra-optimized configuration
    let config = RingBufferConfig::new(2 * 1024 * 1024) // 2M slots
        .unwrap()
        .with_consumers(8) // 8 consumers for parallel processing
        .unwrap()
        .with_wait_strategy(WaitStrategyType::BusySpin)
        .with_optimal_batch_size(65536) // 64K batch size for maximum throughput
        .with_cache_prefetch(true)
        .with_simd_optimizations(true);

    let buffer = Arc::new(
        Mutex::new(RingBuffer::new(config).expect("Failed to create ring buffer"))
    );
    let running = Arc::new(AtomicU64::new(1));
    let total_sent = Arc::new(AtomicU64::new(0));
    let total_received = Arc::new(AtomicU64::new(0));

    println!("🔥 Starting ultra-performance benchmark...");
    println!("⏱️  Running for 10 seconds...");
    println!("");

    let start_time = Instant::now();

    // Producer thread with ultra optimizations
    let producer_buffer = buffer.clone();
    let producer_running = running.clone();
    let producer_sent = total_sent.clone();
    let producer_handle = thread::spawn(move || {
        if let Err(_) = flux::utils::pin_to_cpu(0) {
            println!("⚠️  Could not pin producer to CPU 0");
        }
        let mut batch_count = 0;
        let test_data = vec![0x42u8; 64];
        while producer_running.load(Ordering::Relaxed) == 1 {
            if let Ok(mut buffer) = producer_buffer.try_lock() {
                if let Some((seq, slots)) = buffer.try_claim_slots_ultra(65536) {
                    batch_count += 1;
                    for (i, slot) in slots.iter_mut().enumerate() {
                        slot.set_sequence(seq + (i as u64));
                        slot.set_data(&test_data);
                    }
                    // Release the mutable borrow before calling publish_batch_ultra
                    drop(buffer);
                    // Re-acquire the lock for publishing
                    if let Ok(mut buffer) = producer_buffer.try_lock() {
                        buffer.publish_batch_ultra(seq, 65536);
                        producer_sent.fetch_add(65536 as u64, Ordering::Relaxed);
                    }
                }
            } else {
                std::hint::spin_loop();
            }
        }
        println!("🔥 Producer completed: {} batches", batch_count);
    });

    // Consumer threads with ultra optimizations
    let mut consumer_handles = Vec::new();
    for consumer_id in 0..8 {
        let consumer_buffer = buffer.clone();
        let consumer_running = running.clone();
        let consumer_received = total_received.clone();
        let handle = thread::spawn(move || {
            if let Err(_) = flux::utils::pin_to_cpu(consumer_id + 1) {
                println!("⚠️  Could not pin consumer {} to CPU {}", consumer_id, consumer_id + 1);
            }
            let mut messages_processed = 0;
            let mut batches_processed = 0;
            while consumer_running.load(Ordering::Relaxed) == 1 {
                if let Ok(buffer) = consumer_buffer.try_lock() {
                    let batch = buffer.try_consume_batch_ultra(consumer_id, 65536);
                    if !batch.is_empty() {
                        batches_processed += 1;
                        messages_processed += batch.len();
                        for slot in batch {
                            if slot.is_valid() {
                                let data = slot.data();
                                let mut checksum = 0u64;
                                for chunk in data.chunks(16) {
                                    for &byte in chunk {
                                        checksum = checksum.wrapping_add(byte as u64);
                                    }
                                }
                                std::hint::black_box(checksum);
                            }
                        }
                        consumer_received.fetch_add(batch.len() as u64, Ordering::Relaxed);
                    }
                } else {
                    std::hint::spin_loop();
                }
            }
            println!(
                "🔥 Consumer {}: {} messages, {} batches",
                consumer_id,
                messages_processed,
                batches_processed
            );
        });
        consumer_handles.push(handle);
    }

    // Run benchmark for 10 seconds
    thread::sleep(Duration::from_secs(10));
    running.store(0, Ordering::Relaxed);

    // Wait for all threads to complete
    producer_handle.join().unwrap();
    for handle in consumer_handles {
        handle.join().unwrap();
    }

    let duration = start_time.elapsed();
    let total_messages = total_sent.load(Ordering::Relaxed);
    let messages_per_second = (total_messages as f64) / duration.as_secs_f64();

    println!("");
    println!("🏆 ULTRA PERFORMANCE RESULTS");
    println!("============================");
    println!("📊 Total Messages: {}", total_messages);
    println!("📊 Duration: {:.3} seconds", duration.as_secs_f64());
    println!("📊 Throughput: {:.2} M messages/second", messages_per_second / 1_000_000.0);
    println!("📊 Messages/Core: {:.2} M/sec", messages_per_second / 1_000_000.0 / 8.0);
    println!("");

    if messages_per_second > 100_000_000.0 {
        println!("🎉 ULTRA PERFORMANCE ACHIEVED!");
        println!("✅ Exceeded 100M messages/second target");
        println!("🚀 Flux is ready for extreme workloads!");
    } else if messages_per_second > 50_000_000.0 {
        println!("🔥 EXCELLENT PERFORMANCE!");
        println!("✅ Achieved 50M+ messages/second");
        println!("💡 Room for further optimization");
    } else if messages_per_second > 10_000_000.0 {
        println!("⚡ GOOD PERFORMANCE!");
        println!("✅ Achieved 10M+ messages/second");
        println!("🔧 Consider system-level tuning");
    } else {
        println!("⚠️  PERFORMANCE OPTIMIZATION NEEDED");
        println!("📈 Current: {:.2} M messages/second", messages_per_second / 1_000_000.0);
        println!("🎯 Target: 100M+ messages/second");
    }

    println!("");
    println!("🔧 Performance Tips:");
    println!("• Use CPU isolation: isolcpus=0,1,2,3,4,5,6,7");
    println!("• Set CPU governor to performance mode");
    println!("• Disable CPU frequency scaling");
    println!("• Use huge pages for memory allocation");
    println!("• Run with real-time priority");
    println!("");
    println!("🚀 Flux Ultra Performance Benchmark Complete!");
}

use std::time::Instant;
use flux::disruptor::{ RingBuffer, RingBufferConfig, MessageSlot };

fn main() {
    println!("🔍 PROFILING ANALYSIS BENCHMARK");
    println!("=================================");

    // Profile 1: Memory allocation and copying
    profile_memory_operations();

    // Profile 2: Atomic operations
    profile_atomic_operations();

    // Profile 3: SIMD operations
    profile_simd_operations();

    // Profile 4: Cache line access patterns
    profile_cache_operations();

    println!("\n✅ Profiling analysis complete!");
}

fn profile_memory_operations() {
    println!("\n📊 Memory Operations Profile:");
    println!("-----------------------------");

    let config = RingBufferConfig::new(1024 * 1024).expect("config");
    let mut ring_buffer = RingBuffer::new(config).expect("ring buffer");

    // Test 1: Pure memory copy performance
    let test_data = vec![0u8; 1024];
    let iterations = 1_000_000;

    let start = Instant::now();
    for _ in 0..iterations {
        if let Some((_, slots)) = ring_buffer.try_claim_slots(1) {
            slots[0].set_data(&test_data);
        }
    }
    let duration = start.elapsed();

    println!("Memory copy operations: {} ops/sec", (iterations as f64) / duration.as_secs_f64());
}

fn profile_atomic_operations() {
    println!("\n📊 Atomic Operations Profile:");
    println!("-----------------------------");

    let config = RingBufferConfig::new(1024 * 1024).expect("config");
    let mut ring_buffer = RingBuffer::new(config).expect("ring buffer");

    // Test atomic sequence increments
    let iterations = 10_000_000;
    let start = Instant::now();

    for _ in 0..iterations {
        let _ = ring_buffer.try_claim_slots(1);
    }

    let duration = start.elapsed();
    println!(
        "Atomic sequence operations: {} ops/sec",
        (iterations as f64) / duration.as_secs_f64()
    );
}

fn profile_simd_operations() {
    println!("\n📊 SIMD Operations Profile:");
    println!("----------------------------");

    // Test hardware CRC32 calculation
    let test_data = vec![0u8; 1024];
    let iterations = 1_000_000;

    let start = Instant::now();
    for _ in 0..iterations {
        let checksum = MessageSlot::calculate_checksum_hardware(&test_data);
        std::hint::black_box(checksum);
    }
    let duration = start.elapsed();

    println!("Hardware CRC32 operations: {} ops/sec", (iterations as f64) / duration.as_secs_f64());
}

fn profile_cache_operations() {
    println!("\n📊 Cache Operations Profile:");
    println!("----------------------------");

    let config = RingBufferConfig::new(1024 * 1024).expect("config");
    let mut ring_buffer = RingBuffer::new(config).expect("ring buffer");

    // Test cache line access patterns
    let iterations = 100_000;
    let batch_size = 64; // One cache line worth of slots

    let start = Instant::now();
    for _ in 0..iterations {
        if let Some((_, slots)) = ring_buffer.try_claim_slots(batch_size) {
            for slot in slots {
                std::hint::black_box(slot);
            }
        }
    }
    let duration = start.elapsed();

    println!(
        "Cache line access operations: {} ops/sec",
        ((iterations * batch_size) as f64) / duration.as_secs_f64()
    );
}

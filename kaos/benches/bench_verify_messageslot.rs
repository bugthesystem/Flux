//! MessageSlot data integrity benchmark with Criterion
//!
//! Tests 128-byte MessageSlot with XOR checksum verification.

use criterion::{ criterion_group, criterion_main, Criterion, Throughput };
use std::sync::atomic::{ AtomicU64, Ordering };
use std::sync::Arc;
use std::thread;
use std::hint::black_box;

use kaos::disruptor::{ RingBuffer, MessageSlot, RingBufferEntry };

const RING_SIZE: usize = 1024 * 1024;
const BATCH_SIZE: usize = 8192;
const TOTAL_EVENTS: u64 = 5_000_000;

fn bench_messageslot_verified(events: u64) -> u64 {
    let ring = Arc::new(RingBuffer::<MessageSlot>::new(RING_SIZE).unwrap());
    let producer_cursor = ring.producer_cursor();
    let producer_xor = Arc::new(AtomicU64::new(0));
    let consumer_xor = Arc::new(AtomicU64::new(0));

    let ring_cons = ring.clone();
    let cons_xor = consumer_xor.clone();
    let consumer = thread::spawn(move || {
        let mut cursor = 0u64;
        let mut local_xor = 0u64;
        while cursor < events {
            let prod_seq = producer_cursor.load(Ordering::Acquire);
            if prod_seq > cursor {
                let batch = ((prod_seq - cursor) as usize).min(BATCH_SIZE);
                std::sync::atomic::fence(Ordering::Acquire);
                for i in 0..batch {
                    let seq = cursor + (i as u64);
                    unsafe {
                        let slot = ring_cons.read_slot(seq);
                        local_xor ^= slot.sequence();
                    }
                }
                cursor += batch as u64;
                ring_cons.update_consumer(cursor);
            }
        }
        cons_xor.store(local_xor, Ordering::Release);
    });

    let ring_prod = ring.clone();
    let prod_xor = producer_xor.clone();
    let mut cursor = 0u64;
    let mut value = 1u64;
    let mut local_xor = 0u64;

    let ring_ptr = Arc::as_ptr(&ring_prod) as *mut RingBuffer<MessageSlot>;

    while cursor < events {
        let ring_mut = unsafe { &mut *ring_ptr };
        let batch = ((events - cursor) as usize).min(BATCH_SIZE);
        if let Some(next) = ring_mut.try_claim(batch, cursor) {
            for i in 0..batch {
                let seq = cursor + (i as u64);
                let mut slot = MessageSlot::default();
                slot.set_sequence(value);
                unsafe {
                    ring_mut.write_slot(seq, slot);
                }
                local_xor ^= value;
                value = value.wrapping_add(1);
            }
            cursor = next;
            ring_mut.publish(next);
        }
    }

    prod_xor.store(local_xor, Ordering::Release);
    consumer.join().unwrap();

    // Verify integrity
    let p = producer_xor.load(Ordering::Acquire);
    let c = consumer_xor.load(Ordering::Acquire);
    assert_eq!(p, c, "Data integrity check failed!");

    black_box(events)
}

fn benchmark_messageslot(c: &mut Criterion) {
    let mut group = c.benchmark_group("MessageSlot Verified (5M events)");
    group.throughput(Throughput::Elements(TOTAL_EVENTS));
    group.sample_size(10);

    group.bench_function("128B_xor_verified", |b| {
        b.iter(|| bench_messageslot_verified(TOTAL_EVENTS))
    });

    group.finish();
}

criterion_group!(benches, benchmark_messageslot);
criterion_main!(benches);

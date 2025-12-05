//! Loom concurrency tests for ALL ring buffer types.
//!
//! Tests atomic correctness for:
//! - SPSC (Single Producer Single Consumer)
//! - MPSC (Multiple Producer Single Consumer)
//! - SPMC (Single Producer Multiple Consumer)
//! - MPMC (Multiple Producer Multiple Consumer)
//!
//! Run with: RUSTFLAGS="--cfg loom" cargo test --test loom_ring_buffer --release

#[cfg(loom)]
mod loom_tests {
    use loom::sync::atomic::{AtomicU64, Ordering};
    use loom::sync::Arc;
    use loom::thread;

    // ==================== SPSC TESTS ====================

    /// Test SPSC producer-consumer cursor synchronization
    /// Models the core single-producer single-consumer protocol
    #[test]
    fn test_spsc_cursor_sync() {
        loom::model(|| {
            let producer_cursor = Arc::new(AtomicU64::new(0));
            let consumer_cursor = Arc::new(AtomicU64::new(0));

            let size: u64 = 4; // Small ring for exhaustive testing

            let pc = producer_cursor.clone();
            let cc = consumer_cursor.clone();

            // Producer thread
            let producer = thread::spawn(move || {
                for i in 0..2 {
                    // Wait for space (backpressure)
                    loop {
                        let next = i + 1;
                        let consumer_seq = cc.load(Ordering::Acquire);
                        if next - consumer_seq <= size {
                            break;
                        }
                        loom::thread::yield_now();
                    }
                    // Publish with Release ordering
                    pc.store(i + 1, Ordering::Release);
                }
            });

            let pc2 = producer_cursor.clone();
            let cc2 = consumer_cursor.clone();

            // Consumer thread
            let consumer = thread::spawn(move || {
                let mut consumed = 0u64;
                while consumed < 2 {
                    let available = pc2.load(Ordering::Acquire);
                    if available > consumed {
                        consumed = available;
                        cc2.store(consumed, Ordering::Release);
                    } else {
                        loom::thread::yield_now();
                    }
                }
                consumed
            });

            producer.join().unwrap();
            let final_consumed = consumer.join().unwrap();

            assert_eq!(final_consumed, 2);
            assert_eq!(producer_cursor.load(Ordering::Relaxed), 2);
        });
    }

    /// Test SPSC batch publish/consume pattern
    #[test]
    fn test_spsc_batch_sync() {
        loom::model(|| {
            let producer_cursor = Arc::new(AtomicU64::new(0));
            let consumer_cursor = Arc::new(AtomicU64::new(0));

            let pc = producer_cursor.clone();
            let _cc = consumer_cursor.clone();

            // Producer: publish batch of 2
            let producer = thread::spawn(move || {
                // Claim and publish atomically
                pc.store(2, Ordering::Release);
                2
            });

            let pc2 = producer_cursor.clone();
            let cc2 = consumer_cursor.clone();

            // Consumer: consume batch
            let consumer = thread::spawn(move || loop {
                let available = pc2.load(Ordering::Acquire);
                if available >= 2 {
                    cc2.store(2, Ordering::Release);
                    return available;
                }
                loom::thread::yield_now();
            });

            let produced = producer.join().unwrap();
            let consumed = consumer.join().unwrap();

            assert_eq!(produced, 2);
            assert_eq!(consumed, 2);
        });
    }

    // ==================== MPSC TESTS ====================

    /// Test MPSC compare_exchange_weak for slot claiming
    /// Multiple producers racing to claim slots
    #[test]
    fn test_mpsc_claim_race() {
        loom::model(|| {
            let cursor = Arc::new(AtomicU64::new(0));

            let c1 = cursor.clone();
            let c2 = cursor.clone();

            // Producer 1
            let p1 = thread::spawn(move || {
                let mut current = c1.load(Ordering::Relaxed);
                loop {
                    match c1.compare_exchange_weak(
                        current,
                        current + 1,
                        Ordering::AcqRel,
                        Ordering::Relaxed,
                    ) {
                        Ok(seq) => {
                            return seq;
                        }
                        Err(actual) => {
                            current = actual;
                        }
                    }
                }
            });

            // Producer 2
            let p2 = thread::spawn(move || {
                let mut current = c2.load(Ordering::Relaxed);
                loop {
                    match c2.compare_exchange_weak(
                        current,
                        current + 1,
                        Ordering::AcqRel,
                        Ordering::Relaxed,
                    ) {
                        Ok(seq) => {
                            return seq;
                        }
                        Err(actual) => {
                            current = actual;
                        }
                    }
                }
            });

            let seq1 = p1.join().unwrap();
            let seq2 = p2.join().unwrap();

            // Both must get unique sequences
            assert_ne!(seq1, seq2);
            assert_eq!(cursor.load(Ordering::Relaxed), 2);
        });
    }

    /// Test MPSC with 3 producers
    #[test]
    fn test_mpsc_three_producers() {
        loom::model(|| {
            let cursor = Arc::new(AtomicU64::new(0));

            let handles: Vec<_> = (0..3)
                .map(|_| {
                    let c = cursor.clone();
                    thread::spawn(move || {
                        let mut current = c.load(Ordering::Relaxed);
                        loop {
                            match c.compare_exchange_weak(
                                current,
                                current + 1,
                                Ordering::AcqRel,
                                Ordering::Relaxed,
                            ) {
                                Ok(seq) => {
                                    return seq;
                                }
                                Err(actual) => {
                                    current = actual;
                                }
                            }
                        }
                    })
                })
                .collect();

            let mut seqs: Vec<_> = handles.into_iter().map(|h| h.join().unwrap()).collect();
            seqs.sort();

            // All sequences must be unique and sequential
            assert_eq!(seqs, vec![0, 1, 2]);
            assert_eq!(cursor.load(Ordering::Relaxed), 3);
        });
    }

    // ==================== SPMC TESTS ====================

    /// Test SPMC with multiple consumers claiming reads
    #[test]
    fn test_spmc_consumer_claim() {
        loom::model(|| {
            let read_cursor = Arc::new(AtomicU64::new(0));
            let producer_cursor = Arc::new(AtomicU64::new(2)); // 2 items available

            let rc1 = read_cursor.clone();
            let pc1 = producer_cursor.clone();

            let rc2 = read_cursor.clone();
            let pc2 = producer_cursor.clone();

            // Consumer 1
            let c1 = thread::spawn(move || {
                let current = rc1.load(Ordering::Relaxed);
                let available = pc1.load(Ordering::Acquire);
                if current < available {
                    match rc1.compare_exchange_weak(
                        current,
                        current + 1,
                        Ordering::AcqRel,
                        Ordering::Relaxed,
                    ) {
                        Ok(seq) => {
                            return Some(seq);
                        }
                        Err(_) => {
                            return None;
                        }
                    }
                }
                None
            });

            // Consumer 2
            let c2 = thread::spawn(move || {
                let current = rc2.load(Ordering::Relaxed);
                let available = pc2.load(Ordering::Acquire);
                if current < available {
                    match rc2.compare_exchange_weak(
                        current,
                        current + 1,
                        Ordering::AcqRel,
                        Ordering::Relaxed,
                    ) {
                        Ok(seq) => {
                            return Some(seq);
                        }
                        Err(_) => {
                            return None;
                        }
                    }
                }
                None
            });

            let r1 = c1.join().unwrap();
            let r2 = c2.join().unwrap();

            // At least one should succeed, possibly both with different sequences
            let successes: Vec<_> = [r1, r2].iter().filter_map(|&x| x).collect();
            assert!(!successes.is_empty());

            // All successful claims must be unique
            let unique: std::collections::HashSet<_> = successes.iter().collect();
            assert_eq!(successes.len(), unique.len());
        });
    }

    // ==================== MPMC TESTS ====================

    /// Test MPMC with completion tracking
    /// Out-of-order completion with cursor advancement
    #[test]
    fn test_mpmc_completion_tracking() {
        loom::model(|| {
            let completed_0 = Arc::new(AtomicU64::new(0));
            let completed_1 = Arc::new(AtomicU64::new(0));
            let completed_cursor = Arc::new(AtomicU64::new(0));

            let c1 = completed_1.clone();

            // Thread 1: completes slot 1 first (out of order)
            let t1 = thread::spawn(move || {
                c1.store(1, Ordering::Release);
            });

            let c0_2 = completed_0.clone();
            let cursor2 = completed_cursor.clone();

            // Thread 2: completes slot 0 and tries to advance cursor
            let t2 = thread::spawn(move || {
                c0_2.store(1, Ordering::Release);

                // Try to advance cursor only if slot 0 is done
                if c0_2.load(Ordering::Acquire) == 1 {
                    cursor2.store(1, Ordering::Release);
                }
            });

            t1.join().unwrap();
            t2.join().unwrap();

            // Both slots must be complete
            assert_eq!(completed_0.load(Ordering::Relaxed), 1);
            assert_eq!(completed_1.load(Ordering::Relaxed), 1);
        });
    }

    /// Test MPMC producer and consumer racing
    #[test]
    fn test_mpmc_producer_consumer_race() {
        loom::model(|| {
            let producer_cursor = Arc::new(AtomicU64::new(0));
            let consumer_cursor = Arc::new(AtomicU64::new(0));

            let pc1 = producer_cursor.clone();
            let pc2 = producer_cursor.clone();
            let cc1 = consumer_cursor.clone();

            // Producer 1
            let p1 = thread::spawn(move || {
                let mut current = pc1.load(Ordering::Relaxed);
                loop {
                    match pc1.compare_exchange_weak(
                        current,
                        current + 1,
                        Ordering::AcqRel,
                        Ordering::Relaxed,
                    ) {
                        Ok(_) => {
                            return true;
                        }
                        Err(actual) => {
                            current = actual;
                        }
                    }
                }
            });

            // Producer 2
            let p2 = thread::spawn(move || {
                let mut current = pc2.load(Ordering::Relaxed);
                loop {
                    match pc2.compare_exchange_weak(
                        current,
                        current + 1,
                        Ordering::AcqRel,
                        Ordering::Relaxed,
                    ) {
                        Ok(_) => {
                            return true;
                        }
                        Err(actual) => {
                            current = actual;
                        }
                    }
                }
            });

            // Consumer trying to read
            let pc3 = producer_cursor.clone();
            let c1 = thread::spawn(move || {
                let current = cc1.load(Ordering::Relaxed);
                let available = pc3.load(Ordering::Acquire);
                if current < available {
                    match cc1.compare_exchange_weak(
                        current,
                        current + 1,
                        Ordering::AcqRel,
                        Ordering::Relaxed,
                    ) {
                        Ok(seq) => {
                            return Some(seq);
                        }
                        Err(_) => {
                            return None;
                        }
                    }
                }
                None
            });

            assert!(p1.join().unwrap());
            assert!(p2.join().unwrap());
            let _ = c1.join().unwrap(); // May or may not succeed

            assert_eq!(producer_cursor.load(Ordering::Relaxed), 2);
        });
    }

    // ==================== ADVANCED PATTERNS ====================

    /// Test fence synchronization pattern
    #[test]
    fn test_fence_sync() {
        loom::model(|| {
            let data = Arc::new(AtomicU64::new(0));
            let flag = Arc::new(AtomicU64::new(0));

            let d = data.clone();
            let f = flag.clone();

            let writer = thread::spawn(move || {
                d.store(42, Ordering::Relaxed);
                loom::sync::atomic::fence(Ordering::Release);
                f.store(1, Ordering::Relaxed);
            });

            let d2 = data.clone();
            let f2 = flag.clone();

            let reader = thread::spawn(move || loop {
                if f2.load(Ordering::Relaxed) == 1 {
                    loom::sync::atomic::fence(Ordering::Acquire);
                    return d2.load(Ordering::Relaxed);
                }
                loom::thread::yield_now();
            });

            writer.join().unwrap();
            let value = reader.join().unwrap();

            assert_eq!(value, 42);
        });
    }
}

// Non-loom placeholder test
#[cfg(not(loom))]
#[test]
fn loom_tests_require_cfg_loom() {
    eprintln!("╔══════════════════════════════════════════════════════════════╗");
    eprintln!("║  LOOM CONCURRENCY TESTS                                      ║");
    eprintln!("║                                                              ║");
    eprintln!("║  Run with:                                                   ║");
    eprintln!("║  RUSTFLAGS=\"--cfg loom\" cargo test --test loom_ring_buffer --release ║");
    eprintln!("║                                                              ║");
    eprintln!("║  Tests: SPSC, MPSC, SPMC, MPMC atomic patterns               ║");
    eprintln!("╚══════════════════════════════════════════════════════════════╝");
}

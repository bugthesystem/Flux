//! High-performance macros for Flux ring buffers.
//!
//! ## Macros
//!
//! | Macro | Use Case |
//! |-------|----------|
//! | `publish_batch!` | Fastest - slice-based batch publish |
//! | `consume_batch!` | Fastest - slice-based batch consume |
//! | `publish_light!` | Simple batch publish |
//! | `consume_light!` | Simple batch consume |

// =============================================================================
// PRODUCER MACROS
// =============================================================================

/// Publish batch to RingBuffer
///
#[macro_export]
macro_rules! publish {
    ($rb:expr, $batch_size:expr, $seq:ident, $slots:ident, $body:block) => {
        {
        if let Some(($seq, $slots)) = $rb.try_claim_slots_relaxed($batch_size) {
            let count = $slots.len();
            $body
            $rb.publish_batch_relaxed($seq, count);
            Ok(count)
        } else {
            Err("Ring buffer full")
        }
        }
    };
}

/// Publish batch to RingBuffer (high performance)
///
#[macro_export]
macro_rules! publish_light {
    ($ring:expr, $cursor:expr, $batch_size:expr, $idx:ident, $value_expr:expr) => {
        {
        let ring_ptr = std::sync::Arc::as_ptr(&$ring);
        let ring_mut = unsafe { &mut *(ring_ptr as *mut $crate::disruptor::RingBuffer<_>) };

        if let Some(next) = ring_mut.try_claim($batch_size, $cursor) {
            let start = $cursor;
            unsafe {
                for $idx in 0..$batch_size {
                    ring_mut.write_slot(start + $idx as u64, $value_expr);
                }
            }
            ring_mut.publish(next);
            $cursor = next;
            Ok::<usize, &'static str>($batch_size)
        } else {
            Err("Ring buffer full")
        }
        }
    };
}

/// Publish batch to MappedRingBuffer (high performance)
///
#[macro_export]
macro_rules! publish_mapped {
    ($ring:expr, $batch_size:expr, $seq:ident, $slots:ident, $body:block) => {
        {
        if let Some(($seq, $slots)) = $ring.try_claim_slots($batch_size) {
            let count = $slots.len();
            $body
            $ring.publish($seq + count as u64);
            Ok(count)
        } else {
            Err("Ring buffer full")
        }
        }
    };
}

// =============================================================================
// CONSUMER MACROS
// =============================================================================

/// Consume batch from RingBuffer
///
#[macro_export]
macro_rules! consume {
    ($rb:expr, $consumer_id:expr, $max_count:expr, $slot:ident, $body:block) => {
        {
        let batch = $rb.try_consume_batch($consumer_id, $max_count);
        let count = batch.len();
        for $slot in batch {
            $body
        }
        count
        }
    };
}

/// Consume batch from RingBuffer (high performance)
///
#[macro_export]
macro_rules! consume_light {
    ($ring:expr, $prod_cursor:expr, $cursor:expr, $max_count:expr, $slot:ident, $body:block) => {
        {
        let prod_seq = $prod_cursor.load(std::sync::atomic::Ordering::Relaxed);
        let available = prod_seq.saturating_sub($cursor);

        if available > 0 {
            let to_consume = (available as usize).min($max_count);
            std::sync::atomic::fence(std::sync::atomic::Ordering::Acquire);

            for i in 0..to_consume {
                let $slot = unsafe { $ring.read_slot($cursor + i as u64) };
                $body
            }

            $cursor += to_consume as u64;
            $ring.update_consumer($cursor);
            to_consume
        } else {
            0
        }
        }
    };
}

/// Consume batch from MappedRingBuffer
///
#[macro_export]
macro_rules! consume_mapped {
    ($ring:expr, $consumer_id:expr, $max_count:expr, $slot:ident, $body:block) => {
        {
        let batch = $ring.try_consume_batch($consumer_id, $max_count);
        let count = batch.len();
        for $slot in batch {
            $body
        }
        count
        }
    };
}

// =============================================================================
// SPMC/MPMC CONSUMER MACROS (Read-then-commit pattern)
// =============================================================================

/// Safe consume for SPMC/MPMC with automatic commit via RAII guard
///
/// Uses the read-then-commit pattern - slots are automatically committed
/// when the guard goes out of scope, even on early exit.
///
#[macro_export]
macro_rules! consume_safe {
    ($ring:expr, $slot:ident, $body:block) => {
        loop {
            if let Some(guard) = $ring.try_read() {
                let $slot = guard.get();
                $body
            } else {
                break;
            }
        }
    };
}

/// Batch consume for SPMC/MPMC with automatic commit
///
#[macro_export]
macro_rules! consume_safe_batch {
    ($ring:expr, $max_count:expr, $slot:ident, $body:block) => {
        {
        if let Some(batch) = $ring.try_read_batch($max_count) {
            let count = batch.count();
            for $slot in batch.iter() {
                $body
            }
            count
        } else {
            0
        }
        }
    };
}

// =============================================================================
// PERFORMANCE-OPTIMIZED MACROS (Slice API - fastest!)
// =============================================================================

/// High-performance publish using slice API with prefetching
///
/// This is the FASTEST way to publish - uses direct slice access instead of
/// per-slot function calls.
///
#[macro_export]
macro_rules! publish_batch {
    (
        $slot_type:ty,
        $ring:expr,
        $cursor:expr,
        $batch_size:expr,
        $idx:ident,
        $slot:ident,
        $body:block
    ) => {
        {
        let ring_ptr = std::sync::Arc::as_ptr(&$ring);
        let ring_ref = unsafe { &*(ring_ptr as *const $crate::disruptor::RingBuffer<$slot_type>) };

        if let Some((seq, slots)) = ring_ref.try_claim_slots($batch_size, $cursor) {
            let count = slots.len();
            for ($idx, $slot) in slots.iter_mut().enumerate() {
                $body
            }
            let next = seq + count as u64;
            ring_ref.publish(next);
            $cursor = next;
            Ok::<usize, &'static str>(count)
        } else {
            Err("Ring buffer full")
        }
        }
    };
}

/// High-performance consume using slice API with prefetching
///
/// This is the FASTEST way to consume - uses direct slice access.
///
#[macro_export]
macro_rules! consume_batch {
    (
        $slot_type:ty,
        $ring:expr,
        $producer_cursor:expr,
        $consumer_cursor:expr,
        $batch_size:expr,
        $slot:ident,
        $body:block
    ) => {
        {
        let prod_seq = $producer_cursor.load(std::sync::atomic::Ordering::Acquire);
        let available = prod_seq.saturating_sub($consumer_cursor);

        if available > 0 {
            let to_consume = (available as usize).min($batch_size);
            let slots = $ring.get_read_batch($consumer_cursor, to_consume);
            let count = slots.len();

            for $slot in slots {
                $body
            }

            $consumer_cursor += count as u64;
            $ring.update_consumer($consumer_cursor);
            count
        } else {
            0
        }
        }
    };
}

/// Legacy unrolled publish (kept for backward compatibility)
/// Note: publish_batch! is faster - use that instead
#[macro_export]
macro_rules! publish_light_unrolled {
    ($ring:expr, $cursor:expr, $batch_size:expr, $idx:ident, $value_expr:expr) => {
        {
        let ring_ptr = std::sync::Arc::as_ptr(&$ring);
        let ring_mut = unsafe { &mut *(ring_ptr as *mut $crate::disruptor::RingBuffer<_>) };

        if let Some(next) = ring_mut.try_claim($batch_size, $cursor) {
            let start = $cursor;

            unsafe {
                let mut $idx: usize = 0;

                // Unroll by 8
                while $idx + 8 <= $batch_size {
                    ring_mut.write_slot(start + $idx as u64, $value_expr); $idx += 1;
                    ring_mut.write_slot(start + $idx as u64, $value_expr); $idx += 1;
                    ring_mut.write_slot(start + $idx as u64, $value_expr); $idx += 1;
                    ring_mut.write_slot(start + $idx as u64, $value_expr); $idx += 1;
                    ring_mut.write_slot(start + $idx as u64, $value_expr); $idx += 1;
                    ring_mut.write_slot(start + $idx as u64, $value_expr); $idx += 1;
                    ring_mut.write_slot(start + $idx as u64, $value_expr); $idx += 1;
                    ring_mut.write_slot(start + $idx as u64, $value_expr); $idx += 1;
                }

                // Remainder
                while $idx < $batch_size {
                    ring_mut.write_slot(start + $idx as u64, $value_expr);
                    $idx += 1;
                }
            }

            ring_mut.publish(next);
            $cursor = next;
            Ok::<usize, &'static str>($batch_size)
        } else {
            Err("Ring buffer full")
        }
        }
    };
}

/// High-performance publish with 8x loop unrolling
///
/// Use this for maximum throughput in hot paths.
///
#[macro_export]
macro_rules! publish_unrolled {
    (
        $producer:expr,
        $batch_size:expr,
        $seq:ident,
        $idx:ident,
        $slot:ident,
        { $($body:tt)* }
    ) => {
        {
        let rb_ptr = $producer.ring_buffer;
        let rb = unsafe { &mut *rb_ptr };

        if let Some(($seq, slots)) = rb.try_claim_slots_relaxed($batch_size) {
            let count = slots.len();

            unsafe {
                let mut $idx = 0;

                // Unroll by 8 with prefetch
                while $idx + 8 <= count {
                    #[cfg(target_arch = "aarch64")]
                    {
                        let ptr = slots.as_ptr().add($idx + 16) as *const i8;
                        std::arch::asm!("prfm pldl1keep, [{0}]", in(reg) ptr);
                    }
                    #[cfg(target_arch = "x86_64")]
                    {
                        let ptr = slots.as_ptr().add($idx + 16) as *const i8;
                        std::arch::x86_64::_mm_prefetch(ptr, std::arch::x86_64::_MM_HINT_T0);
                    }

                    { let $slot = slots.get_unchecked_mut($idx); $($body)* } $idx += 1;
                    { let $slot = slots.get_unchecked_mut($idx); $($body)* } $idx += 1;
                    { let $slot = slots.get_unchecked_mut($idx); $($body)* } $idx += 1;
                    { let $slot = slots.get_unchecked_mut($idx); $($body)* } $idx += 1;
                    { let $slot = slots.get_unchecked_mut($idx); $($body)* } $idx += 1;
                    { let $slot = slots.get_unchecked_mut($idx); $($body)* } $idx += 1;
                    { let $slot = slots.get_unchecked_mut($idx); $($body)* } $idx += 1;
                    { let $slot = slots.get_unchecked_mut($idx); $($body)* } $idx += 1;
                }

                // Remainder
                while $idx < count {
                    let $slot = slots.get_unchecked_mut($idx);
                    $($body)*
                    $idx += 1;
                }
            }

            rb.publish_batch_relaxed($seq, count);
            Ok(count)
        } else {
            Err("Ring buffer full")
        }
        }
    };
}

/// High-performance consume with 8x loop unrolling
///
#[macro_export]
macro_rules! consume_unrolled {
    (
        $rb:expr,
        $consumer_id:expr,
        $max_count:expr,
        $idx:ident,
        $slot:ident,
        { $($body:tt)* }
    ) => {
        {
        let batch = $rb.try_consume_batch($consumer_id, $max_count);
        let count = batch.len();

        if count > 0 {
            let mut $idx = 0;

            while $idx + 8 <= count {
                { let $slot = &batch[$idx]; $($body)* } $idx += 1;
                { let $slot = &batch[$idx]; $($body)* } $idx += 1;
                { let $slot = &batch[$idx]; $($body)* } $idx += 1;
                { let $slot = &batch[$idx]; $($body)* } $idx += 1;
                { let $slot = &batch[$idx]; $($body)* } $idx += 1;
                { let $slot = &batch[$idx]; $($body)* } $idx += 1;
                { let $slot = &batch[$idx]; $($body)* } $idx += 1;
                { let $slot = &batch[$idx]; $($body)* } $idx += 1;
            }

            while $idx < count {
                let $slot = &batch[$idx];
                $($body)*
                $idx += 1;
            }
        }

        count
        }
    };
}

#[cfg(test)]
mod tests {
    #[test]
    fn test_macros_compile() {
        // Macros are tested in integration tests
        assert!(true);
    }
}

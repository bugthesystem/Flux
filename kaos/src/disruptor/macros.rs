//! Macros for Kaos ring buffers.
//!
//! ## Macros
//!
//! | Macro | Use Case |
//! |-------|----------|
//! | `publish_batch!` | Slice-based batch publish |
//! | `consume_batch!` | Slice-based batch consume |
//! | `publish_unrolled!` | 8x unrolled batch publish |

/// Publish using slice API - Direct slice access.
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

/// Consume using slice API - Direct slice access.
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

/// Publish with 8x loop unrolling for MessageRingBuffer
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

                // Unroll by 8
                while $idx + 8 <= count {
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

            rb.publish_batch_relaxed($seq, $seq + count as u64 - 1);
            Ok(count)
        } else {
            Err("Ring buffer full")
        }
        }
    };
}

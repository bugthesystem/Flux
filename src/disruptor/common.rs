use std::sync::atomic::AtomicU64;

/// Cache-line padded producer sequence to prevent false sharing.
///
/// False sharing is a performance issue that can occur when multiple threads
/// access different variables that happen to be on the same cache line. By padding
/// the sequence to the size of a cache line (typically 64 or 128 bytes), we ensure
/// that the producer's sequence number is isolated from other data, preventing
/// unnecessary cache invalidations and improving performance in multi-threaded scenarios.
#[repr(align(128))]
pub struct PaddedProducerSequence {
    pub sequence: AtomicU64,
    _padding: [u8; 120], // 128 - 8 bytes for AtomicU64
}

impl PaddedProducerSequence {
    pub fn new(initial: u64) -> Self {
        Self {
            sequence: AtomicU64::new(initial),
            _padding: [0; 120],
        }
    }
}

/// Cache-line padded consumer sequence to prevent false sharing.
///
/// Similar to `PaddedProducerSequence`, this struct pads the consumer's sequence
/// number to a full cache line. This is crucial in multi-consumer scenarios to
/// prevent one consumer's progress from interfering with another's, as each
/// consumer's sequence can be updated independently without causing cache-line
/// contention.
#[repr(align(128))]
pub struct PaddedConsumerSequence {
    pub sequence: AtomicU64,
    _padding: [u8; 120], // 128 - 8 bytes for AtomicU64
}

impl PaddedConsumerSequence {
    pub fn new(initial: u64) -> Self {
        Self {
            sequence: AtomicU64::new(initial),
            _padding: [0; 120],
        }
    }
}

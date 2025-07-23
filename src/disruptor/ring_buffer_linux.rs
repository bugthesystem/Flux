use std::sync::atomic::{ AtomicU64, Ordering };
use std::ptr;
use crate::disruptor::{ MessageSlot, RingBufferConfig };
use crate::utils::LinuxNumaOptimizer;
use crate::utils::memory::{ linux_lock_memory, linux_allocate_huge_pages };
use crate::utils::cpu::{ linux_pin_to_cpu, linux_set_max_priority };

/// Linux-optimized ring buffer with NUMA awareness and huge pages
pub struct LinuxRingBuffer {
    /// Memory-mapped buffer
    buffer: *mut MessageSlot,
    /// Buffer size in slots
    size: usize,
    /// Mask for efficient modulo operations
    mask: usize,
    /// Producer sequence (cache-line padded)
    producer_sequence: PaddedProducerSequence,
    /// Consumer sequences (cache-line padded)
    consumer_sequences: Vec<PaddedConsumerSequence>,
    /// NUMA optimizer
    numa_optimizer: Option<LinuxNumaOptimizer>,
    /// Configuration
    config: RingBufferConfig,
}

/// Cache-line padded producer sequence
#[repr(align(128))]
struct PaddedProducerSequence {
    sequence: AtomicU64,
    _padding: [u8; 120], // 128 - 8 bytes for AtomicU64
}

/// Cache-line padded consumer sequence
#[repr(align(128))]
struct PaddedConsumerSequence {
    sequence: AtomicU64,
    _padding: [u8; 120], // 128 - 8 bytes for AtomicU64
}

impl PaddedProducerSequence {
    fn new(initial: u64) -> Self {
        Self {
            sequence: AtomicU64::new(initial),
            _padding: [0; 120],
        }
    }
}

impl PaddedConsumerSequence {
    fn new(initial: u64) -> Self {
        Self {
            sequence: AtomicU64::new(initial),
            _padding: [0; 120],
        }
    }
}

impl LinuxRingBuffer {
    /// Create a new Linux-optimized ring buffer
    pub fn new(config: RingBufferConfig) -> Result<Self, std::io::Error> {
        let size = config.size;
        let buffer_size = size * std::mem::size_of::<MessageSlot>();

        // Initialize NUMA optimizer if available
        let numa_optimizer = LinuxNumaOptimizer::new();

        // Allocate buffer with Linux optimizations
        let buffer = if config.use_huge_pages {
            // Use huge pages if requested
            linux_allocate_huge_pages(buffer_size)?
        } else if let Some(ref numa) = numa_optimizer {
            // Use NUMA-aware allocation
            unsafe {
                numa.allocate_on_node(buffer_size, 0)
            } // Allocate on node 0
        } else {
            // Fallback to regular allocation
            unsafe {
                libc::malloc(buffer_size) as *mut MessageSlot
            }
        };

        if buffer.is_null() {
            return Err(
                std::io::Error::new(std::io::ErrorKind::Other, "Failed to allocate ring buffer")
            );
        }

        // Lock memory to prevent paging
        linux_lock_memory(buffer as *mut u8, buffer_size)?;

        // Initialize consumer sequences
        let mut consumer_sequences = Vec::new();
        for i in 0..config.num_consumers {
            consumer_sequences.push(PaddedConsumerSequence::new(i as u64));
        }

        Ok(Self {
            buffer,
            size,
            mask: size - 1,
            producer_sequence: PaddedProducerSequence::new(0),
            consumer_sequences,
            numa_optimizer,
            config,
        })
    }

    /// Claim slots for writing (producer)
    pub fn try_claim_slots(&self, count: usize) -> Option<(u64, &mut [MessageSlot])> {
        if count == 0 {
            return None;
        }

        let current_seq = self.producer_sequence.sequence.load(Ordering::Relaxed);
        let next_seq = current_seq + (count as u64);

        // Check if we have space
        let min_consumer_seq = self.get_minimum_consumer_sequence();
        if next_seq > min_consumer_seq + (self.size as u64) {
            return None; // Would wrap around
        }

        // Try to claim the slots atomically
        match
            self.producer_sequence.sequence.compare_exchange_weak(
                current_seq,
                next_seq,
                Ordering::Acquire,
                Ordering::Relaxed
            )
        {
            Ok(_) => {
                // Prefetch slots for better performance
                self.prefetch_slots(current_seq as usize, count);

                // Return the claimed slots
                let start_index = (current_seq as usize) & self.mask;
                let end_index = start_index + count;

                let slots = if end_index <= self.size {
                    // Single contiguous range
                    unsafe {
                        std::slice::from_raw_parts_mut(self.buffer.add(start_index), count)
                    }
                } else {
                    // Wrapped around - need to handle in two parts
                    let first_part = self.size - start_index;
                    unsafe {
                        std::slice::from_raw_parts_mut(
                            self.buffer.add(start_index),
                            count.min(first_part)
                        )
                    }
                };

                Some((current_seq, slots))
            }
            Err(_) => None,
        }
    }

    /// Consume slots for reading (consumer)
    pub fn try_consume_batch(&self, consumer_id: usize, count: usize) -> &[MessageSlot] {
        if count == 0 {
            return &[];
        }

        let consumer_seq = &self.consumer_sequences[consumer_id];
        let current_seq = consumer_seq.sequence.load(Ordering::Relaxed);
        let producer_seq = self.producer_sequence.sequence.load(Ordering::Acquire);

        if current_seq >= producer_seq {
            return &[]; // No messages available
        }

        let available = (producer_seq - current_seq) as usize;
        let to_consume = count.min(available);

        if to_consume == 0 {
            return &[];
        }

        // Prefetch slots for better performance
        self.prefetch_slots(current_seq as usize, to_consume);

        // Update consumer sequence
        consumer_seq.sequence.store(current_seq + (to_consume as u64), Ordering::Release);

        // Return the consumed slots
        let start_index = (current_seq as usize) & self.mask;
        unsafe { std::slice::from_raw_parts(self.buffer.add(start_index), to_consume) }
    }

    /// Publish a batch of slots (called after filling them)
    pub fn publish_batch(&self, _start_seq: u64, _count: usize) {
        // Memory barrier to ensure all data is written before publishing
        std::sync::atomic::fence(Ordering::Release);
    }

    /// Get minimum consumer sequence (for gating)
    fn get_minimum_consumer_sequence(&self) -> u64 {
        self.consumer_sequences
            .iter()
            .map(|cs| cs.sequence.load(Ordering::Acquire))
            .min()
            .unwrap_or(0)
    }

    /// Prefetch slots into L1 cache for better performance
    fn prefetch_slots(&self, start_index: usize, count: usize) {
        const CACHE_PREFETCH_LINES: usize = 4;

        // Aggressively prefetch more cache lines
        for i in 0..count.min(CACHE_PREFETCH_LINES * 32) {
            let slot_index = (start_index + i) & self.mask;
            let slot_ptr = unsafe { self.buffer.add(slot_index) };

            // Use CPU prefetch instruction
            #[cfg(target_arch = "x86_64")]
            unsafe {
                std::arch::x86_64::_mm_prefetch(
                    slot_ptr as *const i8,
                    std::arch::x86_64::_MM_HINT_T0
                );
            }

            #[cfg(target_arch = "aarch64")]
            unsafe {
                std::arch::asm!(
                    "prfm pldl1keep, [{ptr}]",
                    ptr = in(reg) slot_ptr,
                    options(nostack)
                );
            }
        }
    }

    /// Get NUMA optimizer
    pub fn numa_optimizer(&self) -> Option<&LinuxNumaOptimizer> {
        self.numa_optimizer.as_ref()
    }
}

impl Drop for LinuxRingBuffer {
    fn drop(&mut self) {
        eprintln!(
            "[DEBUG] Dropping LinuxRingBuffer: buffer={:p}, size={}, numa={}",
            self.buffer,
            self.size,
            self.numa_optimizer.is_some()
        );
        if let Some(ref numa) = self.numa_optimizer {
            let buffer_size = self.size * std::mem::size_of::<MessageSlot>();
            numa.free(self.buffer);
        } else {
            unsafe {
                libc::free(self.buffer as *mut libc::c_void);
            }
        }
    }
}

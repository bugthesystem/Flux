//! SharedRingBuffer - File-backed ring buffer for inter-process communication
//!
//! Unlike `RingBuffer::new_mapped()` which uses anonymous mmap (single process),
//! `SharedRingBuffer` uses file-backed mmap that can be shared between processes.
//!
//! ## Memory Layout
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────────────┐
//! │                    Shared Memory File                           │
//! ├─────────────────────────────────────────────────────────────────┤
//! │  Header (256 bytes)                                             │
//! │  ├─ Cache line 0: magic, version, capacity, slot_size          │
//! │  ├─ Cache line 1: producer_seq (AtomicU64)                     │
//! │  ├─ Cache line 2: cached_consumer_seq                          │
//! │  └─ Cache line 3: consumer_seq (AtomicU64)                     │
//! ├─────────────────────────────────────────────────────────────────┤
//! │  Slots (T::SIZE bytes each)                                     │
//! │  [slot 0][slot 1][slot 2]...[slot N-1]                         │
//! └─────────────────────────────────────────────────────────────────┘
//! ```
//!
//! ## Usage
//!
//! ```rust,no_run
//! use flux::disruptor::SharedRingBuffer;
//! use flux::disruptor::SmallSlot;
//!
//! // Process A - Create and write
//! let mut rb = SharedRingBuffer::<SmallSlot>::create("/tmp/flux-shared", 1024)?;
//! rb.try_send(&[1, 2, 3, 4])?;
//!
//! // Process B - Open and read
//! let mut rb = SharedRingBuffer::<SmallSlot>::open("/tmp/flux-shared")?;
//! rb.receive(|data| {
//!     // Process the SmallSlot data
//!     let _ = data.value; // Access the u64 value
//! });
//! # Ok::<(), std::io::Error>(())
//! ```

use std::sync::atomic::{ AtomicU64, Ordering };
use std::path::Path;
use std::io;
use std::fs::{ File, OpenOptions };
use std::os::unix::io::AsRawFd;
use crate::disruptor::RingBufferEntry;

/// Magic number to identify flux shared ring buffer files
const MAGIC: u64 = 0x464c55585f534852; // "FLUX_SHR"

/// Format version
const VERSION: u32 = 1;

/// Header size (4 cache lines = 256 bytes)
const HEADER_SIZE: usize = 256;

/// Shared header - lives in the mmap'd region
///
/// Cache-line aligned to prevent false sharing between processes
#[repr(C, align(64))]
struct SharedHeader {
    // Cache line 0: metadata
    magic: u64,
    version: u32,
    capacity: u32,
    slot_size: u32,
    _pad0: [u8; 44],

    // Cache line 1: producer sequence
    producer_seq: AtomicU64,
    _pad1: [u8; 56],

    // Cache line 2: cached consumer (for producer's local cache)
    cached_consumer_seq: AtomicU64,
    _pad2: [u8; 56],

    // Cache line 3: consumer sequence
    consumer_seq: AtomicU64,
    _pad3: [u8; 56],
}

/// File-backed ring buffer for inter-process communication.
///
/// Uses the same LMAX Disruptor pattern as `RingBuffer` but with:
/// - File-backed mmap (shareable between processes)
/// - All metadata in shared memory (not heap)
pub struct SharedRingBuffer<T: RingBufferEntry> {
    /// Memory-mapped region
    mmap_ptr: *mut u8,
    mmap_len: usize,
    /// Capacity (number of slots)
    capacity: u64,
    /// Mask for fast index
    mask: u64,
    /// Slot size
    slot_size: usize,
    /// Local sequence tracking (not shared)
    local_seq: u64,
    /// Cached remote sequence
    cached_remote_seq: u64,
    /// Is this the producer side?
    is_producer: bool,
    /// Keep file handle alive
    _file: File,
    /// Phantom for T
    _phantom: std::marker::PhantomData<T>,
}

impl<T: RingBufferEntry> SharedRingBuffer<T> {
    /// Create a new shared ring buffer (producer side).
    ///
    /// Creates or truncates the file at `path`.
    pub fn create<P: AsRef<Path>>(path: P, capacity: usize) -> io::Result<Self> {
        if capacity == 0 || (capacity & (capacity - 1)) != 0 {
            return Err(
                io::Error::new(io::ErrorKind::InvalidInput, "Capacity must be a power of 2")
            );
        }

        let slot_size = std::mem::size_of::<T>();
        let file_size = HEADER_SIZE + capacity * slot_size;

        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(true)
            .open(&path)?;

        file.set_len(file_size as u64)?;

        let mmap_ptr = unsafe {
            let ptr = libc::mmap(
                std::ptr::null_mut(),
                file_size,
                libc::PROT_READ | libc::PROT_WRITE,
                libc::MAP_SHARED, // <-- KEY: MAP_SHARED for IPC
                file.as_raw_fd(),
                0
            );

            if ptr == libc::MAP_FAILED {
                return Err(io::Error::last_os_error());
            }

            ptr as *mut u8
        };

        // Initialize header
        let header = unsafe { &mut *(mmap_ptr as *mut SharedHeader) };
        header.magic = MAGIC;
        header.version = VERSION;
        header.capacity = capacity as u32;
        header.slot_size = slot_size as u32;
        header.producer_seq = AtomicU64::new(0);
        header.cached_consumer_seq = AtomicU64::new(0);
        header.consumer_seq = AtomicU64::new(0);

        // Zero slots
        unsafe {
            std::ptr::write_bytes(mmap_ptr.add(HEADER_SIZE), 0, capacity * slot_size);
        }

        // Sync to disk
        unsafe {
            libc::msync(mmap_ptr as *mut _, file_size, libc::MS_SYNC);
        }

        Ok(Self {
            mmap_ptr,
            mmap_len: file_size,
            capacity: capacity as u64,
            mask: (capacity - 1) as u64,
            slot_size,
            local_seq: 0,
            cached_remote_seq: 0,
            is_producer: true,
            _file: file,
            _phantom: std::marker::PhantomData,
        })
    }

    /// Open an existing shared ring buffer (consumer side).
    pub fn open<P: AsRef<Path>>(path: P) -> io::Result<Self> {
        let file = OpenOptions::new().read(true).write(true).open(&path)?;

        let metadata = file.metadata()?;
        let file_size = metadata.len() as usize;

        let mmap_ptr = unsafe {
            let ptr = libc::mmap(
                std::ptr::null_mut(),
                file_size,
                libc::PROT_READ | libc::PROT_WRITE,
                libc::MAP_SHARED,
                file.as_raw_fd(),
                0
            );

            if ptr == libc::MAP_FAILED {
                return Err(io::Error::last_os_error());
            }

            ptr as *mut u8
        };

        // Validate header
        let header = unsafe { &*(mmap_ptr as *const SharedHeader) };

        if header.magic != MAGIC {
            unsafe {
                libc::munmap(mmap_ptr as *mut _, file_size);
            }
            return Err(
                io::Error::new(
                    io::ErrorKind::InvalidData,
                    "Invalid flux shared ring buffer (wrong magic)"
                )
            );
        }

        if header.version != VERSION {
            unsafe {
                libc::munmap(mmap_ptr as *mut _, file_size);
            }
            return Err(
                io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("Version mismatch: expected {}, got {}", VERSION, header.version)
                )
            );
        }

        let capacity = header.capacity as usize;
        let slot_size = header.slot_size as usize;

        // Validate slot size matches T
        if slot_size != std::mem::size_of::<T>() {
            unsafe {
                libc::munmap(mmap_ptr as *mut _, file_size);
            }
            return Err(
                io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!(
                        "Slot size mismatch: file has {}, expected {}",
                        slot_size,
                        std::mem::size_of::<T>()
                    )
                )
            );
        }

        Ok(Self {
            mmap_ptr,
            mmap_len: file_size,
            capacity: capacity as u64,
            mask: (capacity - 1) as u64,
            slot_size,
            local_seq: 0,
            cached_remote_seq: 0,
            is_producer: false,
            _file: file,
            _phantom: std::marker::PhantomData,
        })
    }

    /// Get header reference
    #[inline]
    fn header(&self) -> &SharedHeader {
        unsafe { &*(self.mmap_ptr as *const SharedHeader) }
    }

    /// Get mutable header reference
    #[inline]
    fn header_mut(&mut self) -> &mut SharedHeader {
        unsafe { &mut *(self.mmap_ptr as *mut SharedHeader) }
    }

    /// Get slot pointer
    #[inline]
    fn slot_ptr(&self, seq: u64) -> *mut T {
        let index = (seq & self.mask) as usize;
        let offset = HEADER_SIZE + index * self.slot_size;
        unsafe { self.mmap_ptr.add(offset) as *mut T }
    }

    // ========================================================================
    // Producer API
    // ========================================================================

    /// Try to claim a slot for writing (non-blocking).
    #[inline]
    pub fn try_claim(&mut self) -> Option<u64> {
        debug_assert!(self.is_producer, "try_claim() is for producer only");

        // Check space using cached consumer
        if self.local_seq.wrapping_sub(self.cached_remote_seq) >= self.capacity {
            // Refresh cache
            self.cached_remote_seq = self.header().consumer_seq.load(Ordering::Acquire);

            if self.local_seq.wrapping_sub(self.cached_remote_seq) >= self.capacity {
                return None;
            }
        }

        let seq = self.local_seq;
        self.local_seq = self.local_seq.wrapping_add(1);
        Some(seq)
    }

    /// Write to a claimed slot.
    ///
    /// # Safety
    /// Caller must have claimed this sequence via `try_claim()`.
    #[inline]
    pub unsafe fn write_slot(&mut self, seq: u64, value: T) {
        let ptr = self.slot_ptr(seq);
        std::ptr::write_volatile(ptr, value);
    }

    /// Publish written slots up to `seq` (inclusive).
    #[inline]
    pub fn publish(&mut self, seq: u64) {
        std::sync::atomic::fence(Ordering::Release);
        self.header_mut().producer_seq.store(seq.wrapping_add(1), Ordering::Release);
    }

    /// Convenience: claim, write, publish a single slot.
    #[inline]
    pub fn try_send(&mut self, data: &[u8]) -> io::Result<u64> {
        let seq = self
            .try_claim()
            .ok_or_else(|| { io::Error::new(io::ErrorKind::WouldBlock, "Ring buffer full") })?;

        // For simplicity, copy into a default slot
        let mut slot = T::default();
        let slot_bytes = unsafe {
            std::slice::from_raw_parts_mut(&mut slot as *mut T as *mut u8, std::mem::size_of::<T>())
        };

        let copy_len = data.len().min(slot_bytes.len());
        slot_bytes[..copy_len].copy_from_slice(&data[..copy_len]);

        unsafe {
            self.write_slot(seq, slot);
        }
        self.publish(seq);

        Ok(seq)
    }

    // ========================================================================
    // Consumer API
    // ========================================================================

    /// Get available count for reading.
    #[inline]
    pub fn available(&mut self) -> u64 {
        debug_assert!(!self.is_producer, "available() is for consumer only");

        self.cached_remote_seq = self.header().producer_seq.load(Ordering::Acquire);
        self.cached_remote_seq.saturating_sub(self.local_seq)
    }

    /// Process available slots with callback.
    #[inline]
    pub fn receive<F: FnMut(&T)>(&mut self, mut callback: F) -> usize {
        debug_assert!(!self.is_producer, "receive() is for consumer only");

        let producer_seq = self.header().producer_seq.load(Ordering::Acquire);
        let mut count = 0;

        while self.local_seq < producer_seq {
            let ptr = self.slot_ptr(self.local_seq);
            let slot = unsafe { &*ptr };

            callback(slot);

            self.local_seq = self.local_seq.wrapping_add(1);
            count += 1;
        }

        if count > 0 {
            let seq = self.local_seq;
            self.header_mut().consumer_seq.store(seq, Ordering::Release);
        }

        count
    }

    /// Read a single slot (low-level).
    ///
    /// # Safety
    /// Caller must ensure slot has been published.
    #[inline]
    pub unsafe fn read_slot(&self, seq: u64) -> T {
        let ptr = self.slot_ptr(seq);
        std::ptr::read_volatile(ptr)
    }

    /// Advance consumer cursor after reading.
    #[inline]
    pub fn advance_consumer(&mut self, seq: u64) {
        self.local_seq = seq.wrapping_add(1);
        let next = self.local_seq;
        self.header_mut().consumer_seq.store(next, Ordering::Release);
    }
}

impl<T: RingBufferEntry> Drop for SharedRingBuffer<T> {
    fn drop(&mut self) {
        unsafe {
            libc::munmap(self.mmap_ptr as *mut _, self.mmap_len);
        }
    }
}

// Safety: SharedRingBuffer can be sent between threads
// (but not shared - each side should have its own instance)
unsafe impl<T: RingBufferEntry> Send for SharedRingBuffer<T> {}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::disruptor::SmallSlot;
    use std::fs;

    #[test]
    fn test_create_open() {
        let path = "/tmp/flux-shared-test-create";
        let _ = fs::remove_file(path);

        // Create producer side
        let mut producer = SharedRingBuffer::<SmallSlot>::create(path, 1024).unwrap();

        // Open consumer side
        let mut consumer = SharedRingBuffer::<SmallSlot>::open(path).unwrap();

        // Send and receive
        producer.try_send(&(42u64).to_le_bytes()).unwrap();

        let mut received = false;
        consumer.receive(|slot| {
            assert_eq!(slot.value, 42);
            received = true;
        });

        assert!(received);

        let _ = fs::remove_file(path);
    }

    #[test]
    fn test_batch() {
        let path = "/tmp/flux-shared-test-batch";
        let _ = fs::remove_file(path);

        let mut producer = SharedRingBuffer::<SmallSlot>::create(path, 1024).unwrap();
        let mut consumer = SharedRingBuffer::<SmallSlot>::open(path).unwrap();

        // Send 100 messages
        for i in 0u64..100 {
            producer.try_send(&i.to_le_bytes()).unwrap();
        }

        // Receive all
        let mut sum: u64 = 0;
        let count = consumer.receive(|slot| {
            sum += slot.value;
        });

        assert_eq!(count, 100);
        assert_eq!(sum, (0u64..100).sum());

        let _ = fs::remove_file(path);
    }
}

//! SharedRingBuffer - File-backed ring buffer for inter-process communication
//!
//! Uses file-backed mmap (MAP_SHARED) that can be shared between processes.

use crate::disruptor::RingBufferEntry;
use std::fs::{File, OpenOptions};
use std::io;
use std::os::unix::io::AsRawFd;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};

/// Magic bytes "KAOS_SHR" for file format validation
const MAGIC: u64 = 0x4b414f535f534852;
/// File format version (increment on breaking changes)
const VERSION: u32 = 1;
/// Header size in bytes (256 = room for future fields + cache alignment)
const HEADER_SIZE: usize = 256;

#[repr(C, align(64))]
struct SharedHeader {
    magic: u64,
    version: u32,
    capacity: u32,
    slot_size: u32,
    _pad0: [u8; 44],
    producer_seq: AtomicU64,
    _pad1: [u8; 56],
    cached_consumer_seq: AtomicU64,
    _pad2: [u8; 56],
    consumer_seq: AtomicU64,
    _pad3: [u8; 56],
}

pub struct SharedRingBuffer<T: RingBufferEntry> {
    mmap_ptr: *mut u8,
    mmap_len: usize,
    capacity: u64,
    mask: u64,
    slot_size: usize,
    local_seq: u64,
    cached_remote_seq: u64,
    is_producer: bool,
    _file: File,
    _phantom: std::marker::PhantomData<T>,
}

impl<T: RingBufferEntry> SharedRingBuffer<T> {
    pub fn create<P: AsRef<Path>>(path: P, capacity: usize) -> io::Result<Self> {
        if capacity == 0 || (capacity & (capacity - 1)) != 0 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "Capacity must be a power of 2",
            ));
        }

        let slot_size = std::mem::size_of::<T>();
        let file_size = HEADER_SIZE
            .checked_add(
                capacity
                    .checked_mul(slot_size)
                    .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "Size overflow"))?,
            )
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "Size overflow"))?;

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
                libc::MAP_SHARED,
                file.as_raw_fd(),
                0,
            );
            if ptr == libc::MAP_FAILED {
                return Err(io::Error::last_os_error());
            }
            ptr as *mut u8
        };

        let header = unsafe { &mut *(mmap_ptr as *mut SharedHeader) };
        header.magic = MAGIC;
        header.version = VERSION;
        header.capacity = capacity as u32;
        header.slot_size = slot_size as u32;
        header.producer_seq = AtomicU64::new(0);
        header.cached_consumer_seq = AtomicU64::new(0);
        header.consumer_seq = AtomicU64::new(0);

        unsafe {
            std::ptr::write_bytes(mmap_ptr.add(HEADER_SIZE), 0, capacity * slot_size);
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

    pub fn open<P: AsRef<Path>>(path: P) -> io::Result<Self> {
        let file = OpenOptions::new().read(true).write(true).open(&path)?;
        let file_size = file.metadata()?.len() as usize;

        // Bounds check: file must be large enough for header
        if file_size < HEADER_SIZE {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "File too small for header",
            ));
        }

        let mmap_ptr = unsafe {
            let ptr = libc::mmap(
                std::ptr::null_mut(),
                file_size,
                libc::PROT_READ | libc::PROT_WRITE,
                libc::MAP_SHARED,
                file.as_raw_fd(),
                0,
            );
            if ptr == libc::MAP_FAILED {
                return Err(io::Error::last_os_error());
            }
            ptr as *mut u8
        };

        let header = unsafe { &*(mmap_ptr as *const SharedHeader) };

        if header.magic != MAGIC {
            unsafe {
                libc::munmap(mmap_ptr as *mut _, file_size);
            }
            return Err(io::Error::new(io::ErrorKind::InvalidData, "Invalid magic"));
        }
        if header.version != VERSION {
            unsafe {
                libc::munmap(mmap_ptr as *mut _, file_size);
            }
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!(
                    "Version mismatch: expected {}, got {}",
                    VERSION, header.version
                ),
            ));
        }

        let capacity = header.capacity as usize;
        let slot_size = header.slot_size as usize;

        if slot_size != std::mem::size_of::<T>() {
            unsafe {
                libc::munmap(mmap_ptr as *mut _, file_size);
            }
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!(
                    "Slot size mismatch: file has {}, expected {}",
                    slot_size,
                    std::mem::size_of::<T>()
                ),
            ));
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

    fn header(&self) -> &SharedHeader {
        unsafe { &*(self.mmap_ptr as *const SharedHeader) }
    }
    fn header_mut(&mut self) -> &mut SharedHeader {
        unsafe { &mut *(self.mmap_ptr as *mut SharedHeader) }
    }
    fn slot_ptr(&self, seq: u64) -> *mut T {
        let idx = (seq & self.mask) as usize;
        debug_assert!(
            idx < (self.capacity as usize),
            "slot_ptr: idx {} >= capacity {}",
            idx,
            self.capacity
        );
        let offset = HEADER_SIZE + idx * self.slot_size;
        unsafe { self.mmap_ptr.add(offset) as *mut T }
    }

    pub fn try_claim(&mut self) -> Option<u64> {
        debug_assert!(self.is_producer, "try_claim() is for producer only");
        if self.local_seq.wrapping_sub(self.cached_remote_seq) >= self.capacity {
            self.cached_remote_seq = self.header().consumer_seq.load(Ordering::Acquire);
            if self.local_seq.wrapping_sub(self.cached_remote_seq) >= self.capacity {
                return None;
            }
        }
        let seq = self.local_seq;
        self.local_seq = self.local_seq.wrapping_add(1);
        Some(seq)
    }

    /// Write a value to a slot in shared memory.
    ///
    /// # Safety
    ///
    /// - The sequence must have been claimed via `try_claim`.
    /// - Only the producer process should call this method.
    /// - The slot must be published after writing via `publish()`.
    #[cfg(feature = "unsafe-perf")]
    #[inline(always)]
    pub unsafe fn write_slot(&mut self, seq: u64, value: T) {
        std::ptr::write_volatile(self.slot_ptr(seq), value);
    }

    /// Write a value to a slot in shared memory.
    ///
    /// # Safety
    ///
    /// - The sequence must have been claimed via `try_claim`.
    /// - Only the producer process should call this method.
    /// - The slot must be published after writing via `publish()`.
    #[cfg(not(feature = "unsafe-perf"))]
    #[inline(always)]
    pub unsafe fn write_slot(&mut self, seq: u64, value: T) {
        let idx = (seq & self.mask) as usize;
        debug_assert!(
            idx < (self.capacity as usize),
            "SharedRingBuffer::write_slot: idx {} >= capacity {}",
            idx,
            self.capacity
        );
        std::ptr::write_volatile(self.slot_ptr(seq), value);
    }

    pub fn publish(&mut self, seq: u64) {
        std::sync::atomic::fence(Ordering::Release);
        self.header_mut()
            .producer_seq
            .store(seq.wrapping_add(1), Ordering::Release);
    }

    pub fn try_send(&mut self, data: &[u8]) -> io::Result<u64> {
        let seq = self
            .try_claim()
            .ok_or_else(|| io::Error::new(io::ErrorKind::WouldBlock, "Ring buffer full"))?;
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

    pub fn available(&mut self) -> u64 {
        debug_assert!(!self.is_producer, "available() is for consumer only");
        self.cached_remote_seq = self.header().producer_seq.load(Ordering::Acquire);
        self.cached_remote_seq.saturating_sub(self.local_seq)
    }

    /// Try to receive a single message (non-blocking, for polling)
    pub fn try_receive(&mut self) -> Option<T> {
        debug_assert!(!self.is_producer, "try_receive() is for consumer only");
        let producer_seq = self.header().producer_seq.load(Ordering::Acquire);
        if self.local_seq >= producer_seq {
            return None;
        }

        let slot = unsafe { std::ptr::read_volatile(self.slot_ptr(self.local_seq)) };
        self.local_seq = self.local_seq.wrapping_add(1);
        let seq = self.local_seq;
        self.header_mut().consumer_seq.store(seq, Ordering::Release);
        Some(slot)
    }

    /// Receive with callback (batch, more efficient for high throughput)
    pub fn receive<F: FnMut(&T)>(&mut self, mut callback: F) -> usize {
        debug_assert!(!self.is_producer, "receive() is for consumer only");
        let producer_seq = self.header().producer_seq.load(Ordering::Acquire);
        let mut count = 0;
        while self.local_seq < producer_seq {
            let slot = unsafe { &*self.slot_ptr(self.local_seq) };
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

    /// Read a value from a slot in shared memory.
    ///
    /// # Safety
    ///
    /// - The sequence must have been published by the producer.
    /// - Only the consumer process should call this method.
    /// - Call `advance_consumer()` after processing to update the consumer cursor.
    #[cfg(feature = "unsafe-perf")]
    #[inline(always)]
    pub unsafe fn read_slot(&self, seq: u64) -> T {
        std::ptr::read_volatile(self.slot_ptr(seq))
    }

    /// Read a value from a slot in shared memory.
    ///
    /// # Safety
    ///
    /// - The sequence must have been published by the producer.
    /// - Only the consumer process should call this method.
    /// - Call `advance_consumer()` after processing to update the consumer cursor.
    #[cfg(not(feature = "unsafe-perf"))]
    #[inline(always)]
    pub unsafe fn read_slot(&self, seq: u64) -> T {
        let idx = (seq & self.mask) as usize;
        debug_assert!(
            idx < (self.capacity as usize),
            "SharedRingBuffer::read_slot: idx {} >= capacity {}",
            idx,
            self.capacity
        );
        std::ptr::read_volatile(self.slot_ptr(seq))
    }

    pub fn advance_consumer(&mut self, seq: u64) {
        self.local_seq = seq.wrapping_add(1);
        let next = self.local_seq;
        self.header_mut()
            .consumer_seq
            .store(next, Ordering::Release);
    }
}

impl<T: RingBufferEntry> Drop for SharedRingBuffer<T> {
    fn drop(&mut self) {
        unsafe {
            libc::munmap(self.mmap_ptr as *mut _, self.mmap_len);
        }
    }
}

unsafe impl<T: RingBufferEntry> Send for SharedRingBuffer<T> {}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::disruptor::Slot8;
    use std::fs;

    #[test]
    fn test_create_open() {
        let path = "/tmp/kaos-shared-test-create";
        let _ = fs::remove_file(path);

        let mut producer = SharedRingBuffer::<Slot8>::create(path, 1024).unwrap();
        let mut consumer = SharedRingBuffer::<Slot8>::open(path).unwrap();

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
        let path = "/tmp/kaos-shared-test-batch";
        let _ = fs::remove_file(path);

        let mut producer = SharedRingBuffer::<Slot8>::create(path, 1024).unwrap();
        let mut consumer = SharedRingBuffer::<Slot8>::open(path).unwrap();

        for i in 0u64..100 {
            producer.try_send(&i.to_le_bytes()).unwrap();
        }

        let mut sum: u64 = 0;
        let count = consumer.receive(|slot| {
            sum += slot.value;
        });
        assert_eq!(count, 100);
        assert_eq!(sum, (0u64..100).sum::<u64>());
        let _ = fs::remove_file(path);
    }
}

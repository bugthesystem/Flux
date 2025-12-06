//! High-performance message archive using memory-mapped files.
//!
//! ```rust,no_run
//! use kaos_archive::Archive;
//!
//! let mut archive = Archive::create("/tmp/messages", 1024 * 1024).unwrap();
//! let seq = archive.append(b"hello world").unwrap();
//! let msg = archive.read(seq).unwrap();
//! ```

use memmap2::{MmapMut, MmapOptions};
use std::fs::{File, OpenOptions};
use std::io;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};

/// Log file header (64 bytes, cache-line aligned)
#[repr(C, align(64))]
struct LogHeader {
    magic: u64,           // "KAOSLOG\0"
    version: u32,
    _reserved: u32,
    write_pos: AtomicU64, // Current write position
    msg_count: AtomicU64, // Total messages
    _pad: [u8; 32],
}

const MAGIC: u64 = 0x004b414f534c4f47; // "KAOSLOG\0"
const HEADER_SIZE: usize = 64;

/// Index entry (16 bytes)
#[repr(C)]
#[derive(Clone, Copy, Default)]
struct IndexEntry {
    offset: u64, // Offset in log file
    length: u32, // Message length
    _pad: u32,
}

/// Message frame header (8 bytes)
#[repr(C)]
struct FrameHeader {
    length: u32,   // Payload length
    checksum: u32, // CRC32 of payload
}

const FRAME_HEADER_SIZE: usize = 8;

/// Memory-mapped message archive.
pub struct Archive {
    log_mmap: MmapMut,
    index_mmap: MmapMut,
    _log_file: File,
    _index_file: File,
    capacity: usize,
}

#[derive(Debug, thiserror::Error)]
pub enum ArchiveError {
    #[error("io: {0}")]
    Io(#[from] io::Error),
    #[error("archive full")]
    Full,
    #[error("invalid sequence: {0}")]
    InvalidSequence(u64),
    #[error("corrupted: checksum mismatch")]
    Corrupted,
    #[error("invalid magic")]
    InvalidMagic,
}

impl Archive {
    /// Create a new archive with given capacity.
    pub fn create<P: AsRef<Path>>(base_path: P, capacity: usize) -> Result<Self, ArchiveError> {
        let base = base_path.as_ref();
        let log_path = base.with_extension("log");
        let index_path = base.with_extension("idx");

        // Create log file
        let log_file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(true)
            .open(&log_path)?;
        log_file.set_len(capacity as u64)?;

        // Create index file (16 bytes per entry, estimate 64-byte avg message)
        let index_capacity = (capacity / 64) * std::mem::size_of::<IndexEntry>();
        let index_file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(true)
            .open(&index_path)?;
        index_file.set_len(index_capacity as u64)?;

        // Memory map
        let mut log_mmap = unsafe { MmapOptions::new().map_mut(&log_file)? };
        let index_mmap = unsafe { MmapOptions::new().map_mut(&index_file)? };

        // Initialize header
        let header = unsafe { &mut *(log_mmap.as_mut_ptr() as *mut LogHeader) };
        header.magic = MAGIC;
        header.version = 1;
        header.write_pos.store(HEADER_SIZE as u64, Ordering::Release);
        header.msg_count.store(0, Ordering::Release);

        Ok(Self {
            log_mmap,
            index_mmap,
            _log_file: log_file,
            _index_file: index_file,
            capacity,
        })
    }

    /// Open an existing archive.
    pub fn open<P: AsRef<Path>>(base_path: P) -> Result<Self, ArchiveError> {
        let base = base_path.as_ref();
        let log_path = base.with_extension("log");
        let index_path = base.with_extension("idx");

        let log_file = OpenOptions::new().read(true).write(true).open(&log_path)?;
        let index_file = OpenOptions::new().read(true).write(true).open(&index_path)?;

        let capacity = log_file.metadata()?.len() as usize;
        let log_mmap = unsafe { MmapOptions::new().map_mut(&log_file)? };
        let index_mmap = unsafe { MmapOptions::new().map_mut(&index_file)? };

        // Verify magic
        let header = unsafe { &*(log_mmap.as_ptr() as *const LogHeader) };
        if header.magic != MAGIC {
            return Err(ArchiveError::InvalidMagic);
        }

        Ok(Self {
            log_mmap,
            index_mmap,
            _log_file: log_file,
            _index_file: index_file,
            capacity,
        })
    }

    /// Append a message. Returns sequence number.
    pub fn append(&mut self, data: &[u8]) -> Result<u64, ArchiveError> {
        // Read current state
        let header_ptr = self.log_mmap.as_ptr() as *const LogHeader;
        let seq = unsafe { (*header_ptr).msg_count.load(Ordering::Acquire) };
        let pos = unsafe { (*header_ptr).write_pos.load(Ordering::Acquire) } as usize;

        let frame_size = FRAME_HEADER_SIZE + data.len();
        let new_pos = pos + frame_size;

        if new_pos > self.capacity {
            return Err(ArchiveError::Full);
        }

        // Write frame header
        let frame = FrameHeader {
            length: data.len() as u32,
            checksum: crc32(data),
        };
        let frame_bytes = unsafe {
            std::slice::from_raw_parts(&frame as *const FrameHeader as *const u8, FRAME_HEADER_SIZE)
        };
        self.log_mmap[pos..pos + FRAME_HEADER_SIZE].copy_from_slice(frame_bytes);

        // Write payload
        self.log_mmap[pos + FRAME_HEADER_SIZE..new_pos].copy_from_slice(data);

        // Update index
        let index_entry = IndexEntry {
            offset: pos as u64,
            length: data.len() as u32,
            _pad: 0,
        };
        let idx_pos = seq as usize * std::mem::size_of::<IndexEntry>();
        let entry_bytes = unsafe {
            std::slice::from_raw_parts(
                &index_entry as *const IndexEntry as *const u8,
                std::mem::size_of::<IndexEntry>(),
            )
        };
        let idx_len = self.index_mmap.len();
        if idx_pos + entry_bytes.len() <= idx_len {
            self.index_mmap[idx_pos..idx_pos + entry_bytes.len()].copy_from_slice(entry_bytes);
        }

        // Update header atomically
        let header_ptr = self.log_mmap.as_mut_ptr() as *mut LogHeader;
        unsafe {
            (*header_ptr).write_pos.store(new_pos as u64, Ordering::Release);
            (*header_ptr).msg_count.store(seq + 1, Ordering::Release);
        }

        Ok(seq)
    }

    /// Read a message by sequence number. Zero-copy.
    pub fn read(&self, seq: u64) -> Result<&[u8], ArchiveError> {
        let header = self.header();
        let count = header.msg_count.load(Ordering::Acquire);

        if seq >= count {
            return Err(ArchiveError::InvalidSequence(seq));
        }

        let idx_pos = seq as usize * std::mem::size_of::<IndexEntry>();
        let entry = unsafe { &*(self.index_mmap.as_ptr().add(idx_pos) as *const IndexEntry) };

        let offset = entry.offset as usize;
        let length = entry.length as usize;

        // Verify checksum
        let frame = unsafe { &*(self.log_mmap.as_ptr().add(offset) as *const FrameHeader) };
        let data = &self.log_mmap[offset + FRAME_HEADER_SIZE..offset + FRAME_HEADER_SIZE + length];

        if crc32(data) != frame.checksum {
            return Err(ArchiveError::Corrupted);
        }

        Ok(data)
    }

    /// Read without checksum verification (faster).
    pub fn read_unchecked(&self, seq: u64) -> Result<&[u8], ArchiveError> {
        let header = self.header();
        let count = header.msg_count.load(Ordering::Acquire);

        if seq >= count {
            return Err(ArchiveError::InvalidSequence(seq));
        }

        let idx_pos = seq as usize * std::mem::size_of::<IndexEntry>();
        let entry = unsafe { &*(self.index_mmap.as_ptr().add(idx_pos) as *const IndexEntry) };

        let offset = entry.offset as usize;
        let length = entry.length as usize;

        Ok(&self.log_mmap[offset + FRAME_HEADER_SIZE..offset + FRAME_HEADER_SIZE + length])
    }

    /// Replay messages in range.
    pub fn replay<F>(&self, from: u64, to: u64, mut handler: F) -> Result<u64, ArchiveError>
    where
        F: FnMut(u64, &[u8]),
    {
        let mut count = 0;
        for seq in from..to {
            match self.read_unchecked(seq) {
                Ok(data) => {
                    handler(seq, data);
                    count += 1;
                }
                Err(ArchiveError::InvalidSequence(_)) => break,
                Err(e) => return Err(e),
            }
        }
        Ok(count)
    }

    /// Number of messages.
    pub fn len(&self) -> u64 {
        self.header().msg_count.load(Ordering::Acquire)
    }

    /// Is archive empty?
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Bytes used.
    pub fn bytes_used(&self) -> u64 {
        self.header().write_pos.load(Ordering::Acquire)
    }

    /// Flush to disk.
    pub fn flush(&self) -> Result<(), ArchiveError> {
        self.log_mmap.flush()?;
        self.index_mmap.flush()?;
        Ok(())
    }

    fn header(&self) -> &LogHeader {
        unsafe { &*(self.log_mmap.as_ptr() as *const LogHeader) }
    }
}

/// Simple CRC32 (same as kaos crate).
fn crc32(data: &[u8]) -> u32 {
    let mut crc: u32 = 0xFFFFFFFF;
    for &byte in data {
        crc ^= byte as u32;
        for _ in 0..8 {
            crc = if crc & 1 != 0 {
                (crc >> 1) ^ 0xEDB88320
            } else {
                crc >> 1
            };
        }
    }
    !crc
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_create_append_read() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("test");

        let mut archive = Archive::create(&path, 1024 * 1024).unwrap();

        let seq0 = archive.append(b"hello").unwrap();
        let seq1 = archive.append(b"world").unwrap();

        assert_eq!(seq0, 0);
        assert_eq!(seq1, 1);
        assert_eq!(archive.len(), 2);

        assert_eq!(archive.read(0).unwrap(), b"hello");
        assert_eq!(archive.read(1).unwrap(), b"world");
    }

    #[test]
    fn test_reopen() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("test");

        {
            let mut archive = Archive::create(&path, 1024 * 1024).unwrap();
            archive.append(b"persistent").unwrap();
            archive.flush().unwrap();
        }

        let archive = Archive::open(&path).unwrap();
        assert_eq!(archive.len(), 1);
        assert_eq!(archive.read(0).unwrap(), b"persistent");
    }

    #[test]
    fn test_replay() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("test");

        let mut archive = Archive::create(&path, 1024 * 1024).unwrap();
        for i in 0..100u64 {
            archive.append(&i.to_le_bytes()).unwrap();
        }

        let mut replayed = Vec::new();
        archive
            .replay(10, 20, |seq, data| {
                replayed.push((seq, data.to_vec()));
            })
            .unwrap();

        assert_eq!(replayed.len(), 10);
        assert_eq!(replayed[0].0, 10);
        assert_eq!(replayed[9].0, 19);
    }
}


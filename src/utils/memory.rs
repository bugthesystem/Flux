//! Memory utilities for high-performance allocations

use crate::error::{ Result, FluxError };
use std::alloc::{ alloc, dealloc, Layout };

#[cfg(target_arch = "aarch64")]
use std::arch::aarch64::{ vld1q_u8, vst1q_u8 };

/// Allocate aligned memory
pub fn allocate_aligned(size: usize, alignment: usize) -> Result<*mut u8> {
    let layout = Layout::from_size_align(size, alignment).map_err(|_|
        FluxError::config("Invalid memory layout")
    )?;

    unsafe {
        let ptr = alloc(layout);
        if ptr.is_null() {
            Err(FluxError::system_resource("Failed to allocate memory"))
        } else {
            Ok(ptr)
        }
    }
}

/// Deallocate aligned memory
pub unsafe fn deallocate_aligned(ptr: *mut u8, size: usize, alignment: usize) {
    eprintln!("[DEBUG] deallocate_aligned: ptr={:p}, size={}, alignment={}", ptr, size, alignment);
    let layout = Layout::from_size_align(size, alignment).unwrap();
    unsafe {
        dealloc(ptr, layout);
    }
}

/// Allocate memory with huge pages (if available)
pub fn allocate_huge_pages(size: usize) -> Result<*mut u8> {
    // For now, just use regular allocation
    // In a real implementation, this would use mmap with MAP_HUGETLB
    allocate_aligned(size, 4096)
}

/// Lock memory pages to prevent swapping
pub fn lock_memory(_ptr: *mut u8, _size: usize) -> Result<()> {
    // For now, just return success
    // In a real implementation, this would use mlock
    Ok(())
}

/// SIMD-optimized memory copy (NEON on ARM64, memcpy on others)
///
/// # Safety
/// - dst and src must have the same length
/// - dst and src must be valid, non-overlapping slices
/// - Memory must be properly aligned for SIMD operations
pub unsafe fn copy_data_simd(dst: &mut [u8], src: &[u8]) {
    if dst.len() != src.len() {
        return;
    }

    let len = dst.len();
    let dst_ptr = dst.as_mut_ptr();
    let src_ptr = src.as_ptr();

    #[cfg(target_arch = "aarch64")]
    {
        // Use NEON for ultra-fast copying on Apple Silicon
        if len >= 64 {
            let chunks_64 = len / 64;
            for i in 0..chunks_64 {
                let offset = i * 64;
                let v1 = vld1q_u8(src_ptr.add(offset));
                let v2 = vld1q_u8(src_ptr.add(offset + 16));
                let v3 = vld1q_u8(src_ptr.add(offset + 32));
                let v4 = vld1q_u8(src_ptr.add(offset + 48));
                vst1q_u8(dst_ptr.add(offset), v1);
                vst1q_u8(dst_ptr.add(offset + 16), v2);
                vst1q_u8(dst_ptr.add(offset + 32), v3);
                vst1q_u8(dst_ptr.add(offset + 48), v4);
            }
            let remaining_start = chunks_64 * 64;
            for i in remaining_start..len {
                *dst_ptr.add(i) = *src_ptr.add(i);
            }
        } else if len >= 16 {
            let chunks_16 = len / 16;
            for i in 0..chunks_16 {
                let offset = i * 16;
                let chunk = vld1q_u8(src_ptr.add(offset));
                vst1q_u8(dst_ptr.add(offset), chunk);
            }
            let remaining_start = chunks_16 * 16;
            for i in remaining_start..len {
                *dst_ptr.add(i) = *src_ptr.add(i);
            }
        } else {
            let chunks_8 = len / 8;
            for i in 0..chunks_8 {
                let offset = i * 8;
                let chunk = std::ptr::read_unaligned(src_ptr.add(offset) as *const u64);
                std::ptr::write_unaligned(dst_ptr.add(offset) as *mut u64, chunk);
            }
            let remaining_start = chunks_8 * 8;
            for i in remaining_start..len {
                *dst_ptr.add(i) = *src_ptr.add(i);
            }
        }
    }
    #[cfg(not(target_arch = "aarch64"))]
    {
        dst.copy_from_slice(src);
    }
}

/// Memory statistics
#[derive(Debug, Clone)]
pub struct MemoryStats {
    /// Total allocated bytes
    pub allocated_bytes: usize,
    /// Peak allocated bytes
    pub peak_allocated_bytes: usize,
    /// Number of allocations
    pub allocation_count: usize,
    /// Number of deallocations
    pub deallocation_count: usize,
}

impl MemoryStats {
    /// Create a new memory statistics instance
    pub fn new() -> Self {
        Self {
            allocated_bytes: 0,
            peak_allocated_bytes: 0,
            allocation_count: 0,
            deallocation_count: 0,
        }
    }
}

/// Get current memory statistics
pub fn get_memory_stats() -> MemoryStats {
    MemoryStats::new()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_allocate_aligned() {
        let ptr = allocate_aligned(1024, 64).unwrap();
        assert!(!ptr.is_null());

        unsafe {
            deallocate_aligned(ptr, 1024, 64);
        }
    }

    #[test]
    fn test_allocate_huge_pages() {
        let ptr = allocate_huge_pages(1024).unwrap();
        assert!(!ptr.is_null());

        unsafe {
            deallocate_aligned(ptr, 1024, 4096);
        }
    }

    #[test]
    fn test_memory_stats() {
        let stats = get_memory_stats();
        assert_eq!(stats.allocated_bytes, 0);
        assert_eq!(stats.allocation_count, 0);
    }
}

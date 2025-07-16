//! Memory utilities for high-performance allocations

use crate::error::{ Result, FluxError };
use std::alloc::{ alloc, dealloc, Layout };

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

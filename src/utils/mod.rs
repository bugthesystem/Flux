//! Utility functions and helpers for the Flux library
//!
//! This module provides common utilities, performance optimizations,
//! and helper functions used throughout the library.

pub mod cpu;
pub mod cpu_simple;
pub mod memory;
pub mod time;
pub mod checksum;
pub mod numa;

#[cfg(target_os = "macos")]
pub mod macos_optimizations;

#[cfg(
    all(
        target_os = "linux",
        any(feature = "linux_numa", feature = "linux_affinity", feature = "linux_hugepages")
    )
)]
pub mod linux_numa;

#[cfg(target_arch = "aarch64")]
// NEON SIMD functionality moved to optimizations/advanced_simd.rs

// Re-export commonly used utilities
pub use cpu_simple::{
    set_cpu_affinity,
    get_cpu_count,
    pin_to_cpu,
    is_thread_affinity_supported,
    get_thread_affinity_info,
};
pub use memory::{ allocate_aligned, allocate_huge_pages, lock_memory };
pub use time::{ get_nanos, get_tsc, TimestampProvider };
pub use checksum::{ xxhash32, xxhash64, crc32 };
pub use numa::{ allocate_with_cpu_affinity, allocate_local_cpu };

// Linux-specific optimizations
#[cfg(
    all(
        target_os = "linux",
        any(feature = "linux_numa", feature = "linux_affinity", feature = "linux_hugepages")
    )
)]
pub use linux_numa::{
    LinuxNumaOptimizer,
    pin_to_cpu as linux_pin_to_cpu,
    set_max_priority as linux_set_max_priority,
    lock_memory as linux_lock_memory,
    allocate_huge_pages as linux_allocate_huge_pages,
};

/// System page size
pub const PAGE_SIZE: usize = 4096;

/// Huge page size (2MB)
pub const HUGE_PAGE_SIZE: usize = 2 * 1024 * 1024;

/// Default alignment for memory allocations
pub const DEFAULT_ALIGNMENT: usize = 64;

/// Round up to the next power of two
pub fn next_power_of_two(x: usize) -> usize {
    if x == 0 {
        return 1;
    }
    let mut power = 1;
    while power < x {
        power <<= 1;
    }
    power
}

/// Check if a number is a power of two
pub fn is_power_of_two(x: usize) -> bool {
    x != 0 && (x & (x - 1)) == 0
}

/// Round up to the next multiple of the given alignment
pub fn round_up_to_alignment(value: usize, alignment: usize) -> usize {
    (value + alignment - 1) & !(alignment - 1)
}

/// Round down to the previous multiple of the given alignment
pub fn round_down_to_alignment(value: usize, alignment: usize) -> usize {
    value & !(alignment - 1)
}

/// Prefetch memory for better cache performance
#[inline]
pub fn prefetch_read(addr: *const u8) {
    #[cfg(target_arch = "x86_64")]
    unsafe {
        std::arch::x86_64::_mm_prefetch(addr as *const i8, std::arch::x86_64::_MM_HINT_T0);
    }
    #[cfg(not(target_arch = "x86_64"))]
    {
        // No-op on non-x86_64
        let _ = addr;
    }
}

/// Prefetch memory for writing
#[inline]
pub fn prefetch_write(addr: *const u8) {
    #[cfg(target_arch = "x86_64")]
    unsafe {
        std::arch::x86_64::_mm_prefetch(addr as *const i8, std::arch::x86_64::_MM_HINT_T1);
    }
    #[cfg(not(target_arch = "x86_64"))]
    {
        // No-op on non-x86_64
        let _ = addr;
    }
}

/// Compiler fence to prevent reordering
#[inline]
pub fn compiler_fence() {
    std::sync::atomic::compiler_fence(std::sync::atomic::Ordering::SeqCst);
}

/// Full memory barrier
#[inline]
pub fn memory_barrier() {
    std::sync::atomic::fence(std::sync::atomic::Ordering::SeqCst);
}

/// CPU pause instruction for busy loops
#[inline]
pub fn cpu_pause() {
    std::hint::spin_loop();
}

/// Warm up CPU caches by accessing data patterns
pub fn warm_caches(data: &[u8], iterations: usize) {
    for _ in 0..iterations {
        let mut checksum = 0u64;
        for chunk in data.chunks(64) {
            // Cache line size
            for &byte in chunk {
                checksum = checksum.wrapping_add(byte as u64);
            }
        }
        std::hint::black_box(checksum); // Prevent optimization
    }
}

/// Get system information
pub fn get_system_info() -> SystemInfo {
    SystemInfo {
        cpu_count: get_cpu_count(),
        page_size: PAGE_SIZE,
        huge_page_size: HUGE_PAGE_SIZE,
        cache_line_size: crate::constants::CACHE_LINE_SIZE,
    }
}

/// System information structure
#[derive(Debug, Clone)]
pub struct SystemInfo {
    /// Number of CPU cores
    pub cpu_count: usize,
    /// System page size
    pub page_size: usize,
    /// Huge page size
    pub huge_page_size: usize,
    /// CPU cache line size
    pub cache_line_size: usize,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_next_power_of_two() {
        assert_eq!(next_power_of_two(0), 1);
        assert_eq!(next_power_of_two(1), 1);
        assert_eq!(next_power_of_two(2), 2);
        assert_eq!(next_power_of_two(3), 4);
        assert_eq!(next_power_of_two(7), 8);
        assert_eq!(next_power_of_two(1024), 1024);
        assert_eq!(next_power_of_two(1025), 2048);
    }

    #[test]
    fn test_is_power_of_two() {
        assert!(!is_power_of_two(0));
        assert!(is_power_of_two(1));
        assert!(is_power_of_two(2));
        assert!(!is_power_of_two(3));
        assert!(is_power_of_two(4));
        assert!(!is_power_of_two(5));
        assert!(is_power_of_two(1024));
        assert!(!is_power_of_two(1025));
    }

    #[test]
    fn test_alignment() {
        assert_eq!(round_up_to_alignment(100, 64), 128);
        assert_eq!(round_up_to_alignment(64, 64), 64);
        assert_eq!(round_up_to_alignment(65, 64), 128);

        assert_eq!(round_down_to_alignment(100, 64), 64);
        assert_eq!(round_down_to_alignment(64, 64), 64);
        assert_eq!(round_down_to_alignment(65, 64), 64);
    }

    #[test]
    fn test_system_info() {
        let info = get_system_info();
        assert!(info.cpu_count > 0);
        assert!(info.page_size > 0);
        assert!(info.huge_page_size > info.page_size);
        assert!(info.cache_line_size > 0);
    }
}

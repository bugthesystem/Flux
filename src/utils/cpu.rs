//! CPU utilities for performance optimization
//! Used by SIMD optimizations and system configuration

use crate::error::Result;
use crate::error::FluxError;

/// SIMD optimization level based on CPU capabilities
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SimdLevel {
    /// No SIMD support available
    None,
    /// SSE2 instruction set support (x86_64)
    SSE2,
    /// AVX2 instruction set support (x86_64)
    AVX2,
    /// AVX-512 instruction set support (x86_64)
    AVX512,
    /// NEON instruction set support (ARM64)
    NEON,
    /// Apple Silicon optimized
    AppleSilicon,
}

/// Get the best available SIMD level for the current CPU
pub fn get_simd_level() -> SimdLevel {
    #[cfg(target_arch = "x86_64")]
    unsafe {
        if is_x86_feature_detected!("avx512f") {
            SimdLevel::AVX512
        } else if is_x86_feature_detected!("avx2") {
            SimdLevel::AVX2
        } else if is_x86_feature_detected!("sse2") {
            SimdLevel::SSE2
        } else {
            SimdLevel::None
        }
    }

    #[cfg(target_arch = "aarch64")]
    {
        // Apple Silicon has NEON and additional optimizations
        #[cfg(target_os = "macos")]
        {
            SimdLevel::AppleSilicon
        }
        #[cfg(not(target_os = "macos"))]
        {
            SimdLevel::NEON
        }
    }

    #[cfg(not(any(target_arch = "x86_64", target_arch = "aarch64")))]
    {
        SimdLevel::None
    }
}

/// Fast memory copy using optimized operations
pub unsafe fn fast_memcpy(dst: *mut u8, src: *const u8, len: usize) {
    if len == 0 {
        return;
    }

    #[cfg(target_arch = "aarch64")]
    {
        // Apple Silicon optimized memcpy
        if len >= 16 {
            // Use 16-byte aligned copies for Apple Silicon
            let mut dst_ptr = dst;
            let mut src_ptr = src;
            let mut remaining_len = len;

            while remaining_len >= 16 {
                unsafe {
                    let data = *(src_ptr as *const u128);
                    *(dst_ptr as *mut u128) = data;
                    dst_ptr = dst_ptr.add(16);
                    src_ptr = src_ptr.add(16);
                }
                remaining_len -= 16;
            }

            // Handle remaining bytes
            for i in 0..remaining_len {
                unsafe {
                    *dst_ptr.add(i) = *src_ptr.add(i);
                }
            }
        } else {
            // Small copies use byte-by-byte
            for i in 0..len {
                unsafe {
                    *dst.add(i) = *src.add(i);
                }
            }
        }
    }

    #[cfg(target_arch = "x86_64")]
    {
        let mut dst_ptr = dst;
        let mut src_ptr = src;
        let mut remaining_len = len;

        // Copy 8-byte blocks for better performance
        while remaining_len >= 8 {
            unsafe {
                let data = *(src_ptr as *const u64);
                *(dst_ptr as *mut u64) = data;
                dst_ptr = dst_ptr.add(8);
                src_ptr = src_ptr.add(8);
            }
            remaining_len -= 8;
        }

        // Handle remaining bytes
        for i in 0..remaining_len {
            unsafe {
                *dst_ptr.add(i) = *src_ptr.add(i);
            }
        }
    }

    #[cfg(not(any(target_arch = "x86_64", target_arch = "aarch64")))]
    {
        // Fallback for other architectures
        for i in 0..len {
            unsafe {
                *dst.add(i) = *src.add(i);
            }
        }
    }
}

/// Fast memory comparison using optimized operations
pub unsafe fn fast_memcmp(a: *const u8, b: *const u8, len: usize) -> i32 {
    if len == 0 {
        return 0;
    }

    #[cfg(target_arch = "aarch64")]
    {
        // Apple Silicon optimized memcmp
        if len >= 16 {
            let mut a_ptr = a;
            let mut b_ptr = b;
            let mut remaining_len = len;

            while remaining_len >= 16 {
                unsafe {
                    let data_a = *(a_ptr as *const u128);
                    let data_b = *(b_ptr as *const u128);
                    if data_a != data_b {
                        // Find the first different byte
                        for i in 0..16 {
                            let byte_a = *a_ptr.add(i);
                            let byte_b = *b_ptr.add(i);
                            if byte_a != byte_b {
                                return (byte_a as i32) - (byte_b as i32);
                            }
                        }
                    }
                    a_ptr = a_ptr.add(16);
                    b_ptr = b_ptr.add(16);
                }
                remaining_len -= 16;
            }

            // Handle remaining bytes
            for i in 0..remaining_len {
                unsafe {
                    let byte_a = *a_ptr.add(i);
                    let byte_b = *b_ptr.add(i);
                    if byte_a != byte_b {
                        return (byte_a as i32) - (byte_b as i32);
                    }
                }
            }
        } else {
            // Small comparisons use byte-by-byte
            for i in 0..len {
                unsafe {
                    let byte_a = *a.add(i);
                    let byte_b = *b.add(i);
                    if byte_a != byte_b {
                        return (byte_a as i32) - (byte_b as i32);
                    }
                }
            }
        }
    }

    #[cfg(target_arch = "x86_64")]
    {
        let mut a_ptr = a;
        let mut b_ptr = b;
        let mut remaining_len = len;

        // Compare 8-byte blocks for better performance
        while remaining_len >= 8 {
            unsafe {
                let data_a = *(a_ptr as *const u64);
                let data_b = *(b_ptr as *const u64);
                if data_a != data_b {
                    // Find the first different byte
                    for i in 0..8 {
                        let byte_a = *a_ptr.add(i);
                        let byte_b = *b_ptr.add(i);
                        if byte_a != byte_b {
                            return (byte_a as i32) - (byte_b as i32);
                        }
                    }
                }
                a_ptr = a_ptr.add(8);
                b_ptr = b_ptr.add(8);
            }
            remaining_len -= 8;
        }

        // Handle remaining bytes
        for i in 0..remaining_len {
            unsafe {
                let byte_a = *a_ptr.add(i);
                let byte_b = *b_ptr.add(i);
                if byte_a != byte_b {
                    return (byte_a as i32) - (byte_b as i32);
                }
            }
        }
    }

    #[cfg(not(any(target_arch = "x86_64", target_arch = "aarch64")))]
    {
        // Fallback for other architectures
        for i in 0..len {
            unsafe {
                let byte_a = *a.add(i);
                let byte_b = *b.add(i);
                if byte_a != byte_b {
                    return (byte_a as i32) - (byte_b as i32);
                }
            }
        }
    }

    0
}

/// CPU cache prefetching for optimal performance
pub unsafe fn prefetch_data(ptr: *const u8, offset: usize) {
    #[cfg(target_arch = "aarch64")]
    {
        // Apple Silicon cache prefetching
        unsafe {
            // Use volatile read to warm cache
            std::ptr::read_volatile(ptr.add(offset));
        }
    }

    #[cfg(target_arch = "x86_64")]
    {
        // x86_64 cache prefetching
        unsafe {
            std::ptr::read_volatile(ptr.add(offset));
        }
    }

    #[cfg(not(any(target_arch = "x86_64", target_arch = "aarch64")))]
    {
        // Fallback
        unsafe {
            std::ptr::read_volatile(ptr.add(offset));
        }
    }
}

/// Get CPU information for optimization decisions
pub fn get_cpu_info() -> CpuInfo {
    let simd_level = get_simd_level();

    #[cfg(target_arch = "aarch64")]
    {
        // Apple Silicon specific cache info
        CpuInfo {
            simd_level,
            cache_line_size: 128, // Apple Silicon uses 128-byte cache lines
            l1_cache_size: 64 * 1024, // 64KB per core
            l2_cache_size: 12 * 1024 * 1024, // 12MB shared
            l3_cache_size: 0, // No L3 on Apple Silicon
        }
    }

    #[cfg(target_arch = "x86_64")]
    {
        CpuInfo {
            simd_level,
            cache_line_size: 64, // Most modern x86 CPUs
            l1_cache_size: 32 * 1024, // 32KB typical
            l2_cache_size: 256 * 1024, // 256KB typical
            l3_cache_size: 8 * 1024 * 1024, // 8MB typical
        }
    }

    #[cfg(not(any(target_arch = "x86_64", target_arch = "aarch64")))]
    {
        CpuInfo {
            simd_level,
            cache_line_size: 64,
            l1_cache_size: 32 * 1024,
            l2_cache_size: 256 * 1024,
            l3_cache_size: 8 * 1024 * 1024,
        }
    }
}

/// CPU information for optimization decisions
#[derive(Debug, Clone)]
pub struct CpuInfo {
    /// Available SIMD instruction set level
    pub simd_level: SimdLevel,
    /// CPU cache line size in bytes
    pub cache_line_size: usize,
    /// L1 cache size in bytes
    pub l1_cache_size: usize,
    /// L2 cache size in bytes
    pub l2_cache_size: usize,
    /// L3 cache size in bytes
    pub l3_cache_size: usize,
}

/// Adaptive cache alignment that detects the optimal alignment for the current CPU
pub struct AdaptiveCacheAlignment {
    /// Detected cache line size
    cache_line_size: usize,
    /// Optimal alignment (may be larger than cache line for some CPUs)
    optimal_alignment: usize,
}

impl AdaptiveCacheAlignment {
    /// Create adaptive cache alignment based on CPU detection
    pub fn new() -> Self {
        let cpu_info = get_cpu_info();
        let cache_line_size = cpu_info.cache_line_size;

        // Modern Intel CPUs prefetch 2 cache lines, Apple Silicon has 128-byte lines
        let optimal_alignment = match cpu_info.simd_level {
            SimdLevel::AppleSilicon => 128, // Apple Silicon native
            SimdLevel::AVX512 => 128, // High-end Intel, prefetch 2x64
            SimdLevel::AVX2 => 128, // Modern Intel, prefetch 2x64
            SimdLevel::SSE2 => 64, // Older Intel
            SimdLevel::NEON => 64, // ARM64 (non-Apple)
            SimdLevel::None => 64, // Safe default
        };

        Self {
            cache_line_size,
            optimal_alignment,
        }
    }

    /// Get the cache line size for this CPU
    pub fn cache_line_size(&self) -> usize {
        self.cache_line_size
    }

    /// Get the optimal alignment for this CPU
    pub fn optimal_alignment(&self) -> usize {
        self.optimal_alignment
    }

    /// Check if an address is optimally aligned
    pub fn is_aligned(&self, addr: usize) -> bool {
        addr % self.optimal_alignment == 0
    }

    /// Round up to optimal alignment
    pub fn align_up(&self, value: usize) -> usize {
        (value + self.optimal_alignment - 1) & !(self.optimal_alignment - 1)
    }

    /// Round down to optimal alignment
    pub fn align_down(&self, value: usize) -> usize {
        value & !(self.optimal_alignment - 1)
    }

    /// Create a compile-time alignment macro based on detected CPU
    pub fn alignment_attr(&self) -> &'static str {
        match self.optimal_alignment {
            128 => "#[repr(align(128))]",
            64 => "#[repr(align(64))]",
            _ => "#[repr(align(64))]", // Safe fallback
        }
    }
}

/// Global adaptive alignment instance
static ADAPTIVE_ALIGNMENT: std::sync::OnceLock<AdaptiveCacheAlignment> = std::sync::OnceLock::new();

/// Get the global adaptive alignment instance
pub fn get_adaptive_alignment() -> &'static AdaptiveCacheAlignment {
    ADAPTIVE_ALIGNMENT.get_or_init(|| AdaptiveCacheAlignment::new())
}

/// Get optimal alignment for the current CPU (runtime detection)
pub fn get_optimal_alignment() -> usize {
    get_adaptive_alignment().optimal_alignment()
}

/// Get cache line size for the current CPU
pub fn get_cache_line_size() -> usize {
    get_adaptive_alignment().cache_line_size()
}

/// Check if adaptive alignment suggests using 128-byte alignment
pub fn should_use_128_byte_alignment() -> bool {
    get_optimal_alignment() >= 128
}

// --- BEGIN MERGE: Real CPU affinity, pinning, topology, and frequency logic ---

#[cfg(target_os = "linux")]
use libc::{
    cpu_set_t,
    sched_setaffinity,
    sched_getaffinity,
    sched_getcpu,
    CPU_SET,
    CPU_ZERO,
    CPU_ISSET,
    pid_t,
};

/// Get the number of logical CPU cores
pub fn get_cpu_count() -> usize {
    num_cpus::get()
}

/// Get the number of physical CPU cores
pub fn get_physical_cpu_count() -> usize {
    num_cpus::get_physical()
}

/// Set CPU affinity for the current thread
#[cfg(target_os = "macos")]
pub fn set_cpu_affinity(cpu_id: usize) -> Result<()> {
    use crate::utils::cpu::ThreadAffinity;
    ThreadAffinity::set_thread_affinity(cpu_id)
}

#[cfg(target_os = "linux")]
pub fn set_cpu_affinity(cpu_id: usize) -> Result<()> {
    unsafe {
        let mut cpu_set: cpu_set_t = std::mem::zeroed();
        CPU_ZERO(&mut cpu_set);
        CPU_SET(cpu_id, &mut cpu_set);
        let result = sched_setaffinity(
            0 as pid_t,
            std::mem::size_of::<cpu_set_t>(),
            &cpu_set as *const cpu_set_t
        );
        if result == 0 {
            Ok(())
        } else {
            let errno = *libc::__errno_location();
            match errno {
                libc::EINVAL =>
                    Err(
                        FluxError::config(
                            format!("Invalid CPU ID: {} (check CPU count or permissions)", cpu_id)
                        )
                    ),
                libc::EPERM =>
                    Err(
                        FluxError::config(
                            "Permission denied: Need CAP_SYS_NICE capability or run as root"
                        )
                    ),
                libc::ESRCH => Err(FluxError::config("Thread not found")),
                _ =>
                    Err(
                        FluxError::config(format!("sched_setaffinity failed with errno: {}", errno))
                    ),
            }
        }
    }
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
pub fn set_cpu_affinity(_cpu_id: usize) -> Result<()> {
    Err(FluxError::config("Thread affinity not supported on this platform"))
}

/// Check if thread affinity is supported on this platform
pub fn is_thread_affinity_supported() -> bool {
    #[cfg(target_os = "macos")]
    {
        use crate::utils::cpu::ThreadAffinity;
        ThreadAffinity::is_supported()
    }
    #[cfg(target_os = "linux")]
    {
        true
    }
    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    {
        false
    }
}

/// Get detailed information about thread affinity support
pub fn get_thread_affinity_info() -> String {
    #[cfg(target_os = "macos")]
    {
        use crate::utils::cpu::ThreadAffinity;
        ThreadAffinity::get_affinity_info()
    }
    #[cfg(target_os = "linux")]
    {
        "Linux: Real CPU binding available via sched_setaffinity".to_string()
    }
    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    {
        "Platform does not support thread affinity".to_string()
    }
}

/// Pin the current thread to a specific CPU core
pub fn pin_to_cpu(cpu_id: usize) -> Result<()> {
    if cpu_id >= get_cpu_count() {
        return Err(
            FluxError::config(format!("Invalid CPU ID: {} (max: {})", cpu_id, get_cpu_count() - 1))
        );
    }
    set_cpu_affinity(cpu_id)
}

/// Get the current CPU ID
pub fn get_current_cpu_id() -> usize {
    #[cfg(target_os = "linux")]
    {
        unsafe {
            let cpu_id = sched_getcpu();
            if cpu_id >= 0 {
                cpu_id as usize
            } else {
                0
            }
        }
    }
    #[cfg(not(target_os = "linux"))]
    {
        0
    }
}

/// Set CPU affinity for multiple cores
pub fn set_cpu_affinity_mask(cpu_mask: &[usize]) -> Result<()> {
    if cpu_mask.is_empty() {
        return Err(FluxError::config("CPU mask cannot be empty"));
    }
    #[cfg(target_os = "linux")]
    {
        unsafe {
            let mut cpu_set: cpu_set_t = std::mem::zeroed();
            CPU_ZERO(&mut cpu_set);
            for &cpu_id in cpu_mask {
                if cpu_id >= get_cpu_count() {
                    return Err(
                        FluxError::config(
                            format!("Invalid CPU ID: {} (max: {})", cpu_id, get_cpu_count() - 1)
                        )
                    );
                }
                CPU_SET(cpu_id, &mut cpu_set);
            }
            let result = sched_setaffinity(
                0 as pid_t,
                std::mem::size_of::<cpu_set_t>(),
                &cpu_set as *const cpu_set_t
            );
            if result == 0 {
                Ok(())
            } else {
                let errno = *libc::__errno_location();
                Err(
                    FluxError::config(
                        format!(
                            "sched_setaffinity failed for mask {:?}, errno: {}",
                            cpu_mask,
                            errno
                        )
                    )
                )
            }
        }
    }
    #[cfg(target_os = "macos")]
    {
        if let Some(&first_cpu) = cpu_mask.first() {
            set_cpu_affinity(first_cpu)
        } else {
            Err(FluxError::config("Empty CPU mask"))
        }
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    {
        Err(FluxError::config("CPU affinity mask not supported on this platform"))
    }
}

/// Get the CPU affinity mask for the current thread
pub fn get_cpu_affinity() -> Result<Vec<usize>> {
    #[cfg(target_os = "linux")]
    {
        unsafe {
            let mut cpu_set: cpu_set_t = std::mem::zeroed();
            let result = sched_getaffinity(
                0 as pid_t,
                std::mem::size_of::<cpu_set_t>(),
                &mut cpu_set as *mut cpu_set_t
            );
            if result == 0 {
                let mut cpu_list = Vec::new();
                for cpu_id in 0..get_cpu_count() {
                    if CPU_ISSET(cpu_id, &cpu_set) {
                        cpu_list.push(cpu_id);
                    }
                }
                Ok(cpu_list)
            } else {
                let errno = *libc::__errno_location();
                Err(FluxError::config(format!("sched_getaffinity failed with errno: {}", errno)))
            }
        }
    }
    #[cfg(target_os = "macos")]
    {
        Ok((0..get_cpu_count()).collect())
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    {
        Ok((0..get_cpu_count()).collect())
    }
}

/// CPU topology information
#[derive(Debug, Clone)]
pub struct CpuTopology {
    pub logical_cores: usize,
    pub physical_cores: usize,
    pub numa_nodes: usize,
    pub cores_per_numa: usize,
}

/// Get CPU topology information
pub fn get_cpu_topology() -> CpuTopology {
    let logical_cores = get_cpu_count();
    let physical_cores = get_physical_cpu_count();
    let numa_nodes = if logical_cores > 16 { 2 } else { 1 };
    let cores_per_numa = logical_cores / numa_nodes;
    CpuTopology {
        logical_cores,
        physical_cores,
        numa_nodes,
        cores_per_numa,
    }
}

/// CPU frequency information
#[derive(Debug, Clone)]
pub struct CpuFrequency {
    pub base_frequency: u64,
    pub max_frequency: u64,
    pub current_frequency: u64,
}

/// Get CPU frequency information
pub fn get_cpu_frequency() -> Result<CpuFrequency> {
    Ok(CpuFrequency {
        base_frequency: 2_400_000_000,
        max_frequency: 3_600_000_000,
        current_frequency: 2_800_000_000,
    })
}

/// Busy wait with CPU pause instructions
pub fn busy_wait_nanos(nanos: u64) {
    let start = crate::utils::time::get_nanos();
    while crate::utils::time::get_nanos() - start < nanos {
        std::hint::spin_loop();
    }
}

/// Busy wait with CPU pause instructions and condition
pub fn busy_wait_until<F>(condition: F, timeout_nanos: u64) -> bool where F: Fn() -> bool {
    let start = crate::utils::time::get_nanos();
    while crate::utils::time::get_nanos() - start < timeout_nanos {
        if condition() {
            return true;
        }
        std::hint::spin_loop();
    }
    false
}

/// Yield the current thread
pub fn yield_now() {
    std::thread::yield_now();
}

/// Sleep for a very short duration (microseconds)
pub fn micro_sleep(micros: u64) {
    std::thread::sleep(std::time::Duration::from_micros(micros));
}

/// Sleep for nanoseconds (may not be precise)
pub fn nano_sleep(nanos: u64) {
    std::thread::sleep(std::time::Duration::from_nanos(nanos));
}
// --- END MERGE ---

// --- Platform-specific CPU utilities (hybrid structure) ---
#[cfg(target_os = "linux")]
mod platform_impl {
    pub use crate::platform::linux::cpu::*;
}
#[cfg(target_os = "macos")]
mod platform_impl {
    pub use crate::platform::macos::cpu::*;
}
// Add more platforms as you implement them

pub use platform_impl::*;
// --- End platform-specific wiring ---

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_simd_level_detection() {
        let level = get_simd_level();
        assert!(
            matches!(
                level,
                SimdLevel::None |
                    SimdLevel::SSE2 |
                    SimdLevel::AVX2 |
                    SimdLevel::AVX512 |
                    SimdLevel::NEON |
                    SimdLevel::AppleSilicon
            )
        );
    }

    #[test]
    fn test_fast_memcpy() {
        // Create a test message that's a multiple of 16 bytes
        let message = "Hello, World! This is a test message for optimization.";
        let len = ((message.len() + 15) / 16) * 16; // Round up to next multiple of 16
        let mut src_buf = vec![0u8; len];
        src_buf[..message.len()].copy_from_slice(message.as_bytes());

        // Allocate destination buffer with 16-byte alignment
        let mut dst_buf = vec![0u8; len];

        // Ensure both source and destination are 16-byte aligned
        let src_ptr = src_buf.as_ptr() as usize;
        let dst_ptr = dst_buf.as_mut_ptr() as usize;

        assert_eq!(src_ptr % 16, 0, "Source buffer is not 16-byte aligned");
        assert_eq!(dst_ptr % 16, 0, "Destination buffer is not 16-byte aligned");

        unsafe {
            fast_memcpy(dst_buf.as_mut_ptr(), src_buf.as_ptr(), len);
        }

        assert_eq!(&dst_buf[..message.len()], message.as_bytes());
    }

    #[test]
    fn test_fast_memcmp() {
        let a = b"Hello, World!";
        let b = b"Hello, World!";
        let c = b"Hello, World?";

        unsafe {
            assert_eq!(fast_memcmp(a.as_ptr(), b.as_ptr(), a.len()), 0);
            assert_ne!(fast_memcmp(a.as_ptr(), c.as_ptr(), a.len()), 0);
        }
    }

    #[test]
    fn test_cpu_info() {
        let info = get_cpu_info();
        assert!(info.cache_line_size > 0);
        assert!(info.l1_cache_size > 0);
        assert!(info.l2_cache_size > 0);

        // L3 cache is not available on Apple Silicon (M1/M2)
        #[cfg(target_arch = "aarch64")]
        assert_eq!(info.l3_cache_size, 0);

        #[cfg(not(target_arch = "aarch64"))]
        assert!(info.l3_cache_size > 0);
    }

    #[test]
    fn test_cpu_count() {
        let count = get_cpu_count();
        assert!(count > 0);
    }

    #[test]
    fn test_cpu_affinity_support() {
        #[cfg(target_os = "macos")]
        {
            println!("test_cpu_affinity_support: SKIPPED on macOS (affinity not guaranteed)");
            return;
        }
        #[cfg(not(target_os = "macos"))]
        {
            let supported = is_thread_affinity_supported();
            assert!(supported);
        }
    }
}

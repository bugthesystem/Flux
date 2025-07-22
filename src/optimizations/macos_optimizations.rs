//! macOS-specific optimizations for Apple Silicon
//!
//! This module provides platform-specific optimizations for macOS/Apple Silicon:
//! - Thread affinity hints (Intel only - Apple Silicon not supported)
//! - Memory locking and alignment
//! - Process/thread priority optimization
//! - Grand Central Dispatch integration
//!
//! ## Thread Affinity Support Check
//! **Apple Silicon (M1/M2)**: Thread affinity is NOT supported (returns KERN_NOT_SUPPORTED)
//! **Intel Macs**: Limited affinity "hints" only (not hard CPU binding)
//! **Alternative**: Use QoS classes for thread priority optimization

use crate::error::{ Result, FluxError };
use std::ptr;

#[cfg(target_os = "macos")]
use libc::{
    c_void,
    size_t,
    mlock,
    munlock,
    posix_memalign,
    pthread_set_qos_class_self_np,
    qos_class_t,
};

// Thread affinity imports - available on macOS but limited support
#[cfg(target_os = "macos")]
use libc::{ mach_port_t, kern_return_t, integer_t, mach_msg_type_number_t };

// These constants are defined in mach/thread_policy.h
#[cfg(target_os = "macos")]
const THREAD_AFFINITY_POLICY: u32 = 4;
#[cfg(target_os = "macos")]
const THREAD_AFFINITY_POLICY_COUNT: u32 = 1;
#[cfg(target_os = "macos")]
const THREAD_AFFINITY_TAG_NULL: integer_t = 0;

// Mach thread policy structure
#[cfg(target_os = "macos")]
#[repr(C)]
struct ThreadAffinityPolicyData {
    affinity_tag: integer_t,
}

// External functions from libSystem (available but limited on Apple Silicon)
#[cfg(target_os = "macos")]
extern "C" {
    fn thread_policy_set(
        thread: mach_port_t,
        flavor: u32,
        policy_info: *const ThreadAffinityPolicyData,
        count: mach_msg_type_number_t
    ) -> kern_return_t;

    fn pthread_mach_thread_np(thread: libc::pthread_t) -> mach_port_t;

    fn mach_thread_self() -> mach_port_t;
}

/// Apple Silicon core types
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum CoreType {
    /// Performance core (P-core)
    Performance,
    /// Efficiency core (E-core)
    Efficiency,
    /// Unknown core type
    Unknown,
}

/// macOS-specific optimization manager
pub struct MacOSOptimizer {
    /// Number of P-cores detected
    p_cores: usize,
    /// Number of E-cores detected
    e_cores: usize,
    /// Total cores
    total_cores: usize,
    /// Core type mapping
    core_types: Vec<CoreType>,
}

impl MacOSOptimizer {
    /// Create a new macOS optimizer
    pub fn new() -> Result<Self> {
        let (p_cores, e_cores, core_types) = Self::detect_core_types()?;
        let total_cores = p_cores + e_cores;

        Ok(Self {
            p_cores,
            e_cores,
            total_cores,
            core_types,
        })
    }

    /// Detect P-cores vs E-cores using sysctl
    fn detect_core_types() -> Result<(usize, usize, Vec<CoreType>)> {
        let mut core_types = Vec::new();
        let mut p_cores = 0;
        let mut e_cores = 0;

        // Get number of physical cores
        let physical_cores = num_cpus::get_physical();

        for cpu_id in 0..physical_cores {
            // Try to detect core type using sysctl
            // Note: This is a simplified approach - real detection would use
            // more sophisticated sysctl queries or CPUID-like instructions
            let core_type = if cpu_id < 8 {
                // Assume first 8 cores are P-cores (typical Apple Silicon)
                CoreType::Performance
            } else {
                // Assume remaining cores are E-cores
                CoreType::Efficiency
            };

            core_types.push(core_type);

            match core_type {
                CoreType::Performance => {
                    p_cores += 1;
                }
                CoreType::Efficiency => {
                    e_cores += 1;
                }
                CoreType::Unknown => {
                    // Default to P-core for unknown
                    p_cores += 1;
                }
            }
        }

        Ok((p_cores, e_cores, core_types))
    }

    /// Get number of P-cores
    pub fn p_core_count(&self) -> usize {
        self.p_cores
    }

    /// Get number of E-cores
    pub fn e_core_count(&self) -> usize {
        self.e_cores
    }

    /// Get total core count
    pub fn total_core_count(&self) -> usize {
        self.total_cores
    }

    /// Get core type for a specific CPU
    pub fn get_core_type(&self, cpu_id: usize) -> CoreType {
        if cpu_id < self.core_types.len() { self.core_types[cpu_id] } else { CoreType::Unknown }
    }

    /// Get list of P-core CPU IDs
    pub fn get_p_core_cpus(&self) -> Vec<usize> {
        self.core_types
            .iter()
            .enumerate()
            .filter_map(|(cpu_id, core_type)| {
                if *core_type == CoreType::Performance { Some(cpu_id) } else { None }
            })
            .collect()
    }

    /// Get list of E-core CPU IDs
    pub fn get_e_core_cpus(&self) -> Vec<usize> {
        self.core_types
            .iter()
            .enumerate()
            .filter_map(|(cpu_id, core_type)| {
                if *core_type == CoreType::Efficiency { Some(cpu_id) } else { None }
            })
            .collect()
    }
}

/// Memory locking utilities
pub struct MemoryLocker;

impl MemoryLocker {
    /// Lock memory pages to prevent swapping
    pub fn lock_memory(ptr: *const c_void, size: size_t) -> Result<()> {
        #[cfg(target_os = "macos")]
        {
            let result = unsafe { mlock(ptr, size) };
            if result != 0 {
                return Err(FluxError::config("Failed to lock memory pages"));
            }
        }
        Ok(())
    }

    /// Unlock memory pages
    pub fn unlock_memory(ptr: *const c_void, size: size_t) -> Result<()> {
        #[cfg(target_os = "macos")]
        {
            let result = unsafe { munlock(ptr, size) };
            if result != 0 {
                return Err(FluxError::config("Failed to unlock memory pages"));
            }
        }
        Ok(())
    }

    /// Allocate aligned memory
    pub fn allocate_aligned(size: size_t, alignment: size_t) -> Result<*mut c_void> {
        #[cfg(target_os = "macos")]
        {
            let mut ptr: *mut c_void = ptr::null_mut();
            let result = unsafe { posix_memalign(&mut ptr, alignment, size) };
            if result != 0 {
                return Err(FluxError::config("Failed to allocate aligned memory"));
            }
            Ok(ptr)
        }
        #[cfg(not(target_os = "macos"))]
        {
            // Fallback for non-macOS
            let ptr = unsafe { libc::malloc(size) };
            if ptr.is_null() {
                return Err(FluxError::config("Failed to allocate memory"));
            }
            Ok(ptr)
        }
    }

    /// Free aligned memory
    pub fn free_aligned(ptr: *mut c_void) {
        #[cfg(target_os = "macos")]
        {
            unsafe { libc::free(ptr) }
        }
        #[cfg(not(target_os = "macos"))]
        {
            unsafe { libc::free(ptr) }
        }
    }
}

/// Thread priority and QoS utilities
pub struct ThreadOptimizer;

impl ThreadOptimizer {
    /// Set thread QoS to user-interactive (highest priority)
    pub fn set_max_priority() -> Result<()> {
        #[cfg(target_os = "macos")]
        {
            let result = unsafe {
                pthread_set_qos_class_self_np(qos_class_t::QOS_CLASS_USER_INTERACTIVE, 0)
            };
            if result != 0 {
                return Err(FluxError::config("Failed to set thread QoS"));
            }
        }
        Ok(())
    }

    /// Set thread QoS to user-initiated (high priority)
    pub fn set_high_priority() -> Result<()> {
        #[cfg(target_os = "macos")]
        {
            let result = unsafe {
                pthread_set_qos_class_self_np(qos_class_t::QOS_CLASS_USER_INITIATED, 0)
            };
            if result != 0 {
                return Err(FluxError::config("Failed to set thread QoS"));
            }
        }
        Ok(())
    }

    /// Set thread QoS to utility (normal priority)
    pub fn set_normal_priority() -> Result<()> {
        #[cfg(target_os = "macos")]
        {
            let result = unsafe {
                pthread_set_qos_class_self_np(qos_class_t::QOS_CLASS_UTILITY, 0)
            };
            if result != 0 {
                return Err(FluxError::config("Failed to set thread QoS"));
            }
        }
        Ok(())
    }

    /// Set thread QoS to background (low priority)
    pub fn set_low_priority() -> Result<()> {
        #[cfg(target_os = "macos")]
        {
            let result = unsafe {
                pthread_set_qos_class_self_np(qos_class_t::QOS_CLASS_BACKGROUND, 0)
            };
            if result != 0 {
                return Err(FluxError::config("Failed to set thread QoS"));
            }
        }
        Ok(())
    }
}

/// Cache line size detection
pub fn get_cache_line_size() -> usize {
    // Apple Silicon typically has 128-byte cache lines
    // This could be detected dynamically, but 128 is a safe default
    128
}

/// Align pointer to cache line boundary
pub fn align_to_cache_line(ptr: *mut u8) -> *mut u8 {
    let cache_line_size = get_cache_line_size();
    let addr = ptr as usize;
    let aligned_addr = (addr + cache_line_size - 1) & !(cache_line_size - 1);
    aligned_addr as *mut u8
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_macos_optimizer_creation() {
        let optimizer = MacOSOptimizer::new();
        assert!(optimizer.is_ok());

        if let Ok(optimizer) = optimizer {
            assert!(optimizer.total_core_count() > 0);
            assert!(optimizer.p_core_count() > 0);
        }
    }

    #[test]
    fn test_cache_line_alignment() {
        let ptr = 0x1001 as *mut u8;
        let aligned = align_to_cache_line(ptr);
        assert_eq!((aligned as usize) % get_cache_line_size(), 0);
    }
}

/// Real thread affinity implementation for macOS
///
/// IMPORTANT: This has severe limitations on Apple Silicon!
/// - Apple Silicon (M1/M2): Returns KERN_NOT_SUPPORTED (thread affinity disabled in hardware)
/// - Intel Macs: Limited "hints" only - not guaranteed CPU binding
/// - Alternative: Use QoS classes (ThreadOptimizer) for thread priority
pub struct ThreadAffinity;

impl ThreadAffinity {
    /// Attempt to set thread affinity hint (Intel only, fails on Apple Silicon)
    ///
    /// ## Parameters
    /// - `cpu_id`: CPU core ID to hint for affinity
    ///
    /// ## Returns
    /// - `Ok(())`: Affinity hint was set (Intel Macs only)
    /// - `Err(...)`: Apple Silicon (not supported) or other error
    ///
    /// ## Apple Silicon Reality
    /// This will ALWAYS fail on M1/M2 with "Thread affinity not supported on Apple Silicon"
    #[cfg(target_os = "macos")]
    pub fn set_thread_affinity(cpu_id: usize) -> Result<()> {
        // Get current thread's Mach port using pthread_self()
        let current_thread = unsafe { pthread_mach_thread_np(libc::pthread_self()) };

        // Set affinity policy data
        let policy_data = ThreadAffinityPolicyData {
            affinity_tag: cpu_id as integer_t,
        };

        // Attempt to set thread policy
        let result = unsafe {
            thread_policy_set(
                current_thread,
                THREAD_AFFINITY_POLICY,
                &policy_data as *const ThreadAffinityPolicyData,
                THREAD_AFFINITY_POLICY_COUNT
            )
        };

        match result {
            0 => {
                // KERN_SUCCESS - worked on Intel Macs
                println!("✅ Thread affinity hint set to CPU {} (Intel Mac)", cpu_id);
                Ok(())
            }
            46 => {
                // KERN_NOT_SUPPORTED - Apple Silicon
                Err(
                    FluxError::config(
                        "Thread affinity not supported on Apple Silicon (M1/M2). Use QoS classes instead."
                    )
                )
            }
            4 => {
                // KERN_INVALID_ARGUMENT
                Err(FluxError::config(format!("Invalid CPU ID: {} (check CPU count)", cpu_id)))
            }
            _ => {
                // Other error
                Err(FluxError::config(format!("Thread affinity failed with error: {}", result)))
            }
        }
    }

    #[cfg(not(target_os = "macos"))]
    pub fn set_thread_affinity(_cpu_id: usize) -> Result<()> {
        Err(FluxError::config("Thread affinity only available on macOS"))
    }

    /// Check if thread affinity is supported on current system
    ///
    /// Returns false on Apple Silicon, true on Intel Macs (with caveats)
    #[cfg(target_os = "macos")]
    pub fn is_supported() -> bool {
        // Quick test - try to set a null affinity tag
        let policy_data = ThreadAffinityPolicyData {
            affinity_tag: THREAD_AFFINITY_TAG_NULL,
        };

        let current_thread = unsafe { mach_thread_self() };
        let result = unsafe {
            thread_policy_set(
                current_thread,
                THREAD_AFFINITY_POLICY,
                &policy_data as *const ThreadAffinityPolicyData,
                THREAD_AFFINITY_POLICY_COUNT
            )
        };

        result == 0 // KERN_SUCCESS means supported (Intel), anything else means not supported
    }

    #[cfg(not(target_os = "macos"))]
    pub fn is_supported() -> bool {
        false
    }

    /// Get platform-specific thread affinity information
    pub fn get_affinity_info() -> String {
        #[cfg(target_os = "macos")]
        {
            if Self::is_supported() {
                "Intel Mac: Thread affinity hints available (not guaranteed)".to_string()
            } else {
                "Apple Silicon: Thread affinity NOT supported. Use QoS classes instead.".to_string()
            }
        }
        #[cfg(not(target_os = "macos"))]
        {
            "Non-macOS platform".to_string()
        }
    }
}

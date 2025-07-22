//! Simplified CPU utilities without problematic dependencies

use crate::error::{ Result, FluxError };
use std::thread;

// Linux-specific imports for real thread affinity
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
///
/// ## Reality Check
/// - **Apple Silicon (M1/M2)**: NOT supported (returns error)
/// - **Intel Macs**: Limited affinity hints only (not guaranteed)
/// - **Linux**: Real CPU binding (sched_setaffinity)
/// - **Alternative**: Use thread priority/QoS optimization
pub fn set_cpu_affinity(cpu_id: usize) -> Result<()> {
    #[cfg(target_os = "macos")]
    {
        use crate::optimizations::macos_optimizations::ThreadAffinity;
        ThreadAffinity::set_thread_affinity(cpu_id)
    }

    #[cfg(target_os = "linux")]
    {
        // Real Linux thread affinity using sched_setaffinity
        unsafe {
            // Create CPU set
            let mut cpu_set: cpu_set_t = std::mem::zeroed();
            CPU_ZERO(&mut cpu_set);
            CPU_SET(cpu_id, &mut cpu_set);

            // Set affinity for current thread (0 = current thread)
            let result = sched_setaffinity(
                0 as pid_t, // 0 = current thread
                std::mem::size_of::<cpu_set_t>(), // Size of cpu_set_t
                &cpu_set as *const cpu_set_t // CPU set pointer
            );

            if result == 0 {
                println!("✅ Thread bound to CPU core {} (Linux)", cpu_id);
                Ok(())
            } else {
                let errno = *libc::__errno_location();
                match errno {
                    libc::EINVAL => {
                        Err(
                            FluxError::config(
                                format!("Invalid CPU ID: {} (check CPU count or permissions)", cpu_id)
                            )
                        )
                    }
                    libc::EPERM => {
                        Err(
                            FluxError::config(
                                "Permission denied: Need CAP_SYS_NICE capability or run as root"
                            )
                        )
                    }
                    libc::ESRCH => { Err(FluxError::config("Thread not found")) }
                    _ => {
                        Err(
                            FluxError::config(
                                format!("sched_setaffinity failed with errno: {}", errno)
                            )
                        )
                    }
                }
            }
        }
    }

    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    {
        Err(FluxError::config("Thread affinity not supported on this platform"))
    }
}

/// Check if thread affinity is supported on this platform
///
/// Returns `true` if thread affinity is available, `false` otherwise.
/// Note: Even if `true`, affinity may be limited (hints only on Intel Macs)
pub fn is_thread_affinity_supported() -> bool {
    #[cfg(target_os = "macos")]
    {
        use crate::optimizations::macos_optimizations::ThreadAffinity;
        ThreadAffinity::is_supported()
    }

    #[cfg(target_os = "linux")]
    {
        true // Linux generally supports real CPU affinity
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
        use crate::optimizations::macos_optimizations::ThreadAffinity;
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
///
/// Returns the CPU core ID that the current thread is running on.
/// On unsupported platforms, returns 0.
pub fn get_current_cpu_id() -> usize {
    #[cfg(target_os = "linux")]
    {
        // Use real sched_getcpu() on Linux
        unsafe {
            let cpu_id = libc::sched_getcpu();
            if cpu_id >= 0 {
                cpu_id as usize
            } else {
                0 // Fall back to 0 on error
            }
        }
    }

    #[cfg(not(target_os = "linux"))]
    {
        // macOS and other platforms don't have direct equivalent
        // Would need complex thread_info() calls or task_info()
        0
    }
}

/// Set CPU affinity for multiple cores
///
/// Sets the current thread to run on any of the specified CPU cores.
pub fn set_cpu_affinity_mask(cpu_mask: &[usize]) -> Result<()> {
    if cpu_mask.is_empty() {
        return Err(FluxError::config("CPU mask cannot be empty"));
    }

    #[cfg(target_os = "linux")]
    {
        unsafe {
            // Create CPU set
            let mut cpu_set: cpu_set_t = std::mem::zeroed();
            CPU_ZERO(&mut cpu_set);

            // Add each CPU to the set
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

            // Set affinity for current thread
            let result = sched_setaffinity(
                0 as pid_t,
                std::mem::size_of::<cpu_set_t>(),
                &cpu_set as *const cpu_set_t
            );

            if result == 0 {
                println!("✅ Thread bound to CPU cores: {:?} (Linux)", cpu_mask);
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
        // macOS doesn't support multi-core affinity masks
        // Fall back to single-core affinity for first CPU
        if let Some(&first_cpu) = cpu_mask.first() {
            println!("⚠️ macOS: Using single CPU {} from mask {:?}", first_cpu, cpu_mask);
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
///
/// Returns the list of CPU cores this thread is allowed to run on.
pub fn get_cpu_affinity() -> Result<Vec<usize>> {
    #[cfg(target_os = "linux")]
    {
        unsafe {
            // Create CPU set to receive current affinity
            let mut cpu_set: cpu_set_t = std::mem::zeroed();

            // Get current thread's affinity
            let result = libc::sched_getaffinity(
                0 as pid_t, // 0 = current thread
                std::mem::size_of::<cpu_set_t>(),
                &mut cpu_set as *mut cpu_set_t
            );

            if result == 0 {
                // Extract CPU IDs from the set
                let mut cpu_list = Vec::new();
                for cpu_id in 0..get_cpu_count() {
                    if libc::CPU_ISSET(cpu_id, &cpu_set) {
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
        // macOS doesn't have a direct equivalent to sched_getaffinity
        // thread_policy_get() would be complex and limited
        // Return all CPUs as available (conservative approach)
        Ok((0..get_cpu_count()).collect())
    }

    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    {
        // Default to all CPUs for unsupported platforms
        Ok((0..get_cpu_count()).collect())
    }
}

/// CPU topology information
#[derive(Debug, Clone)]
pub struct CpuTopology {
    /// Total number of logical cores
    pub logical_cores: usize,
    /// Total number of physical cores
    pub physical_cores: usize,
    /// Number of NUMA nodes
    pub numa_nodes: usize,
    /// CPU cores per NUMA node
    pub cores_per_numa: usize,
}

/// Get CPU topology information
pub fn get_cpu_topology() -> CpuTopology {
    let logical_cores = get_cpu_count();
    let physical_cores = get_physical_cpu_count();

    // Simple heuristic for NUMA detection
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
    /// Base CPU frequency in Hz
    pub base_frequency: u64,
    /// Maximum CPU frequency in Hz
    pub max_frequency: u64,
    /// Current CPU frequency in Hz
    pub current_frequency: u64,
}

/// Get CPU frequency information
pub fn get_cpu_frequency() -> Result<CpuFrequency> {
    // Placeholder implementation
    Ok(CpuFrequency {
        base_frequency: 2_400_000_000, // 2.4 GHz
        max_frequency: 3_600_000_000, // 3.6 GHz
        current_frequency: 2_800_000_000, // 2.8 GHz
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
    thread::yield_now();
}

/// Sleep for a very short duration (microseconds)
pub fn micro_sleep(micros: u64) {
    thread::sleep(std::time::Duration::from_micros(micros));
}

/// Sleep for nanoseconds (may not be precise)
pub fn nano_sleep(nanos: u64) {
    thread::sleep(std::time::Duration::from_nanos(nanos));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cpu_count() {
        let count = get_cpu_count();
        assert!(count > 0);
        assert!(count <= 1024); // Reasonable upper bound
    }

    #[test]
    fn test_physical_cpu_count() {
        let count = get_physical_cpu_count();
        assert!(count > 0);
        assert!(count <= get_cpu_count());
    }

    #[test]
    fn test_cpu_topology() {
        let topology = get_cpu_topology();
        assert!(topology.logical_cores > 0);
        assert!(topology.physical_cores > 0);
        assert!(topology.numa_nodes > 0);
        assert!(topology.cores_per_numa > 0);
    }

    #[test]
    fn test_cpu_affinity() {
        let affinity = get_cpu_affinity().unwrap();
        assert!(!affinity.is_empty());

        // Test setting affinity - handle platform differences
        let affinity_result = set_cpu_affinity(0);

        #[cfg(target_arch = "aarch64")]
        {
            // Apple Silicon: Should return error
            assert!(affinity_result.is_err());
        }

        #[cfg(not(target_arch = "aarch64"))]
        {
            // Other platforms: Should succeed
            assert!(affinity_result.is_ok());
        }

        let mask_result = set_cpu_affinity_mask(&[0, 1]);

        #[cfg(target_arch = "aarch64")]
        {
            // Apple Silicon: Should return error
            assert!(mask_result.is_err());
        }

        #[cfg(not(target_arch = "aarch64"))]
        {
            // Other platforms: Should succeed
            assert!(mask_result.is_ok());
        }
    }

    #[test]
    fn test_pin_to_cpu() {
        let cpu_count = get_cpu_count();

        // Valid CPU ID - handle platform differences
        let pin_result = pin_to_cpu(0);

        #[cfg(target_arch = "aarch64")]
        {
            // Apple Silicon: Should return error
            assert!(pin_result.is_err());
        }

        #[cfg(not(target_arch = "aarch64"))]
        {
            // Other platforms: Should succeed
            assert!(pin_result.is_ok());
        }

        // Invalid CPU ID - should always fail regardless of platform
        assert!(pin_to_cpu(cpu_count).is_err());
    }
}

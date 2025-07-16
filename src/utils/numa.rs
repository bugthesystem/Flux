//! NUMA topology utilities

use std::ptr;
use crate::error::{ Result, FluxError };

/// NUMA-aware memory allocation (simplified for macOS)
pub struct NumaAllocator {
    /// Current CPU core
    current_cpu: u32,
    /// Total CPU cores
    num_cpus: u32,
}

impl NumaAllocator {
    /// Create a new NUMA allocator
    pub fn new() -> Result<Self> {
        // On macOS, we'll use CPU-based allocation instead of NUMA
        let num_cpus = num_cpus::get() as u32;
        let current_cpu = 0; // Will be set by pinning

        Ok(Self {
            current_cpu,
            num_cpus,
        })
    }

    /// Allocate memory with CPU affinity (simplified NUMA)
    pub fn allocate_with_cpu_affinity(&self, size: usize, cpu_id: u32) -> Result<*mut u8> {
        if cpu_id >= self.num_cpus {
            return Err(FluxError::RingBufferFull);
        }

        // Allocate memory normally (macOS handles NUMA automatically)
        let ptr = unsafe {
            libc::mmap(
                ptr::null_mut(),
                size,
                libc::PROT_READ | libc::PROT_WRITE,
                libc::MAP_PRIVATE | libc::MAP_ANONYMOUS,
                -1,
                0
            )
        };

        if ptr == libc::MAP_FAILED {
            return Err(FluxError::RingBufferFull);
        }

        // Lock memory to prevent swapping
        unsafe {
            let _ = libc::mlock(ptr, size);
        }

        // Set CPU affinity for the current thread
        // This helps with NUMA locality on macOS
        let _ = crate::utils::pin_to_cpu(cpu_id as usize);

        Ok(ptr as *mut u8)
    }

    /// Allocate memory from the local CPU
    pub fn allocate_local(&self, size: usize) -> Result<*mut u8> {
        self.allocate_with_cpu_affinity(size, self.current_cpu)
    }

    /// Get the CPU core for allocation
    pub fn get_cpu_for_allocation(&self, numa_node: i32) -> Result<u32> {
        // Simple mapping: numa_node 0 -> CPU 0-3, numa_node 1 -> CPU 4-7, etc.
        let cpus_per_node = self.num_cpus / 2; // Assume 2 NUMA nodes
        let cpu_id = (numa_node as u32) * cpus_per_node;

        if cpu_id >= self.num_cpus {
            return Err(FluxError::RingBufferFull);
        }

        Ok(cpu_id)
    }

    /// Get the current CPU
    pub fn current_cpu(&self) -> u32 {
        self.current_cpu
    }

    /// Get the total number of CPUs
    pub fn num_cpus(&self) -> u32 {
        self.num_cpus
    }
}

/// Allocate memory with CPU affinity
pub fn allocate_with_cpu_affinity(size: usize, cpu_id: u32) -> Result<*mut u8> {
    let allocator = NumaAllocator::new()?;
    allocator.allocate_with_cpu_affinity(size, cpu_id)
}

/// Allocate memory from the local CPU
pub fn allocate_local_cpu(size: usize) -> Result<*mut u8> {
    let allocator = NumaAllocator::new()?;
    allocator.allocate_local(size)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_numa_allocator() {
        let allocator = NumaAllocator::new().unwrap();
        assert_eq!(allocator.num_cpus() > 0, true);
    }
}

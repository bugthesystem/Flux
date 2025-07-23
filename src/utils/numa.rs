//! Cross-platform NUMA and CPU affinity utilities
//!
//! This module provides a unified API for NUMA-aware memory allocation and CPU affinity.
//! On Linux, it uses libnuma and advanced features. On macOS/other, it provides a simplified fallback.

use std::ptr;
use crate::error::{ Result, FluxError };

#[cfg(all(target_os = "linux", feature = "linux_numa"))]
use libnuma_sys::*;
#[cfg(all(target_os = "linux", feature = "linux_affinity"))]
use libc::{ cpu_set_t, sched_setaffinity, CPU_ZERO, CPU_SET, SCHED_OTHER };

/// Unified NUMA/affinity manager
pub struct NumaManager {
    #[cfg(all(target_os = "linux", feature = "linux_numa"))]
    num_nodes: i32,
    #[cfg(all(target_os = "linux", feature = "linux_numa"))]
    current_node: i32,
    #[cfg(not(all(target_os = "linux", feature = "linux_numa")))]
    current_cpu: u32,
    #[cfg(not(all(target_os = "linux", feature = "linux_numa")))]
    num_cpus: u32,
}

impl NumaManager {
    /// Create a new NUMA/affinity manager
    pub fn new() -> Option<Self> {
        #[cfg(all(target_os = "linux", feature = "linux_numa"))]
        unsafe {
            if numa_available() < 0 {
                return None;
            }
            let num_nodes = numa_num_configured_nodes();
            if num_nodes <= 0 {
                return None;
            }
            Some(Self { num_nodes, current_node: 0 })
        }
        #[cfg(not(all(target_os = "linux", feature = "linux_numa")))]
        {
            let num_cpus = num_cpus::get() as u32;
            let current_cpu = 0;
            Some(Self { current_cpu, num_cpus })
        }
    }

    /// Get the number of NUMA nodes (Linux) or CPUs (fallback)
    pub fn num_nodes(&self) -> u32 {
        #[cfg(all(target_os = "linux", feature = "linux_numa"))]
        {
            self.num_nodes as u32
        }
        #[cfg(not(all(target_os = "linux", feature = "linux_numa")))]
        {
            1
        }
    }

    /// Allocate memory on a specific NUMA node (Linux) or with CPU affinity (fallback)
    pub fn allocate_on_node(&self, size: usize, node: u32) -> Result<*mut u8> {
        #[cfg(all(target_os = "linux", feature = "linux_numa"))]
        unsafe {
            let ptr = numa_alloc_onnode(size, node as i32);
            if ptr.is_null() {
                // Fallback to regular allocation
                let fallback = libc::malloc(size) as *mut u8;
                if fallback.is_null() {
                    return Err(FluxError::system_resource("Failed to allocate memory"));
                }
                Ok(fallback)
            } else {
                Ok(ptr as *mut u8)
            }
        }
        #[cfg(not(all(target_os = "linux", feature = "linux_numa")))]
        {
            if node >= self.num_cpus {
                return Err(FluxError::RingBufferFull);
            }
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
            unsafe {
                let _ = libc::mlock(ptr, size);
            }
            let _ = crate::utils::pin_to_cpu(node as usize);
            Ok(ptr as *mut u8)
        }
    }

    /// Free NUMA-allocated memory (Linux) or fallback
    pub fn free(&self, ptr: *mut u8, size: usize) {
        #[cfg(all(target_os = "linux", feature = "linux_numa"))]
        unsafe {
            numa_free(ptr as *mut libc::c_void, size);
        }
        #[cfg(not(all(target_os = "linux", feature = "linux_numa")))]
        unsafe {
            libc::munmap(ptr as *mut libc::c_void, size);
        }
    }

    /// Get the next NUMA node (Linux) or CPU (fallback)
    pub fn next_node(&mut self) -> u32 {
        #[cfg(all(target_os = "linux", feature = "linux_numa"))]
        {
            let node = self.current_node;
            self.current_node = (self.current_node + 1) % self.num_nodes;
            node as u32
        }
        #[cfg(not(all(target_os = "linux", feature = "linux_numa")))]
        {
            let cpu = self.current_cpu;
            self.current_cpu = (self.current_cpu + 1) % self.num_cpus;
            cpu
        }
    }

    /// Get the NUMA node for a CPU (Linux) or always 0 (fallback)
    pub fn node_of_cpu(&self, cpu: u32) -> u32 {
        #[cfg(all(target_os = "linux", feature = "linux_numa"))]
        unsafe {
            numa_node_of_cpu(cpu as i32) as u32
        }
        #[cfg(not(all(target_os = "linux", feature = "linux_numa")))]
        {
            0
        }
    }
}

/// Pin a thread to a specific CPU core
pub fn pin_to_cpu(cpu: usize) -> Result<()> {
    #[cfg(all(target_os = "linux", feature = "linux_affinity"))]
    unsafe {
        let mut cpuset: cpu_set_t = std::mem::zeroed();
        CPU_ZERO(&mut cpuset);
        CPU_SET(cpu as libc::c_uint, &mut cpuset);
        let result = sched_setaffinity(0, std::mem::size_of::<cpu_set_t>(), &cpuset);
        if result == 0 {
            Ok(())
        } else {
            Err(FluxError::Io(std::io::Error::last_os_error()))
        }
    }
    #[cfg(not(all(target_os = "linux", feature = "linux_affinity")))]
    {
        Ok(())
    }
}

/// Set thread priority to maximum
pub fn set_max_priority() -> Result<()> {
    #[cfg(all(target_os = "linux", feature = "linux_affinity"))]
    {
        use libc::{ sched_param, sched_setscheduler, SCHED_FIFO };
        unsafe {
            let mut param: sched_param = std::mem::zeroed();
            param.sched_priority = 99;
            let result = sched_setscheduler(0, SCHED_FIFO, &param);
            if result == 0 {
                Ok(())
            } else {
                Err(FluxError::Io(std::io::Error::last_os_error()))
            }
        }
    }
    #[cfg(not(all(target_os = "linux", feature = "linux_affinity")))]
    {
        Ok(())
    }
}

/// Lock memory to prevent paging
pub fn lock_memory(ptr: *mut u8, size: usize) -> Result<()> {
    #[cfg(all(target_os = "linux", feature = "linux_hugepages"))]
    {
        use libc::{ mlock };
        unsafe {
            let result = mlock(ptr as *const libc::c_void, size);
            if result == 0 {
                Ok(())
            } else {
                Err(FluxError::Io(std::io::Error::last_os_error()))
            }
        }
    }
    #[cfg(not(all(target_os = "linux", feature = "linux_hugepages")))]
    {
        Ok(())
    }
}

/// Allocate memory using huge pages (Linux) or fallback
pub fn allocate_huge_pages(size: usize) -> Result<*mut u8> {
    #[cfg(all(target_os = "linux", feature = "linux_hugepages"))]
    {
        use libc::{ mmap, MAP_ANONYMOUS, MAP_PRIVATE, MAP_HUGETLB, PROT_READ, PROT_WRITE };
        unsafe {
            let ptr = mmap(
                ptr::null_mut(),
                size,
                PROT_READ | PROT_WRITE,
                MAP_PRIVATE | MAP_ANONYMOUS | MAP_HUGETLB,
                -1,
                0
            );
            if ptr == libc::MAP_FAILED {
                Err(FluxError::Io(std::io::Error::last_os_error()))
            } else {
                Ok(ptr as *mut u8)
            }
        }
    }
    #[cfg(not(all(target_os = "linux", feature = "linux_hugepages")))]
    {
        unsafe {
            let ptr = libc::malloc(size);
            if ptr.is_null() {
                Err(FluxError::Io(std::io::Error::new(std::io::ErrorKind::Other, "malloc failed")))
            } else {
                Ok(ptr as *mut u8)
            }
        }
    }
}

/// Allocate memory with CPU affinity (cross-platform)
pub fn allocate_with_cpu_affinity(size: usize, cpu_id: u32) -> Result<*mut u8> {
    let manager = NumaManager::new().ok_or_else(|| FluxError::config("NUMA/affinity unavailable"))?;
    manager.allocate_on_node(size, cpu_id)
}

/// Allocate memory from the local CPU (cross-platform)
pub fn allocate_local_cpu(size: usize) -> Result<*mut u8> {
    let manager = NumaManager::new().ok_or_else(|| FluxError::config("NUMA/affinity unavailable"))?;
    #[cfg(all(target_os = "linux", feature = "linux_numa"))]
    {
        manager.allocate_on_node(size, manager.current_node as u32)
    }
    #[cfg(not(all(target_os = "linux", feature = "linux_numa")))]
    {
        manager.allocate_on_node(size, manager.current_cpu)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_numa_manager() {
        let manager = NumaManager::new();
        assert!(manager.is_some());
        let manager = manager.unwrap();
        assert_eq!(manager.num_nodes() > 0, true);
    }
}

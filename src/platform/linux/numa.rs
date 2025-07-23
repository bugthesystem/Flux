//! Linux-specific NUMA utilities

use crate::error::{ Result, FluxError };

#[cfg(feature = "linux_numa")]
use libnuma_sys::*;
#[cfg(feature = "linux_affinity")]
use libc::{ cpu_set_t, sched_setaffinity, CPU_ZERO, CPU_SET, SCHED_OTHER };

/// Linux NUMA/affinity manager
pub struct NumaManager {
    #[cfg(feature = "linux_numa")]
    num_nodes: i32,
    #[cfg(feature = "linux_numa")]
    current_node: i32,
}

impl NumaManager {
    pub fn new() -> Option<Self> {
        #[cfg(feature = "linux_numa")]
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
        #[cfg(not(feature = "linux_numa"))]
        {
            None
        }
    }

    pub fn num_nodes(&self) -> u32 {
        #[cfg(feature = "linux_numa")]
        {
            self.num_nodes as u32
        }
        #[cfg(not(feature = "linux_numa"))]
        {
            1
        }
    }

    pub fn allocate_on_node(&self, _size: usize, _node: u32) -> Result<*mut u8> {
        #[cfg(feature = "linux_numa")]
        unsafe {
            let ptr = numa_alloc_onnode(size, node as i32);
            if ptr.is_null() {
                let fallback = libc::malloc(size) as *mut u8;
                if fallback.is_null() {
                    return Err(FluxError::system_resource("Failed to allocate memory"));
                }
                Ok(fallback)
            } else {
                Ok(ptr as *mut u8)
            }
        }
        #[cfg(not(feature = "linux_numa"))]
        {
            Err(FluxError::config("NUMA not available"))
        }
    }

    pub fn free(&self, ptr: *mut u8, size: usize) {
        #[cfg(feature = "linux_numa")]
        unsafe {
            numa_free(ptr as *mut libc::c_void, size);
        }
        #[cfg(not(feature = "linux_numa"))]
        unsafe {
            libc::munmap(ptr as *mut libc::c_void, size);
        }
    }

    pub fn next_node(&mut self) -> u32 {
        #[cfg(feature = "linux_numa")]
        {
            let node = self.current_node;
            self.current_node = (self.current_node + 1) % self.num_nodes;
            node as u32
        }
        #[cfg(not(feature = "linux_numa"))]
        {
            0
        }
    }

    pub fn node_of_cpu(&self, _cpu: u32) -> u32 {
        #[cfg(feature = "linux_numa")]
        unsafe {
            numa_node_of_cpu(cpu as i32) as u32
        }
        #[cfg(not(feature = "linux_numa"))]
        {
            0
        }
    }
}

/// Pin a thread to a specific CPU core (Linux)
#[cfg(feature = "linux_affinity")]
pub fn pin_to_cpu(cpu: usize) -> Result<()> {
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
}

/// Set thread priority to maximum (Linux)
#[cfg(feature = "linux_affinity")]
pub fn set_max_priority() -> Result<()> {
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

/// Allocate memory with CPU affinity (Linux)
pub fn allocate_with_cpu_affinity(size: usize, cpu_id: u32) -> Result<*mut u8> {
    let manager = NumaManager::new().ok_or_else(|| FluxError::config("NUMA/affinity unavailable"))?;
    manager.allocate_on_node(size, cpu_id)
}

/// Allocate memory from the local NUMA node (Linux)
pub fn allocate_local_cpu(size: usize) -> Result<*mut u8> {
    let manager = NumaManager::new().ok_or_else(|| FluxError::config("NUMA/affinity unavailable"))?;
    manager.allocate_on_node(size, 0)
}

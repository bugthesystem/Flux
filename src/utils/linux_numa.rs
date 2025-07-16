#[cfg(all(target_os = "linux", feature = "linux_numa"))]
use libnuma_sys::*;
#[cfg(all(target_os = "linux", feature = "linux_affinity"))]
use libc::{ cpu_set_t, sched_setaffinity, CPU_ZERO, CPU_SET, SCHED_OTHER };
use std::ptr;

/// Linux-specific NUMA optimizations
pub struct LinuxNumaOptimizer {
    num_nodes: i32,
    current_node: i32,
}

impl LinuxNumaOptimizer {
    /// Create a new NUMA optimizer
    #[cfg(all(target_os = "linux", feature = "linux_numa"))]
    pub fn new() -> Option<Self> {
        unsafe {
            if numa_available() < 0 {
                return None;
            }

            let num_nodes = numa_num_configured_nodes();
            if num_nodes <= 0 {
                return None;
            }

            Some(Self {
                num_nodes,
                current_node: 0,
            })
        }
    }

    #[cfg(not(all(target_os = "linux", feature = "linux_numa")))]
    pub fn new() -> Option<Self> {
        None
    }

    /// Get the number of NUMA nodes
    #[cfg(all(target_os = "linux", feature = "linux_numa"))]
    pub fn num_nodes(&self) -> i32 {
        self.num_nodes
    }

    #[cfg(not(all(target_os = "linux", feature = "linux_numa")))]
    pub fn num_nodes(&self) -> i32 {
        1
    }

    /// Allocate memory on a specific NUMA node
    #[cfg(all(target_os = "linux", feature = "linux_numa"))]
    pub fn allocate_on_node(&self, size: usize, node: i32) -> *mut u8 {
        unsafe {
            let ptr = numa_alloc_onnode(size, node);
            if ptr.is_null() {
                // Fallback to regular allocation
                libc::malloc(size) as *mut u8
            } else {
                ptr as *mut u8
            }
        }
    }

    #[cfg(not(all(target_os = "linux", feature = "linux_numa")))]
    pub fn allocate_on_node(&self, _size: usize, _node: i32) -> *mut u8 {
        // Fallback to regular allocation
        unsafe {
            libc::malloc(0) as *mut u8
        }
    }

    /// Free NUMA-allocated memory
    #[cfg(all(target_os = "linux", feature = "linux_numa"))]
    pub fn free(&self, ptr: *mut u8) {
        unsafe {
            numa_free(ptr as *mut libc::c_void, 0);
        }
    }

    #[cfg(not(all(target_os = "linux", feature = "linux_numa")))]
    pub fn free(&self, _ptr: *mut u8) {
        // No-op for non-Linux
    }

    /// Get the next NUMA node (round-robin)
    pub fn next_node(&mut self) -> i32 {
        let node = self.current_node;
        self.current_node = (self.current_node + 1) % self.num_nodes;
        node
    }

    /// Get the NUMA node for a CPU
    #[cfg(all(target_os = "linux", feature = "linux_numa"))]
    pub fn node_of_cpu(&self, cpu: i32) -> i32 {
        unsafe { numa_node_of_cpu(cpu) }
    }

    #[cfg(not(all(target_os = "linux", feature = "linux_numa")))]
    pub fn node_of_cpu(&self, _cpu: i32) -> i32 {
        0
    }
}

/// Pin a thread to a specific CPU core
#[cfg(all(target_os = "linux", feature = "linux_affinity"))]
pub fn pin_to_cpu(cpu: usize) -> Result<(), std::io::Error> {
    unsafe {
        let mut cpuset: cpu_set_t = std::mem::zeroed();
        CPU_ZERO(&mut cpuset);
        CPU_SET(cpu as libc::c_uint, &mut cpuset);

        let result = sched_setaffinity(
            0, // Current thread
            std::mem::size_of::<cpu_set_t>(),
            &cpuset
        );

        if result == 0 {
            Ok(())
        } else {
            Err(std::io::Error::last_os_error())
        }
    }
}

#[cfg(not(all(target_os = "linux", feature = "linux_affinity")))]
pub fn pin_to_cpu(_cpu: usize) -> Result<(), std::io::Error> {
    // No-op for non-Linux
    Ok(())
}

/// Set thread priority to maximum
#[cfg(all(target_os = "linux", feature = "linux_affinity"))]
pub fn set_max_priority() -> Result<(), std::io::Error> {
    use libc::{ sched_param, sched_setscheduler, SCHED_FIFO };

    unsafe {
        let mut param: sched_param = std::mem::zeroed();
        param.sched_priority = 99; // Maximum priority

        let result = sched_setscheduler(
            0, // Current thread
            SCHED_FIFO,
            &param
        );

        if result == 0 {
            Ok(())
        } else {
            Err(std::io::Error::last_os_error())
        }
    }
}

#[cfg(not(all(target_os = "linux", feature = "linux_affinity")))]
pub fn set_max_priority() -> Result<(), std::io::Error> {
    // No-op for non-Linux
    Ok(())
}

/// Lock memory to prevent paging
#[cfg(all(target_os = "linux", feature = "linux_hugepages"))]
pub fn lock_memory(ptr: *mut u8, size: usize) -> Result<(), std::io::Error> {
    use libc::{ mlock, munlock };

    unsafe {
        let result = mlock(ptr as *const libc::c_void, size);
        if result == 0 {
            Ok(())
        } else {
            Err(std::io::Error::last_os_error())
        }
    }
}

#[cfg(not(all(target_os = "linux", feature = "linux_hugepages")))]
pub fn lock_memory(_ptr: *mut u8, _size: usize) -> Result<(), std::io::Error> {
    // No-op for non-Linux
    Ok(())
}

/// Allocate memory using huge pages
#[cfg(all(target_os = "linux", feature = "linux_hugepages"))]
pub fn allocate_huge_pages(size: usize) -> Result<*mut u8, std::io::Error> {
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
            Err(std::io::Error::last_os_error())
        } else {
            Ok(ptr as *mut u8)
        }
    }
}

#[cfg(not(all(target_os = "linux", feature = "linux_hugepages")))]
pub fn allocate_huge_pages(_size: usize) -> Result<*mut u8, std::io::Error> {
    // Fallback to regular allocation
    unsafe {
        let ptr = libc::malloc(0);
        if ptr.is_null() {
            Err(std::io::Error::new(std::io::ErrorKind::Other, "malloc failed"))
        } else {
            Ok(ptr as *mut u8)
        }
    }
}

use crate::error::Result;

/// System page size
pub const PAGE_SIZE: usize = 4096;

#[cfg(target_os = "linux")]
pub fn pin_to_cpu(cpu_id: usize) -> Result<()> {
    use libc::{cpu_set_t, sched_setaffinity, CPU_SET, CPU_ZERO};
    use std::mem;

    unsafe {
        let mut cpu_set: cpu_set_t = mem::zeroed();
        CPU_ZERO(&mut cpu_set);
        CPU_SET(cpu_id, &mut cpu_set);
        
        if sched_setaffinity(0, mem::size_of::<cpu_set_t>(), &cpu_set) != 0 {
            return Err(crate::error::FluxError::system_resource("Failed to set CPU affinity"));
        }
    }
    Ok(())
}

#[cfg(target_os = "macos")]
pub fn pin_to_cpu(cpu_id: usize) -> Result<()> {
    use libc::{pthread_self, thread_affinity_policy_data_t, thread_policy_set};
    use libc::{THREAD_AFFINITY_POLICY, mach_port_t};
    
    unsafe {
        let mut policy = thread_affinity_policy_data_t {
            affinity_tag: cpu_id as i32,
        };
        
        let result = thread_policy_set(
            pthread_self() as mach_port_t,
            THREAD_AFFINITY_POLICY as u32,
            &mut policy as *mut _ as *mut i32,
            1,
        );
        
        if result != 0 {
            return Err(crate::error::FluxError::system_resource("Failed to set CPU affinity"));
        }
    }
    Ok(())
}

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
pub fn pin_to_cpu(_cpu_id: usize) -> Result<()> {
    Ok(())
}

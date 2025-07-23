//! Linux-specific CPU/thread utilities

#[cfg(target_os = "linux")]
pub fn linux_pin_to_cpu(cpu: usize) -> std::io::Result<()> {
    use libc::{ cpu_set_t, sched_setaffinity, CPU_ZERO, CPU_SET };
    unsafe {
        let mut cpuset: cpu_set_t = std::mem::zeroed();
        CPU_ZERO(&mut cpuset);
        CPU_SET(cpu, &mut cpuset);
        let result = sched_setaffinity(0, std::mem::size_of::<cpu_set_t>(), &cpuset);
        if result == 0 {
            Ok(())
        } else {
            Err(std::io::Error::last_os_error())
        }
    }
}

#[cfg(not(target_os = "linux"))]
pub fn linux_pin_to_cpu(_cpu: usize) -> std::io::Result<()> {
    Ok(())
}

#[cfg(target_os = "linux")]
pub fn linux_set_max_priority() -> std::io::Result<()> {
    use libc::{ sched_param, sched_setscheduler, SCHED_FIFO };
    unsafe {
        let mut param: sched_param = std::mem::zeroed();
        param.sched_priority = 99;
        let result = sched_setscheduler(0, SCHED_FIFO, &param);
        if result == 0 {
            Ok(())
        } else {
            Err(std::io::Error::last_os_error())
        }
    }
}

#[cfg(not(target_os = "linux"))]
pub fn linux_set_max_priority() -> std::io::Result<()> {
    Ok(())
}

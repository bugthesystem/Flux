//! Linux-specific optimizations: NUMA, huge pages, thread pinning, priority

#[cfg(target_os = "linux")]
pub fn linux_lock_memory(ptr: *mut u8, size: usize) -> std::io::Result<()> {
    use libc::mlock;
    let result = unsafe { mlock(ptr as *const libc::c_void, size) };
    if result == 0 {
        Ok(())
    } else {
        Err(std::io::Error::last_os_error())
    }
}

#[cfg(not(target_os = "linux"))]
pub fn linux_lock_memory(_ptr: *mut u8, _size: usize) -> std::io::Result<()> {
    Ok(())
}

#[cfg(target_os = "linux")]
pub fn linux_allocate_huge_pages(size: usize) -> std::io::Result<*mut u8> {
    use libc::{ mmap, MAP_ANONYMOUS, MAP_PRIVATE, MAP_HUGETLB, PROT_READ, PROT_WRITE };
    let ptr = unsafe {
        mmap(
            std::ptr::null_mut(),
            size,
            PROT_READ | PROT_WRITE,
            MAP_PRIVATE | MAP_ANONYMOUS | MAP_HUGETLB,
            -1,
            0
        )
    };
    if ptr == libc::MAP_FAILED {
        Err(std::io::Error::last_os_error())
    } else {
        Ok(ptr as *mut u8)
    }
}

#[cfg(not(target_os = "linux"))]
pub fn linux_allocate_huge_pages(_size: usize) -> std::io::Result<*mut u8> {
    Err(std::io::Error::new(std::io::ErrorKind::Other, "Huge pages not supported on this platform"))
}

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

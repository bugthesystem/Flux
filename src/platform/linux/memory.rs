//! Linux-specific memory utilities

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

//! macOS-specific memory utilities

use crate::error::{ Result, FluxError };
use std::ptr;

#[cfg(target_os = "macos")]
use libc::{ c_void, size_t, mlock, munlock, posix_memalign };

pub struct MemoryLocker;

impl MemoryLocker {
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
            let ptr = unsafe { libc::malloc(size) };
            if ptr.is_null() {
                return Err(FluxError::config("Failed to allocate memory"));
            }
            Ok(ptr)
        }
    }

    pub fn free_aligned(ptr: *mut c_void) {
        unsafe { libc::free(ptr) }
    }
}

pub fn get_cache_line_size() -> usize {
    128 // Apple Silicon typically has 128-byte cache lines
}

pub fn align_to_cache_line(ptr: *mut u8) -> *mut u8 {
    let cache_line_size = get_cache_line_size();
    let addr = ptr as usize;
    let aligned_addr = (addr + cache_line_size - 1) & !(cache_line_size - 1);
    aligned_addr as *mut u8
}

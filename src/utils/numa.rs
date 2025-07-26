//! Cross-platform NUMA and CPU affinity utilities

// Only the API surface and platform wiring remain here.
// All platform-specific logic is in platform/linux/numa.rs or platform/macos/numa.rs.

#[cfg(target_os = "linux")]
mod platform_impl {
    pub use crate::platform::linux::numa::*;
}
#[cfg(target_os = "macos")]
mod platform_impl {
    pub use crate::platform::macos::numa::*;
}

pub use platform_impl::*;

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

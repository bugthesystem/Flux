//! macOS-specific CPU/core/thread utilities

use crate::error::{ Result, FluxError };

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum CoreType {
    Performance,
    Efficiency,
    Unknown,
}

pub struct MacOSOptimizer {
    p_cores: usize,
    e_cores: usize,
    total_cores: usize,
    core_types: Vec<CoreType>,
}

impl MacOSOptimizer {
    pub fn new() -> Result<Self> {
        let (p_cores, e_cores, core_types) = Self::detect_core_types()?;
        let total_cores = p_cores + e_cores;
        Ok(Self { p_cores, e_cores, total_cores, core_types })
    }
    fn detect_core_types() -> Result<(usize, usize, Vec<CoreType>)> {
        let mut core_types = Vec::new();
        let mut p_cores = 0;
        let mut e_cores = 0;
        let physical_cores = num_cpus::get_physical();
        for cpu_id in 0..physical_cores {
            let core_type = if cpu_id < 8 { CoreType::Performance } else { CoreType::Efficiency };
            core_types.push(core_type);
            match core_type {
                CoreType::Performance => {
                    p_cores += 1;
                }
                CoreType::Efficiency => {
                    e_cores += 1;
                }
                CoreType::Unknown => {
                    p_cores += 1;
                }
            }
        }
        Ok((p_cores, e_cores, core_types))
    }
    pub fn p_core_count(&self) -> usize {
        self.p_cores
    }
    pub fn e_core_count(&self) -> usize {
        self.e_cores
    }
    pub fn total_core_count(&self) -> usize {
        self.total_cores
    }
    pub fn get_core_type(&self, cpu_id: usize) -> CoreType {
        if cpu_id < self.core_types.len() { self.core_types[cpu_id] } else { CoreType::Unknown }
    }
    pub fn get_p_core_cpus(&self) -> Vec<usize> {
        self.core_types
            .iter()
            .enumerate()
            .filter_map(|(cpu_id, core_type)| (
                if *core_type == CoreType::Performance {
                    Some(cpu_id)
                } else {
                    None
                }
            ))
            .collect()
    }
    pub fn get_e_core_cpus(&self) -> Vec<usize> {
        self.core_types
            .iter()
            .enumerate()
            .filter_map(|(cpu_id, core_type)| (
                if *core_type == CoreType::Efficiency {
                    Some(cpu_id)
                } else {
                    None
                }
            ))
            .collect()
    }
}

pub struct ThreadOptimizer;

impl ThreadOptimizer {
    pub fn set_max_priority() -> Result<()> {
        #[cfg(target_os = "macos")]
        {
            use libc::pthread_set_qos_class_self_np;
            use libc::qos_class_t;
            let result = unsafe {
                pthread_set_qos_class_self_np(qos_class_t::QOS_CLASS_USER_INTERACTIVE, 0)
            };
            if result != 0 {
                return Err(FluxError::config("Failed to set thread QoS"));
            }
        }
        Ok(())
    }
    // ... (other QoS methods can be added similarly)
}

pub struct ThreadAffinity;

impl ThreadAffinity {
    pub fn set_thread_affinity(cpu_id: usize) -> Result<()> {
        #[cfg(target_os = "macos")]
        {
            use libc::{
                pthread_mach_thread_np,
                mach_port_t,
                kern_return_t,
                integer_t,
                mach_msg_type_number_t,
            };
            extern "C" {
                fn thread_policy_set(
                    thread: mach_port_t,
                    flavor: u32,
                    policy_info: *const ThreadAffinityPolicyData,
                    count: mach_msg_type_number_t
                ) -> kern_return_t;
            }
            #[repr(C)]
            struct ThreadAffinityPolicyData {
                affinity_tag: integer_t,
            }
            const THREAD_AFFINITY_POLICY: u32 = 4;
            const THREAD_AFFINITY_POLICY_COUNT: u32 = 1;
            let current_thread = unsafe { pthread_mach_thread_np(libc::pthread_self()) };
            let policy_data = ThreadAffinityPolicyData { affinity_tag: cpu_id as integer_t };
            let result = unsafe {
                thread_policy_set(
                    current_thread,
                    THREAD_AFFINITY_POLICY,
                    &policy_data,
                    THREAD_AFFINITY_POLICY_COUNT
                )
            };
            match result {
                0 => Ok(()),
                46 =>
                    Err(
                        FluxError::config(
                            "Thread affinity not supported on Apple Silicon (M1/M2). Use QoS classes instead."
                        )
                    ),
                4 =>
                    Err(FluxError::config(format!("Invalid CPU ID: {} (check CPU count)", cpu_id))),
                _ =>
                    Err(
                        FluxError::config(format!("Thread affinity failed with error: {}", result))
                    ),
            }
        }
        #[cfg(not(target_os = "macos"))]
        {
            Err(FluxError::config("Thread affinity only available on macOS"))
        }
    }
    #[cfg(target_os = "macos")]
    pub fn is_supported() -> bool {
        use libc::{ mach_port_t, kern_return_t, integer_t, mach_msg_type_number_t };
        extern "C" {
            fn thread_policy_set(
                thread: mach_port_t,
                flavor: u32,
                policy_info: *const ThreadAffinityPolicyData,
                count: mach_msg_type_number_t
            ) -> kern_return_t;
            fn mach_thread_self() -> mach_port_t;
        }
        #[repr(C)]
        struct ThreadAffinityPolicyData {
            affinity_tag: integer_t,
        }
        const THREAD_AFFINITY_POLICY: u32 = 4;
        const THREAD_AFFINITY_POLICY_COUNT: u32 = 1;
        const THREAD_AFFINITY_TAG_NULL: integer_t = 0;
        let policy_data = ThreadAffinityPolicyData { affinity_tag: THREAD_AFFINITY_TAG_NULL };
        let current_thread = unsafe { mach_thread_self() };
        let result = unsafe {
            thread_policy_set(
                current_thread,
                THREAD_AFFINITY_POLICY,
                &policy_data,
                THREAD_AFFINITY_POLICY_COUNT
            )
        };
        result == 0
    }
    #[cfg(not(target_os = "macos"))]
    pub fn is_supported() -> bool {
        false
    }
    pub fn get_affinity_info() -> String {
        #[cfg(target_os = "macos")]
        {
            if Self::is_supported() {
                "Intel Mac: Thread affinity hints available (not guaranteed)".to_string()
            } else {
                "Apple Silicon: Thread affinity NOT supported. Use QoS classes instead.".to_string()
            }
        }
        #[cfg(not(target_os = "macos"))]
        {
            "Non-macOS platform".to_string()
        }
    }
}

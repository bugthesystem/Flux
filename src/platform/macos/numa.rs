//! macOS-specific NUMA utilities (stub)

use crate::error::{ Result, FluxError };

pub struct NumaManager;

impl NumaManager {
    pub fn new() -> Option<Self> {
        Some(NumaManager)
    }
    pub fn num_nodes(&self) -> u32 {
        1
    }
    pub fn allocate_on_node(&self, _size: usize, _node: u32) -> Result<*mut u8> {
        Err(FluxError::config("NUMA not supported on macOS"))
    }
    pub fn free(&self, _ptr: *mut u8, _size: usize) {}
    pub fn next_node(&mut self) -> u32 {
        0
    }
    pub fn node_of_cpu(&self, _cpu: u32) -> u32 {
        0
    }
}

pub fn allocate_with_cpu_affinity(_size: usize, _cpu_id: u32) -> Result<*mut u8> {
    Err(FluxError::config("NUMA/affinity not supported on macOS"))
}

pub fn allocate_local_cpu(_size: usize) -> Result<*mut u8> {
    Err(FluxError::config("NUMA/affinity not supported on macOS"))
}

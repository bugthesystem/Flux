//! Producer implementation for ring buffer


/// Producer for ring buffer
pub struct Producer;

impl Producer {
    /// Create new producer
    pub fn new() -> Self {
        Self
    }
}

impl Default for Producer {
    fn default() -> Self {
        Self::new()
    }
}

//! Consumer implementation for ring buffer

/// Consumer for ring buffer
pub struct Consumer {
    _consumer_id: usize,
}

impl Consumer {
    /// Create new consumer
    pub fn new(consumer_id: usize) -> Self {
        Self { _consumer_id: consumer_id }
    }
}

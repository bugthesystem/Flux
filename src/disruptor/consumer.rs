//! Consumer implementation for ring buffer


/// Consumer for ring buffer
pub struct Consumer {
    consumer_id: usize,
}

impl Consumer {
    /// Create new consumer
    pub fn new(consumer_id: usize) -> Self {
        Self { consumer_id }
    }
}

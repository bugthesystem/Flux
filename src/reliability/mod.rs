//! Reliability features for message transport


/// Reliability configuration
#[derive(Debug, Clone)]
pub struct ReliabilityConfig {
    /// Enable forward error correction
    pub enable_fec: bool,
    /// Retransmission timeout
    pub retransmit_timeout_ms: u64,
}

impl Default for ReliabilityConfig {
    fn default() -> Self {
        Self {
            enable_fec: false,
            retransmit_timeout_ms: 100,
        }
    }
}

/// Forward error correction encoder
pub struct FecEncoder {
    data_shards: usize,
    parity_shards: usize,
}

impl FecEncoder {
    /// Create new FEC encoder
    pub fn new(data_shards: usize, parity_shards: usize) -> Self {
        Self { data_shards, parity_shards }
    }
}

/// Adaptive timeout controller
pub struct AdaptiveTimeout {
    current_timeout: u64,
}

impl AdaptiveTimeout {
    /// Create new adaptive timeout
    pub fn new() -> Self {
        Self { current_timeout: 100_000 }
    }

    /// Get current timeout
    pub fn get_timeout(&self) -> u64 {
        self.current_timeout
    }
}

impl Default for AdaptiveTimeout {
    fn default() -> Self {
        Self::new()
    }
}

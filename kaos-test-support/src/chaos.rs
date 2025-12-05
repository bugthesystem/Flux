//! Chaos testing utilities - random failures, delays, corruption.
//!
//! Inspired by Netflix's Chaos Monkey and Aeron's test harnesses.

use rand::Rng;
use std::time::Duration;

/// Types of chaos events that can be injected
#[derive(Debug, Clone)]
pub enum ChaosEvent {
    /// Delay processing by a random amount
    Delay { min_us: u64, max_us: u64 },
    /// Corrupt a byte at a random position
    CorruptByte { position: Option<usize> },
    /// Corrupt multiple bytes
    CorruptBytes { count: usize },
    /// Truncate data to a random length
    Truncate { min_len: usize },
    /// Duplicate the data
    Duplicate,
    /// Reorder (swap with next)
    Reorder,
    /// No chaos - pass through
    None,
}

/// Chaos monkey for injecting random failures into tests.
///
/// # Example
///
/// ```
/// use kaos_test_support::chaos::{ChaosMonkey, ChaosEvent};
///
/// let mut monkey = ChaosMonkey::new()
///     .with_delay_probability(0.01)  // 1% chance of delay
///     .with_corruption_probability(0.001);  // 0.1% chance of corruption
///
/// let mut data = vec![1, 2, 3, 4, 5];
/// monkey.maybe_corrupt(&mut data);
/// ```
pub struct ChaosMonkey {
    delay_probability: f64,
    delay_min_us: u64,
    delay_max_us: u64,
    corruption_probability: f64,
    truncate_probability: f64,
    duplicate_probability: f64,
    reorder_probability: f64,
    rng: rand::rngs::ThreadRng,
    events_triggered: usize,
}

impl ChaosMonkey {
    pub fn new() -> Self {
        Self {
            delay_probability: 0.0,
            delay_min_us: 100,
            delay_max_us: 10_000,
            corruption_probability: 0.0,
            truncate_probability: 0.0,
            duplicate_probability: 0.0,
            reorder_probability: 0.0,
            rng: rand::thread_rng(),
            events_triggered: 0,
        }
    }

    /// Create an aggressive chaos monkey for stress testing
    pub fn aggressive() -> Self {
        Self::new()
            .with_delay_probability(0.05)
            .with_corruption_probability(0.01)
            .with_truncate_probability(0.005)
    }

    /// Create a mild chaos monkey for basic testing
    pub fn mild() -> Self {
        Self::new()
            .with_delay_probability(0.01)
            .with_corruption_probability(0.001)
    }

    pub fn with_delay_probability(mut self, prob: f64) -> Self {
        self.delay_probability = prob.clamp(0.0, 1.0);
        self
    }

    pub fn with_delay_range(mut self, min_us: u64, max_us: u64) -> Self {
        self.delay_min_us = min_us;
        self.delay_max_us = max_us;
        self
    }

    pub fn with_corruption_probability(mut self, prob: f64) -> Self {
        self.corruption_probability = prob.clamp(0.0, 1.0);
        self
    }

    pub fn with_truncate_probability(mut self, prob: f64) -> Self {
        self.truncate_probability = prob.clamp(0.0, 1.0);
        self
    }

    pub fn with_duplicate_probability(mut self, prob: f64) -> Self {
        self.duplicate_probability = prob.clamp(0.0, 1.0);
        self
    }

    pub fn with_reorder_probability(mut self, prob: f64) -> Self {
        self.reorder_probability = prob.clamp(0.0, 1.0);
        self
    }

    /// Maybe inject a delay
    pub fn maybe_delay(&mut self) {
        if self.rng.gen::<f64>() < self.delay_probability {
            let delay_us = self.rng.gen_range(self.delay_min_us..=self.delay_max_us);
            std::thread::sleep(Duration::from_micros(delay_us));
            self.events_triggered += 1;
        }
    }

    /// Maybe corrupt data in place
    pub fn maybe_corrupt(&mut self, data: &mut [u8]) -> bool {
        if data.is_empty() {
            return false;
        }

        if self.rng.gen::<f64>() < self.corruption_probability {
            let pos = self.rng.gen_range(0..data.len());
            data[pos] = self.rng.gen();
            self.events_triggered += 1;
            return true;
        }
        false
    }

    /// Maybe truncate data
    pub fn maybe_truncate(&mut self, data: &mut Vec<u8>, min_len: usize) -> bool {
        if data.len() <= min_len {
            return false;
        }

        if self.rng.gen::<f64>() < self.truncate_probability {
            let new_len = self.rng.gen_range(min_len..data.len());
            data.truncate(new_len);
            self.events_triggered += 1;
            return true;
        }
        false
    }

    /// Decide what chaos event to apply (if any)
    pub fn decide(&mut self) -> ChaosEvent {
        let roll = self.rng.gen::<f64>();
        let mut threshold = 0.0;

        threshold += self.delay_probability;
        if roll < threshold {
            self.events_triggered += 1;
            return ChaosEvent::Delay {
                min_us: self.delay_min_us,
                max_us: self.delay_max_us,
            };
        }

        threshold += self.corruption_probability;
        if roll < threshold {
            self.events_triggered += 1;
            return ChaosEvent::CorruptByte { position: None };
        }

        threshold += self.truncate_probability;
        if roll < threshold {
            self.events_triggered += 1;
            return ChaosEvent::Truncate { min_len: 8 };
        }

        threshold += self.duplicate_probability;
        if roll < threshold {
            self.events_triggered += 1;
            return ChaosEvent::Duplicate;
        }

        threshold += self.reorder_probability;
        if roll < threshold {
            self.events_triggered += 1;
            return ChaosEvent::Reorder;
        }

        ChaosEvent::None
    }

    /// Get number of chaos events triggered
    pub fn events_triggered(&self) -> usize {
        self.events_triggered
    }
}

/// Apply a chaos event to data
pub fn apply_chaos(event: &ChaosEvent, data: &mut Vec<u8>, rng: &mut impl Rng) {
    match event {
        ChaosEvent::Delay { min_us, max_us } => {
            let delay = rng.gen_range(*min_us..=*max_us);
            std::thread::sleep(Duration::from_micros(delay));
        }
        ChaosEvent::CorruptByte { position } => {
            if !data.is_empty() {
                let pos = position.unwrap_or_else(|| rng.gen_range(0..data.len()));
                if pos < data.len() {
                    data[pos] = rng.gen();
                }
            }
        }
        ChaosEvent::CorruptBytes { count } => {
            for _ in 0..*count {
                if !data.is_empty() {
                    let pos = rng.gen_range(0..data.len());
                    data[pos] = rng.gen();
                }
            }
        }
        ChaosEvent::Truncate { min_len } => {
            if data.len() > *min_len {
                let new_len = rng.gen_range(*min_len..data.len());
                data.truncate(new_len);
            }
        }
        ChaosEvent::Duplicate | ChaosEvent::Reorder | ChaosEvent::None => {
            // These are handled at a higher level (packet queue)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_no_chaos() {
        let mut monkey = ChaosMonkey::new();
        let original = vec![1, 2, 3, 4, 5];
        let mut data = original.clone();

        // With 0 probability, should never corrupt
        for _ in 0..1000 {
            monkey.maybe_corrupt(&mut data);
        }
        assert_eq!(data, original);
    }

    #[test]
    fn test_always_corrupt() {
        let mut monkey = ChaosMonkey::new().with_corruption_probability(1.0);
        let original = vec![1, 2, 3, 4, 5];
        let mut data = original.clone();

        let corrupted = monkey.maybe_corrupt(&mut data);
        assert!(corrupted);
        assert_ne!(data, original);
    }

    #[test]
    fn test_aggressive_triggers_events() {
        let mut monkey = ChaosMonkey::aggressive();
        let mut data = vec![0u8; 100];

        for _ in 0..10000 {
            monkey.maybe_corrupt(&mut data);
            monkey.maybe_delay();
        }

        // Should have triggered some events
        assert!(monkey.events_triggered() > 0);
    }
}

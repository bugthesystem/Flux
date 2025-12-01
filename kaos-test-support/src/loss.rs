//! Packet loss simulation for RUDP testing.
//!
//! Inspired by Aeron's `LossGenerator` and `PortLossGenerator`.

use rand::Rng;
use std::collections::HashSet;

/// Decision for whether to drop a packet
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DropDecision {
    Drop,
    Pass,
}

/// Pattern for packet loss simulation
#[derive(Debug, Clone)]
pub enum LossPattern {
    /// No loss - pass everything
    None,
    /// Drop every Nth packet
    Periodic { every_n: usize },
    /// Drop packets randomly with given probability (0.0-1.0)
    Random { probability: f64 },
    /// Drop a burst of packets starting at sequence
    Burst { start_seq: u64, length: usize },
    /// Drop specific sequences
    Specific { sequences: HashSet<u64> },
    /// Drop for a duration after trigger
    Timed { trigger_seq: u64, duration_packets: usize },
    /// Combination of patterns
    Combined(Vec<LossPattern>),
}

/// Generates packet loss for testing RUDP recovery.
///
/// # Example
///
/// ```
/// use kaos_test_support::loss::{LossGenerator, LossPattern, DropDecision};
///
/// let mut gen = LossGenerator::new(LossPattern::Periodic { every_n: 10 });
///
/// for seq in 0..100 {
///     match gen.should_drop(seq) {
///         DropDecision::Drop => println!("Dropping packet {}", seq),
///         DropDecision::Pass => println!("Passing packet {}", seq),
///     }
/// }
/// ```
pub struct LossGenerator {
    pattern: LossPattern,
    packet_count: usize,
    rng: rand::rngs::ThreadRng,
    // For burst tracking
    burst_dropped: usize,
    // For timed tracking
    timed_triggered: bool,
    timed_remaining: usize,
}

impl LossGenerator {
    pub fn new(pattern: LossPattern) -> Self {
        Self {
            pattern,
            packet_count: 0,
            rng: rand::thread_rng(),
            burst_dropped: 0,
            timed_triggered: false,
            timed_remaining: 0,
        }
    }

    /// Create a generator that drops nothing
    pub fn none() -> Self {
        Self::new(LossPattern::None)
    }

    /// Create a generator that drops every Nth packet
    pub fn periodic(every_n: usize) -> Self {
        Self::new(LossPattern::Periodic { every_n })
    }

    /// Create a generator with random loss probability
    pub fn random(probability: f64) -> Self {
        Self::new(LossPattern::Random { probability: probability.clamp(0.0, 1.0) })
    }

    /// Create a generator that drops a burst of packets
    pub fn burst(start_seq: u64, length: usize) -> Self {
        Self::new(LossPattern::Burst { start_seq, length })
    }

    /// Create a generator that drops specific sequences
    pub fn specific(sequences: impl IntoIterator<Item = u64>) -> Self {
        Self::new(LossPattern::Specific {
            sequences: sequences.into_iter().collect(),
        })
    }

    /// Decide whether to drop a packet at the given sequence
    pub fn should_drop(&mut self, sequence: u64) -> DropDecision {
        self.packet_count += 1;
        self.check_pattern(sequence, &self.pattern.clone())
    }

    fn check_pattern(&mut self, sequence: u64, pattern: &LossPattern) -> DropDecision {
        match pattern {
            LossPattern::None => DropDecision::Pass,

            LossPattern::Periodic { every_n } => {
                if *every_n > 0 && self.packet_count % every_n == 0 {
                    DropDecision::Drop
                } else {
                    DropDecision::Pass
                }
            }

            LossPattern::Random { probability } => {
                if self.rng.gen::<f64>() < *probability {
                    DropDecision::Drop
                } else {
                    DropDecision::Pass
                }
            }

            LossPattern::Burst { start_seq, length } => {
                if sequence >= *start_seq && sequence < start_seq + (*length as u64) {
                    self.burst_dropped += 1;
                    DropDecision::Drop
                } else {
                    DropDecision::Pass
                }
            }

            LossPattern::Specific { sequences } => {
                if sequences.contains(&sequence) {
                    DropDecision::Drop
                } else {
                    DropDecision::Pass
                }
            }

            LossPattern::Timed { trigger_seq, duration_packets } => {
                if sequence == *trigger_seq {
                    self.timed_triggered = true;
                    self.timed_remaining = *duration_packets;
                }

                if self.timed_triggered && self.timed_remaining > 0 {
                    self.timed_remaining -= 1;
                    DropDecision::Drop
                } else {
                    DropDecision::Pass
                }
            }

            LossPattern::Combined(patterns) => {
                for p in patterns {
                    if self.check_pattern(sequence, p) == DropDecision::Drop {
                        return DropDecision::Drop;
                    }
                }
                DropDecision::Pass
            }
        }
    }

    /// Get statistics
    pub fn stats(&self) -> LossStats {
        LossStats {
            total_packets: self.packet_count,
            burst_dropped: self.burst_dropped,
        }
    }
}

#[derive(Debug, Clone)]
pub struct LossStats {
    pub total_packets: usize,
    pub burst_dropped: usize,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_no_loss() {
        let mut gen = LossGenerator::none();
        for seq in 0..100 {
            assert_eq!(gen.should_drop(seq), DropDecision::Pass);
        }
    }

    #[test]
    fn test_periodic_loss() {
        let mut gen = LossGenerator::periodic(10);
        let mut drops = 0;
        for seq in 0..100 {
            if gen.should_drop(seq) == DropDecision::Drop {
                drops += 1;
            }
        }
        assert_eq!(drops, 10); // 10, 20, 30, ..., 100
    }

    #[test]
    fn test_burst_loss() {
        let mut gen = LossGenerator::burst(50, 10);
        let mut drops = vec![];
        for seq in 0..100 {
            if gen.should_drop(seq) == DropDecision::Drop {
                drops.push(seq);
            }
        }
        assert_eq!(drops, (50..60).collect::<Vec<_>>());
    }

    #[test]
    fn test_specific_loss() {
        let mut gen = LossGenerator::specific([5, 15, 25, 99]);
        let mut drops = vec![];
        for seq in 0..100 {
            if gen.should_drop(seq) == DropDecision::Drop {
                drops.push(seq);
            }
        }
        assert_eq!(drops, vec![5, 15, 25, 99]);
    }

    #[test]
    fn test_random_loss() {
        let mut gen = LossGenerator::random(0.1); // 10% loss
        let mut drops = 0;
        for seq in 0..10000 {
            if gen.should_drop(seq) == DropDecision::Drop {
                drops += 1;
            }
        }
        // Should be roughly 1000 ± some variance
        assert!(drops > 800 && drops < 1200, "drops = {}", drops);
    }
}


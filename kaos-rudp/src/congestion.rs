//! Congestion Control (AIMD)
//!
//! Additive Increase Multiplicative Decrease for network fairness.

use std::time::{Duration, Instant};

/// AIMD congestion controller
pub struct CongestionController {
    /// Current window size (packets)
    pub window: u32,
    /// Minimum window
    min_window: u32,
    /// Maximum window
    max_window: u32,
    /// Slow start threshold
    ssthresh: u32,
    /// RTT estimate (microseconds)
    rtt_us: u64,
    /// Last loss time
    last_loss: Instant,
    /// Packets in flight
    in_flight: u32,
}

impl CongestionController {
    pub fn new(initial_window: u32, max_window: u32) -> Self {
        Self {
            window: initial_window,
            min_window: 4,
            max_window,
            ssthresh: max_window / 2,
            rtt_us: 1000, // 1ms initial
            last_loss: Instant::now(),
            in_flight: 0,
        }
    }

    /// Can we send more packets?
    #[inline]
    pub fn can_send(&self) -> bool {
        self.in_flight < self.window
    }

    /// Record packet sent
    #[inline]
    pub fn on_send(&mut self) {
        self.in_flight = self.in_flight.saturating_add(1);
    }

    /// Record ACK received (additive increase)
    #[inline]
    pub fn on_ack(&mut self) {
        self.in_flight = self.in_flight.saturating_sub(1);
        
        if self.window < self.ssthresh {
            // Slow start: exponential increase
            self.window = (self.window + 1).min(self.max_window);
        } else {
            // Congestion avoidance: additive increase (1 per RTT)
            // Approximate: increase by 1/window per ACK
            if self.window < self.max_window {
                self.window += 1;
            }
        }
    }

    /// Record loss (multiplicative decrease)
    pub fn on_loss(&mut self) {
        // Don't decrease too frequently (at most once per RTT)
        if self.last_loss.elapsed() > Duration::from_micros(self.rtt_us) {
            self.ssthresh = (self.window / 2).max(self.min_window);
            self.window = self.ssthresh;
            self.last_loss = Instant::now();
        }
    }

    /// Update RTT estimate
    pub fn update_rtt(&mut self, sample_us: u64) {
        // EWMA: rtt = 0.875 * rtt + 0.125 * sample
        self.rtt_us = (self.rtt_us * 7 + sample_us) / 8;
    }

    /// Get current window
    pub fn window_size(&self) -> u32 { self.window }

    /// Get packets in flight
    pub fn in_flight(&self) -> u32 { self.in_flight }

    /// Get RTT estimate
    pub fn rtt_us(&self) -> u64 { self.rtt_us }
}

impl Default for CongestionController {
    fn default() -> Self {
        Self::new(64, 65536)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_aimd() {
        let mut cc = CongestionController::new(10, 100);
        
        // Slow start
        for _ in 0..5 {
            cc.on_send();
            cc.on_ack();
        }
        assert!(cc.window > 10);
        
        // Loss
        let before = cc.window;
        std::thread::sleep(Duration::from_millis(2));
        cc.on_loss();
        assert!(cc.window < before);
    }
}


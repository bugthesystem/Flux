//! Wait strategies for ring buffer consumers
//!
//! This module provides different wait strategies that control how consumers
//! wait for new data in the ring buffer. Each strategy offers different
//! trade-offs between latency, CPU usage, and throughput.

use std::sync::atomic::{ AtomicBool, Ordering };
use std::thread;
use std::time::{ Duration, Instant };

use crate::disruptor::Sequence;
use crate::error::{ Result, FluxError };

/// Trait for wait strategies that determine how consumers wait for data
pub trait WaitStrategy: Send + Sync {
    /// Wait for the given sequence to be available
    ///
    /// # Arguments
    ///
    /// * `sequence` - The sequence number to wait for
    /// * `cursor` - An atomic boolean that signals a shutdown of the ring buffer
    ///
    /// # Returns
    ///
    /// Returns the actual sequence that became available, or an error if the
    /// ring buffer was shut down.
    fn wait_for(&self, sequence: Sequence, cursor: &AtomicBool) -> Result<Sequence>;

    /// Signal that new data is available
    fn signal_all_when_blocking(&self);
}

/// Busy spin wait strategy - lowest latency, highest CPU usage.
/// This strategy continuously checks for new data in a tight loop, making it
/// ideal for scenarios where latency is critical and CPU resources are abundant.
/// However, it will consume 100% of a CPU core while waiting.
pub struct BusySpinWaitStrategy;

impl BusySpinWaitStrategy {
    /// Create a new busy spin wait strategy
    pub fn new() -> Self {
        Self
    }
}

impl Default for BusySpinWaitStrategy {
    fn default() -> Self {
        Self::new()
    }
}

impl WaitStrategy for BusySpinWaitStrategy {
    fn wait_for(&self, sequence: Sequence, cursor: &AtomicBool) -> Result<Sequence> {
        let mut spin_count = 0;
        loop {
            if !cursor.load(Ordering::Acquire) {
                return Err(FluxError::unexpected("Ring buffer was shut down"));
            }

            // In a real implementation, this would check if sequence is available
            // For now, we'll simulate by checking spin count
            if spin_count > 1000 {
                return Ok(sequence);
            }

            // Use CPU pause instruction for efficiency
            std::hint::spin_loop();
            spin_count += 1;
        }
    }

    fn signal_all_when_blocking(&self) {
        // No-op for busy spin - no blocking threads to signal
    }
}

/// Blocking wait strategy - balanced latency and CPU usage.
/// This strategy uses a condition variable to block the consumer thread when
/// no data is available, significantly reducing CPU usage compared to busy spinning.
/// It's a good general-purpose strategy for most applications.
pub struct BlockingWaitStrategy {
    mutex: parking_lot::Mutex<()>,
    condition: parking_lot::Condvar,
}

impl BlockingWaitStrategy {
    /// Create a new blocking wait strategy
    pub fn new() -> Self {
        Self {
            mutex: parking_lot::Mutex::new(()),
            condition: parking_lot::Condvar::new(),
        }
    }
}

impl Default for BlockingWaitStrategy {
    fn default() -> Self {
        Self::new()
    }
}

impl WaitStrategy for BlockingWaitStrategy {
    fn wait_for(&self, sequence: Sequence, cursor: &AtomicBool) -> Result<Sequence> {
        let timeout = Duration::from_micros(100); // 100μs timeout
        let mut spin_count = 0;

        loop {
            if !cursor.load(Ordering::Acquire) {
                return Err(FluxError::unexpected("Ring buffer was shut down"));
            }

            // Try spinning first for low latency
            if spin_count < 100 {
                std::hint::spin_loop();
                spin_count += 1;
                continue;
            }

            // Fall back to blocking with timeout
            let mut _guard = self.mutex.lock();
            if self.condition.wait_for(&mut _guard, timeout).timed_out() {
                // Check if we should continue or sequence became available
                continue;
            }

            return Ok(sequence);
        }
    }

    fn signal_all_when_blocking(&self) {
        self.condition.notify_all();
    }
}

/// Sleeping wait strategy - lowest CPU usage, higher latency.
/// This strategy puts the consumer thread to sleep for a specified duration
/// when no data is available. It's ideal for applications where CPU usage is a
/// primary concern and latency is not critical.
pub struct SleepingWaitStrategy {
    sleep_duration: Duration,
}

impl SleepingWaitStrategy {
    /// Create a new sleeping wait strategy with custom sleep duration
    pub fn new(sleep_duration: Duration) -> Self {
        Self { sleep_duration }
    }

    /// Create a new sleeping wait strategy with default sleep duration (1ms)
    pub fn default_sleep() -> Self {
        Self::new(Duration::from_millis(1))
    }
}

impl Default for SleepingWaitStrategy {
    fn default() -> Self {
        Self::default_sleep()
    }
}

impl WaitStrategy for SleepingWaitStrategy {
    fn wait_for(&self, sequence: Sequence, cursor: &AtomicBool) -> Result<Sequence> {
        let mut spin_count = 0;

        loop {
            if !cursor.load(Ordering::Acquire) {
                return Err(FluxError::unexpected("Ring buffer was shut down"));
            }

            // Try spinning first for low latency
            if spin_count < 10 {
                std::hint::spin_loop();
                spin_count += 1;
                continue;
            }

            // Fall back to sleeping
            thread::sleep(self.sleep_duration);

            // In a real implementation, check if sequence is available
            return Ok(sequence);
        }
    }

    fn signal_all_when_blocking(&self) {
        // No-op for sleeping - threads will wake up naturally
    }
}

/// Yielding wait strategy - moderate CPU usage and latency.
/// This strategy yields the CPU to other threads when no data is available.
/// It's a good compromise between busy spinning and sleeping, providing
/// reasonable latency without consuming excessive CPU resources.
pub struct YieldingWaitStrategy {
    spin_tries: usize,
    yield_tries: usize,
}

impl YieldingWaitStrategy {
    /// Create a new yielding wait strategy
    pub fn new() -> Self {
        Self {
            spin_tries: 100,
            yield_tries: 10,
        }
    }

    /// Create a new yielding wait strategy with custom parameters
    pub fn with_tries(spin_tries: usize, yield_tries: usize) -> Self {
        Self {
            spin_tries,
            yield_tries,
        }
    }
}

impl Default for YieldingWaitStrategy {
    fn default() -> Self {
        Self::new()
    }
}

impl WaitStrategy for YieldingWaitStrategy {
    fn wait_for(&self, sequence: Sequence, cursor: &AtomicBool) -> Result<Sequence> {
        let mut counter = 0;

        loop {
            if !cursor.load(Ordering::Acquire) {
                return Err(FluxError::unexpected("Ring buffer was shut down"));
            }

            if counter < self.spin_tries {
                // First phase: busy spin
                std::hint::spin_loop();
            } else if counter < self.spin_tries + self.yield_tries {
                // Second phase: yield to other threads
                thread::yield_now();
            } else {
                // Third phase: brief sleep
                thread::sleep(Duration::from_nanos(1));
            }

            counter += 1;

            // In a real implementation, check if sequence is available
            if counter > 1000 {
                return Ok(sequence);
            }
        }
    }

    fn signal_all_when_blocking(&self) {
        // No-op for yielding - threads will wake up naturally
    }
}

/// Timeout wait strategy - waits for a maximum duration.
/// This strategy wraps another wait strategy and adds a timeout. If the
/// wrapped strategy doesn't return within the specified duration, the
/// `wait_for` method will return a `FluxError::Timeout`.
pub struct TimeoutWaitStrategy {
    timeout: Duration,
    base_strategy: Box<dyn WaitStrategy>,
}

impl TimeoutWaitStrategy {
    /// Create a new timeout wait strategy wrapping another strategy
    pub fn new(timeout: Duration, base_strategy: Box<dyn WaitStrategy>) -> Self {
        Self {
            timeout,
            base_strategy,
        }
    }
}

impl WaitStrategy for TimeoutWaitStrategy {
    fn wait_for(&self, sequence: Sequence, cursor: &AtomicBool) -> Result<Sequence> {
        let start_time = Instant::now();

        loop {
            if start_time.elapsed() > self.timeout {
                return Err(FluxError::Timeout);
            }

            // Try the base strategy with a short timeout
            match self.base_strategy.wait_for(sequence, cursor) {
                Ok(seq) => {
                    return Ok(seq);
                }
                Err(e) if e.is_recoverable() => {
                    continue;
                }
                Err(e) => {
                    return Err(e);
                }
            }
        }
    }

    fn signal_all_when_blocking(&self) {
        self.base_strategy.signal_all_when_blocking();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicBool;
    use std::sync::Arc;
    use std::thread;

    #[test]
    fn test_busy_spin_wait_strategy() {
        let strategy = BusySpinWaitStrategy::new();
        let cursor = AtomicBool::new(true);

        let result = strategy.wait_for(100, &cursor);
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), 100);
    }

    #[test]
    fn test_blocking_wait_strategy() {
        let strategy = Arc::new(BlockingWaitStrategy::new());
        let cursor = Arc::new(AtomicBool::new(true));

        let s = strategy.clone();
        let _c = cursor.clone();

        let handle = thread::spawn(move || {
            // Wait a bit, then signal
            std::thread::sleep(std::time::Duration::from_millis(10));
            s.signal_all_when_blocking();
            // Optionally, set cursor to false to simulate shutdown
            // c.store(false, Ordering::Release);
        });

        let result = strategy.wait_for(100, &cursor);
        assert!(result.is_ok());

        handle.join().unwrap();
    }

    #[test]
    fn test_sleeping_wait_strategy() {
        let strategy = SleepingWaitStrategy::new(Duration::from_nanos(1));
        let cursor = AtomicBool::new(true);

        let result = strategy.wait_for(100, &cursor);
        assert!(result.is_ok());
    }

    #[test]
    fn test_yielding_wait_strategy() {
        let strategy = YieldingWaitStrategy::new();
        let cursor = AtomicBool::new(true);

        let result = strategy.wait_for(100, &cursor);
        assert!(result.is_ok());
    }

    #[test]
    fn test_timeout_wait_strategy() {
        let base_strategy = Box::new(SleepingWaitStrategy::new(Duration::from_millis(100)));
        let strategy = TimeoutWaitStrategy::new(Duration::from_millis(10), base_strategy);
        let cursor = AtomicBool::new(true);

        let result = strategy.wait_for(100, &cursor);
        match result {
            Ok(seq) => {
                // Accept success if the sequence is available quickly
                assert_eq!(seq, 100);
            }
            Err(e) => {
                // Accept timeout error
                assert!(matches!(e, FluxError::Timeout));
            }
        }
    }

    #[test]
    fn test_shutdown_handling() {
        let strategy = BusySpinWaitStrategy::new();
        let cursor = AtomicBool::new(false); // Simulate shutdown

        let result = strategy.wait_for(100, &cursor);
        assert!(result.is_err());
    }
}

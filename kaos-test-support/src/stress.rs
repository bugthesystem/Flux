//! Stress testing utilities for long-duration tests.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

/// Configuration for stress tests
#[derive(Debug, Clone)]
pub struct StressConfig {
    /// Duration to run the test
    pub duration: Duration,
    /// Number of producer threads
    pub producers: usize,
    /// Number of consumer threads
    pub consumers: usize,
    /// Messages per batch
    pub batch_size: usize,
    /// Target messages per second (0 = unlimited)
    pub target_rate: u64,
    /// Print progress every N seconds
    pub report_interval: Duration,
}

impl Default for StressConfig {
    fn default() -> Self {
        Self {
            duration: Duration::from_secs(10),
            producers: 1,
            consumers: 1,
            batch_size: 100,
            target_rate: 0,
            report_interval: Duration::from_secs(1),
        }
    }
}

impl StressConfig {
    pub fn new(duration_secs: u64) -> Self {
        Self {
            duration: Duration::from_secs(duration_secs),
            ..Default::default()
        }
    }

    pub fn with_producers(mut self, n: usize) -> Self {
        self.producers = n;
        self
    }

    pub fn with_consumers(mut self, n: usize) -> Self {
        self.consumers = n;
        self
    }

    pub fn with_batch_size(mut self, size: usize) -> Self {
        self.batch_size = size;
        self
    }

    pub fn with_target_rate(mut self, rate: u64) -> Self {
        self.target_rate = rate;
        self
    }
}

/// Metrics collected during stress testing
#[derive(Debug, Clone)]
pub struct StressMetrics {
    pub messages_sent: u64,
    pub messages_received: u64,
    pub bytes_sent: u64,
    pub bytes_received: u64,
    pub errors: u64,
    pub duration: Duration,
    pub peak_rate: f64,
}

impl StressMetrics {
    pub fn new() -> Self {
        Self {
            messages_sent: 0,
            messages_received: 0,
            bytes_sent: 0,
            bytes_received: 0,
            errors: 0,
            duration: Duration::ZERO,
            peak_rate: 0.0,
        }
    }

    pub fn send_rate(&self) -> f64 {
        if self.duration.as_secs_f64() > 0.0 {
            self.messages_sent as f64 / self.duration.as_secs_f64()
        } else {
            0.0
        }
    }

    pub fn receive_rate(&self) -> f64 {
        if self.duration.as_secs_f64() > 0.0 {
            self.messages_received as f64 / self.duration.as_secs_f64()
        } else {
            0.0
        }
    }

    pub fn loss_rate(&self) -> f64 {
        if self.messages_sent > 0 {
            1.0 - (self.messages_received as f64 / self.messages_sent as f64)
        } else {
            0.0
        }
    }

    pub fn throughput_mbps(&self) -> f64 {
        if self.duration.as_secs_f64() > 0.0 {
            (self.bytes_received as f64 / 1_000_000.0) / self.duration.as_secs_f64()
        } else {
            0.0
        }
    }
}

impl Default for StressMetrics {
    fn default() -> Self {
        Self::new()
    }
}

/// Shared counters for stress testing
pub struct StressCounters {
    pub sent: AtomicU64,
    pub received: AtomicU64,
    pub bytes_sent: AtomicU64,
    pub bytes_received: AtomicU64,
    pub errors: AtomicU64,
    pub running: AtomicBool,
}

impl StressCounters {
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            sent: AtomicU64::new(0),
            received: AtomicU64::new(0),
            bytes_sent: AtomicU64::new(0),
            bytes_received: AtomicU64::new(0),
            errors: AtomicU64::new(0),
            running: AtomicBool::new(true),
        })
    }

    pub fn record_send(&self, bytes: usize) {
        self.sent.fetch_add(1, Ordering::Relaxed);
        self.bytes_sent.fetch_add(bytes as u64, Ordering::Relaxed);
    }

    pub fn record_receive(&self, bytes: usize) {
        self.received.fetch_add(1, Ordering::Relaxed);
        self.bytes_received.fetch_add(bytes as u64, Ordering::Relaxed);
    }

    pub fn record_error(&self) {
        self.errors.fetch_add(1, Ordering::Relaxed);
    }

    pub fn is_running(&self) -> bool {
        self.running.load(Ordering::Relaxed)
    }

    pub fn stop(&self) {
        self.running.store(false, Ordering::Relaxed);
    }

    pub fn snapshot(&self) -> StressMetrics {
        StressMetrics {
            messages_sent: self.sent.load(Ordering::Relaxed),
            messages_received: self.received.load(Ordering::Relaxed),
            bytes_sent: self.bytes_sent.load(Ordering::Relaxed),
            bytes_received: self.bytes_received.load(Ordering::Relaxed),
            errors: self.errors.load(Ordering::Relaxed),
            duration: Duration::ZERO,
            peak_rate: 0.0,
        }
    }
}

impl Default for StressCounters {
    fn default() -> Self {
        Self {
            sent: AtomicU64::new(0),
            received: AtomicU64::new(0),
            bytes_sent: AtomicU64::new(0),
            bytes_received: AtomicU64::new(0),
            errors: AtomicU64::new(0),
            running: AtomicBool::new(true),
        }
    }
}

/// Runner for stress tests with progress reporting
pub struct StressRunner {
    config: StressConfig,
    counters: Arc<StressCounters>,
}

impl StressRunner {
    pub fn new(config: StressConfig) -> Self {
        Self {
            config,
            counters: StressCounters::new(),
        }
    }

    pub fn counters(&self) -> Arc<StressCounters> {
        self.counters.clone()
    }

    pub fn config(&self) -> &StressConfig {
        &self.config
    }

    /// Run the stress test with progress reporting
    pub fn run_with_progress<F>(&self, test_fn: F) -> StressMetrics
    where
        F: FnOnce(Arc<StressCounters>),
    {
        let start = Instant::now();
        let counters = self.counters.clone();
        let duration = self.config.duration;
        let report_interval = self.config.report_interval;

        // Spawn progress reporter
        let report_counters = counters.clone();
        let reporter = std::thread::spawn(move || {
            let mut last_sent = 0u64;
            let mut peak_rate = 0.0f64;

            while report_counters.is_running() {
                std::thread::sleep(report_interval);
                
                let current_sent = report_counters.sent.load(Ordering::Relaxed);
                let current_received = report_counters.received.load(Ordering::Relaxed);
                let errors = report_counters.errors.load(Ordering::Relaxed);
                
                let rate = (current_sent - last_sent) as f64 / report_interval.as_secs_f64();
                peak_rate = peak_rate.max(rate);
                last_sent = current_sent;
                
                let elapsed = start.elapsed();
                eprintln!(
                    "[{:>5.1}s] sent: {:>10}, recv: {:>10}, rate: {:>8.0}/s, errors: {}",
                    elapsed.as_secs_f64(),
                    current_sent,
                    current_received,
                    rate,
                    errors
                );

                if elapsed >= duration {
                    report_counters.stop();
                    break;
                }
            }
            
            peak_rate
        });

        // Run the test
        test_fn(counters.clone());

        // Stop and collect
        counters.stop();
        let peak_rate = reporter.join().unwrap_or(0.0);
        
        let mut metrics = counters.snapshot();
        metrics.duration = start.elapsed();
        metrics.peak_rate = peak_rate;
        
        metrics
    }
}

/// Print a summary of stress test results
pub fn print_summary(metrics: &StressMetrics) {
    eprintln!("\n╔══════════════════════════════════════════════════════════════╗");
    eprintln!("║                    STRESS TEST RESULTS                       ║");
    eprintln!("╠══════════════════════════════════════════════════════════════╣");
    eprintln!("║  Duration:        {:>10.2}s                                ║", metrics.duration.as_secs_f64());
    eprintln!("║  Messages Sent:   {:>10}                                  ║", metrics.messages_sent);
    eprintln!("║  Messages Recv:   {:>10}                                  ║", metrics.messages_received);
    eprintln!("║  Send Rate:       {:>10.0} msg/s                          ║", metrics.send_rate());
    eprintln!("║  Receive Rate:    {:>10.0} msg/s                          ║", metrics.receive_rate());
    eprintln!("║  Peak Rate:       {:>10.0} msg/s                          ║", metrics.peak_rate);
    eprintln!("║  Loss Rate:       {:>10.4}%                               ║", metrics.loss_rate() * 100.0);
    eprintln!("║  Errors:          {:>10}                                  ║", metrics.errors);
    eprintln!("║  Throughput:      {:>10.2} MB/s                           ║", metrics.throughput_mbps());
    eprintln!("╚══════════════════════════════════════════════════════════════╝");
    
    // Verdict
    if metrics.errors > 0 {
        eprintln!("\n❌ FAILED: {} errors detected", metrics.errors);
    } else if metrics.loss_rate() > 0.01 {
        eprintln!("\n⚠️  WARNING: {:.2}% message loss", metrics.loss_rate() * 100.0);
    } else {
        eprintln!("\n✅ PASSED: No errors, {:.4}% loss", metrics.loss_rate() * 100.0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_stress_counters() {
        let counters = StressCounters::new();
        
        counters.record_send(100);
        counters.record_send(100);
        counters.record_receive(100);
        
        let metrics = counters.snapshot();
        assert_eq!(metrics.messages_sent, 2);
        assert_eq!(metrics.messages_received, 1);
        assert_eq!(metrics.bytes_sent, 200);
    }

    #[test]
    fn test_stress_metrics_rates() {
        let mut metrics = StressMetrics::new();
        metrics.messages_sent = 1000;
        metrics.messages_received = 990;
        metrics.duration = Duration::from_secs(10);
        
        assert!((metrics.send_rate() - 100.0).abs() < 0.1);
        assert!((metrics.receive_rate() - 99.0).abs() < 0.1);
        assert!((metrics.loss_rate() - 0.01).abs() < 0.001);
    }
}


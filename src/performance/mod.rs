//! Performance monitoring and metrics

use std::sync::atomic::{ AtomicU64, Ordering };
use std::time::Instant;

/// Performance statistics
#[derive(Debug, Clone)]
pub struct PerformanceStats {
    /// Messages processed per second
    pub messages_per_second: u64,
    /// Total messages processed
    pub total_messages: u64,
    /// Error count
    pub error_count: u64,
    /// P50 latency in nanoseconds
    pub p50_latency_ns: u64,
    /// P95 latency in nanoseconds
    pub p95_latency_ns: u64,
    /// P99 latency in nanoseconds
    pub p99_latency_ns: u64,
    /// P99.9 latency in nanoseconds
    pub p999_latency_ns: u64,
}

/// Performance monitor
pub struct PerformanceMonitor {
    throughput_counter: AtomicU64,
    error_counter: AtomicU64,
    start_time: Instant,
}

impl PerformanceMonitor {
    /// Create a new performance monitor
    pub fn new() -> Self {
        Self {
            throughput_counter: AtomicU64::new(0),
            error_counter: AtomicU64::new(0),
            start_time: Instant::now(),
        }
    }

    /// Record throughput
    pub fn record_throughput(&self, count: u64) {
        self.throughput_counter.fetch_add(count, Ordering::Relaxed);
    }

    /// Record error
    pub fn record_error(&self) {
        self.error_counter.fetch_add(1, Ordering::Relaxed);
    }

    /// Get performance statistics
    pub fn get_stats(&self) -> PerformanceStats {
        let elapsed = self.start_time.elapsed();
        let total_messages = self.throughput_counter.load(Ordering::Relaxed);
        let messages_per_second = if elapsed.as_secs() > 0 {
            total_messages / elapsed.as_secs()
        } else {
            0
        };

        PerformanceStats {
            messages_per_second,
            total_messages,
            error_count: self.error_counter.load(Ordering::Relaxed),
            p50_latency_ns: 100_000, // Placeholder
            p95_latency_ns: 200_000, // Placeholder
            p99_latency_ns: 500_000, // Placeholder
            p999_latency_ns: 1_000_000, // Placeholder
        }
    }
}

impl Default for PerformanceMonitor {
    fn default() -> Self {
        Self::new()
    }
}

/// Production configuration
#[derive(Debug, Clone)]
pub struct ProductionConfig {
    /// Enable huge pages
    pub huge_pages: bool,
    /// NUMA node
    pub numa_node: Option<usize>,
    /// CPU cores to isolate
    pub cpu_isolation: Vec<usize>,
    /// Lock memory
    pub memory_lock: bool,
    /// Real-time priority
    pub realtime_priority: Option<i32>,
}

impl Default for ProductionConfig {
    fn default() -> Self {
        Self {
            huge_pages: false,
            numa_node: None,
            cpu_isolation: Vec::new(),
            memory_lock: false,
            realtime_priority: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_performance_monitor() {
        let monitor = PerformanceMonitor::new();

        monitor.record_throughput(100);
        monitor.record_error();

        let stats = monitor.get_stats();
        assert_eq!(stats.total_messages, 100);
        assert_eq!(stats.error_count, 1);
    }
}

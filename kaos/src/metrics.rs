//! Metrics for kaos ring buffers.
//!
//! Lightweight counters for observability

use std::sync::atomic::{ AtomicU64, Ordering };

/// Global metrics counters
pub struct Metrics {
    pub messages_sent: AtomicU64,
    pub messages_received: AtomicU64,
    pub bytes_sent: AtomicU64,
    pub bytes_received: AtomicU64,
    pub backpressure_events: AtomicU64,
    pub retransmits: AtomicU64,
}

impl Metrics {
    pub const fn new() -> Self {
        Self {
            messages_sent: AtomicU64::new(0),
            messages_received: AtomicU64::new(0),
            bytes_sent: AtomicU64::new(0),
            bytes_received: AtomicU64::new(0),
            backpressure_events: AtomicU64::new(0),
            retransmits: AtomicU64::new(0),
        }
    }

    #[inline]
    pub fn record_send(&self, bytes: u64) {
        self.messages_sent.fetch_add(1, Ordering::Relaxed);
        self.bytes_sent.fetch_add(bytes, Ordering::Relaxed);
    }

    #[inline]
    pub fn record_receive(&self, bytes: u64) {
        self.messages_received.fetch_add(1, Ordering::Relaxed);
        self.bytes_received.fetch_add(bytes, Ordering::Relaxed);
    }

    #[inline]
    pub fn record_backpressure(&self) {
        self.backpressure_events.fetch_add(1, Ordering::Relaxed);
    }

    #[inline]
    pub fn record_retransmit(&self) {
        self.retransmits.fetch_add(1, Ordering::Relaxed);
    }

    pub fn snapshot(&self) -> MetricsSnapshot {
        MetricsSnapshot {
            messages_sent: self.messages_sent.load(Ordering::Relaxed),
            messages_received: self.messages_received.load(Ordering::Relaxed),
            bytes_sent: self.bytes_sent.load(Ordering::Relaxed),
            bytes_received: self.bytes_received.load(Ordering::Relaxed),
            backpressure_events: self.backpressure_events.load(Ordering::Relaxed),
            retransmits: self.retransmits.load(Ordering::Relaxed),
        }
    }

    pub fn reset(&self) {
        self.messages_sent.store(0, Ordering::Relaxed);
        self.messages_received.store(0, Ordering::Relaxed);
        self.bytes_sent.store(0, Ordering::Relaxed);
        self.bytes_received.store(0, Ordering::Relaxed);
        self.backpressure_events.store(0, Ordering::Relaxed);
        self.retransmits.store(0, Ordering::Relaxed);
    }
}

impl Default for Metrics {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone, Copy)]
pub struct MetricsSnapshot {
    pub messages_sent: u64,
    pub messages_received: u64,
    pub bytes_sent: u64,
    pub bytes_received: u64,
    pub backpressure_events: u64,
    pub retransmits: u64,
}

impl std::fmt::Display for MetricsSnapshot {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "tx={} rx={} bytes_tx={} bytes_rx={} backpressure={} retx={}",
            self.messages_sent,
            self.messages_received,
            self.bytes_sent,
            self.bytes_received,
            self.backpressure_events,
            self.retransmits
        )
    }
}

/// Global metrics instance
pub static METRICS: Metrics = Metrics::new();

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_metrics() {
        let m = Metrics::new();
        m.record_send(100);
        m.record_receive(100);
        m.record_backpressure();

        let s = m.snapshot();
        assert_eq!(s.messages_sent, 1);
        assert_eq!(s.messages_received, 1);
        assert_eq!(s.bytes_sent, 100);
        assert_eq!(s.backpressure_events, 1);
    }
}

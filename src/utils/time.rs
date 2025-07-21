//! High-precision time utilities for performance measurement

use std::time::{ SystemTime, UNIX_EPOCH, Duration, Instant };
use std::sync::atomic::{ AtomicU64, Ordering };

/// Get current time in nanoseconds since Unix epoch
pub fn get_nanos() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_nanos() as u64
}

/// Get current time in microseconds since Unix epoch
pub fn get_micros() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_micros() as u64
}

/// Get current time in milliseconds since Unix epoch
pub fn get_millis() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_millis() as u64
}

/// Get current time in seconds since Unix epoch
pub fn get_secs() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_secs()
}

/// Get TSC (Time Stamp Counter) for ultra-high precision timing
/// This is CPU-specific and may not be available on all platforms
pub fn get_tsc() -> u64 {
    #[cfg(target_arch = "x86_64")]
    unsafe {
        std::arch::x86_64::_rdtsc()
    }
    #[cfg(not(target_arch = "x86_64"))]
    {
        // Fallback to nanosecond precision
        get_nanos()
    }
}

/// Get monotonic time for measuring elapsed time
pub fn get_monotonic_nanos() -> u64 {
    static START: std::sync::Once = std::sync::Once::new();
    static mut START_INSTANT: Option<Instant> = None;

    START.call_once(|| {
        unsafe {
            START_INSTANT = Some(Instant::now());
        }
    });

    unsafe { START_INSTANT.unwrap().elapsed().as_nanos() as u64 }
}

/// High-precision timer for measuring elapsed time
pub struct Timer {
    start: Instant,
    start_nanos: u64,
}

impl Timer {
    /// Create a new timer starting now
    pub fn new() -> Self {
        Self {
            start: Instant::now(),
            start_nanos: get_nanos(),
        }
    }

    /// Get elapsed time in nanoseconds
    pub fn elapsed_nanos(&self) -> u64 {
        self.start.elapsed().as_nanos() as u64
    }

    /// Get elapsed time in microseconds
    pub fn elapsed_micros(&self) -> u64 {
        self.start.elapsed().as_micros() as u64
    }

    /// Get elapsed time in milliseconds
    pub fn elapsed_millis(&self) -> u64 {
        self.start.elapsed().as_millis() as u64
    }

    /// Get elapsed time as Duration
    pub fn elapsed(&self) -> Duration {
        self.start.elapsed()
    }

    /// Reset the timer
    pub fn reset(&mut self) {
        self.start = Instant::now();
        self.start_nanos = get_nanos();
    }
}

impl Default for Timer {
    fn default() -> Self {
        Self::new()
    }
}

/// Timestamp provider trait for different time sources
pub trait TimestampProvider: Send + Sync {
    /// Get current timestamp in nanoseconds
    fn now_nanos(&self) -> u64;

    /// Get current timestamp in microseconds
    fn now_micros(&self) -> u64 {
        self.now_nanos() / 1000
    }

    /// Get current timestamp in milliseconds
    fn now_millis(&self) -> u64 {
        self.now_nanos() / 1_000_000
    }
}

/// System time provider using SystemTime
pub struct SystemTimeProvider;

impl TimestampProvider for SystemTimeProvider {
    fn now_nanos(&self) -> u64 {
        get_nanos()
    }
}

/// TSC-based timestamp provider (x86_64 only)
pub struct TscTimeProvider {
    tsc_frequency: u64,
    offset: AtomicU64,
}

impl TscTimeProvider {
    /// Create a new TSC time provider
    /// frequency: TSC frequency in Hz
    pub fn new(frequency: u64) -> Self {
        Self {
            tsc_frequency: frequency,
            offset: AtomicU64::new(0),
        }
    }

    /// Calibrate the TSC offset with system time
    pub fn calibrate(&self) {
        let system_time = get_nanos();
        let tsc_time = get_tsc();

        // Use checked arithmetic to prevent overflow
        let tsc_nanos = if let Some(product) = tsc_time.checked_mul(1_000_000_000) {
            product / self.tsc_frequency
        } else {
            // Handle overflow by using 128-bit arithmetic
            let tsc_time_128 = tsc_time as u128;
            let freq_128 = self.tsc_frequency as u128;
            ((tsc_time_128 * 1_000_000_000) / freq_128) as u64
        };

        let offset = system_time.saturating_sub(tsc_nanos);
        self.offset.store(offset, Ordering::Release);
    }

    /// Get TSC frequency
    pub fn frequency(&self) -> u64 {
        self.tsc_frequency
    }
}

impl TimestampProvider for TscTimeProvider {
    fn now_nanos(&self) -> u64 {
        let tsc = get_tsc();

        // Use checked arithmetic to prevent overflow
        let tsc_nanos = if let Some(product) = tsc.checked_mul(1_000_000_000) {
            product / self.tsc_frequency
        } else {
            // Handle overflow by using 128-bit arithmetic
            let tsc_128 = tsc as u128;
            let freq_128 = self.tsc_frequency as u128;
            ((tsc_128 * 1_000_000_000) / freq_128) as u64
        };

        let offset = self.offset.load(Ordering::Acquire);
        tsc_nanos + offset
    }
}

/// Monotonic timestamp provider
pub struct MonotonicTimeProvider;

impl TimestampProvider for MonotonicTimeProvider {
    fn now_nanos(&self) -> u64 {
        get_monotonic_nanos()
    }
}

/// Sleep for a precise duration using busy waiting
pub fn precise_sleep(duration: Duration) {
    let start = Instant::now();
    while start.elapsed() < duration {
        std::hint::spin_loop();
    }
}

/// Sleep for nanoseconds with high precision
pub fn sleep_nanos(nanos: u64) {
    precise_sleep(Duration::from_nanos(nanos));
}

/// Sleep for microseconds with high precision
pub fn sleep_micros(micros: u64) {
    precise_sleep(Duration::from_micros(micros));
}

/// Measure the execution time of a function
pub fn measure_time<F, R>(f: F) -> (R, Duration) where F: FnOnce() -> R {
    let start = Instant::now();
    let result = f();
    let elapsed = start.elapsed();
    (result, elapsed)
}

/// Measure the execution time of a function in nanoseconds
pub fn measure_nanos<F, R>(f: F) -> (R, u64) where F: FnOnce() -> R {
    let (result, duration) = measure_time(f);
    (result, duration.as_nanos() as u64)
}

/// Rate limiter for controlling execution frequency
pub struct RateLimiter {
    interval: Duration,
    last_execution: Instant,
}

impl RateLimiter {
    /// Create a new rate limiter
    pub fn new(rate_per_second: u64) -> Self {
        Self {
            interval: Duration::from_nanos(1_000_000_000 / rate_per_second),
            last_execution: Instant::now() - Duration::from_secs(1),
        }
    }

    /// Check if execution is allowed (non-blocking)
    pub fn try_execute(&mut self) -> bool {
        let now = Instant::now();
        if now.duration_since(self.last_execution) >= self.interval {
            self.last_execution = now;
            true
        } else {
            false
        }
    }

    /// Wait until execution is allowed (blocking)
    pub fn wait_and_execute(&mut self) {
        let now = Instant::now();
        let elapsed = now.duration_since(self.last_execution);
        if elapsed < self.interval {
            let sleep_time = self.interval - elapsed;
            std::thread::sleep(sleep_time);
        }
        self.last_execution = Instant::now();
    }
}

/// Timeout helper for operations
pub struct Timeout {
    deadline: Instant,
}

impl Timeout {
    /// Create a new timeout
    pub fn new(duration: Duration) -> Self {
        Self {
            deadline: Instant::now() + duration,
        }
    }

    /// Check if the timeout has expired
    pub fn is_expired(&self) -> bool {
        Instant::now() >= self.deadline
    }

    /// Get remaining time
    pub fn remaining(&self) -> Duration {
        let now = Instant::now();
        if now >= self.deadline {
            Duration::ZERO
        } else {
            self.deadline - now
        }
    }
}

/// Format duration as a human-readable string
pub fn format_duration(duration: Duration) -> String {
    let nanos = duration.as_nanos();

    if nanos < 1_000 {
        format!("{}ns", nanos)
    } else if nanos < 1_000_000 {
        format!("{:.2}μs", (nanos as f64) / 1_000.0)
    } else if nanos < 1_000_000_000 {
        format!("{:.2}ms", (nanos as f64) / 1_000_000.0)
    } else {
        format!("{:.2}s", (nanos as f64) / 1_000_000_000.0)
    }
}

/// Convert nanoseconds to Duration
pub fn nanos_to_duration(nanos: u64) -> Duration {
    Duration::from_nanos(nanos)
}

/// Convert Duration to nanoseconds
pub fn duration_to_nanos(duration: Duration) -> u64 {
    duration.as_nanos() as u64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_time_functions() {
        let nanos = get_nanos();
        let micros = get_micros();
        let millis = get_millis();
        let secs = get_secs();

        assert!(nanos > 0);
        assert!(micros > 0);
        assert!(millis > 0);
        assert!(secs > 0);

        // Check relationships
        assert!(nanos / 1000 >= micros - 1); // Allow for rounding
        assert!(micros / 1000 >= millis - 1);
        assert!(millis / 1000 >= secs - 1);
    }

    #[test]
    fn test_timer() {
        let mut timer = Timer::new();
        std::thread::sleep(Duration::from_millis(1));

        let elapsed = timer.elapsed_nanos();
        assert!(elapsed >= 1_000_000); // At least 1ms

        timer.reset();
        let new_elapsed = timer.elapsed_nanos();
        assert!(new_elapsed < elapsed);
    }

    #[test]
    fn test_timestamp_providers() {
        let system_provider = SystemTimeProvider;
        let monotonic_provider = MonotonicTimeProvider;

        let sys_time = system_provider.now_nanos();
        let mono_time = monotonic_provider.now_nanos();

        assert!(sys_time > 0);
        // Monotonic time validation (always >= 0)
        let _ = mono_time;
    }

    #[test]
    fn test_measure_time() {
        let (result, duration) = measure_time(|| {
            std::thread::sleep(Duration::from_millis(1));
            42
        });

        assert_eq!(result, 42);
        assert!(duration >= Duration::from_millis(1));
    }

    #[test]
    fn test_measure_nanos() {
        let (result, nanos) = measure_nanos(|| {
            std::thread::sleep(Duration::from_millis(1));
            "test"
        });

        assert_eq!(result, "test");
        assert!(nanos >= 1_000_000);
    }

    #[test]
    fn test_rate_limiter() {
        let mut limiter = RateLimiter::new(1000); // 1000 per second

        assert!(limiter.try_execute());
        assert!(!limiter.try_execute()); // Should be rate limited

        // Wait a bit and try again
        std::thread::sleep(Duration::from_millis(2));
        assert!(limiter.try_execute());
    }

    #[test]
    fn test_timeout() {
        let timeout = Timeout::new(Duration::from_millis(10));
        assert!(!timeout.is_expired());

        std::thread::sleep(Duration::from_millis(15));
        assert!(timeout.is_expired());
        assert_eq!(timeout.remaining(), Duration::ZERO);
    }

    #[test]
    fn test_format_duration() {
        assert_eq!(format_duration(Duration::from_nanos(500)), "500ns");
        assert_eq!(format_duration(Duration::from_micros(1500)), "1.50ms");
        assert_eq!(format_duration(Duration::from_millis(2500)), "2.50s");
    }

    #[test]
    fn test_conversion_functions() {
        let duration = Duration::from_nanos(123456789);
        let nanos = duration_to_nanos(duration);
        let back = nanos_to_duration(nanos);

        assert_eq!(nanos, 123456789);
        assert_eq!(back, duration);
    }

    #[test]
    fn test_monotonic_time() {
        let time1 = get_monotonic_nanos();
        std::thread::sleep(Duration::from_millis(1));
        let time2 = get_monotonic_nanos();

        assert!(time2 > time1);
    }

    #[test]
    fn test_tsc_time_provider() {
        let provider = TscTimeProvider::new(2_400_000_000); // 2.4 GHz
        provider.calibrate();

        let time1 = provider.now_nanos();
        std::thread::sleep(Duration::from_millis(1));
        let time2 = provider.now_nanos();

        assert!(time2 > time1);
        assert_eq!(provider.frequency(), 2_400_000_000);
    }
}

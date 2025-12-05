//! Insights - Observability for kaos.
//!
//! Unified tracing, profiling, and logging. Zero-cost when disabled.
//!
//! # Usage
//!
//! ## Basic tracing (console output)
//! ```toml
//! kaos = { version = "0.1", features = ["tracing"] }
//! ```
//! ```rust,ignore
//! tracing_subscriber::fmt::init();
//! ```
//!
//! ## Tracy profiler (real-time visualization)
//! ```toml
//! kaos = { version = "0.1", features = ["tracy"] }
//! ```
//! ```rust,ignore
//! kaos::init_tracy();
//! ```
//! Then run Tracy profiler: https://github.com/wolfpld/tracy

/// Initialize Tracy profiler (call once at startup)
#[cfg(feature = "tracy")]
pub fn init_tracy() {
    use tracing_subscriber::layer::SubscriberExt;
    tracing::subscriber::set_global_default(
        tracing_subscriber::registry().with(tracing_tracy::TracyLayer::default()),
    )
    .expect("setup tracy layer");
}

#[cfg(not(feature = "tracy"))]
pub fn init_tracy() {}

/// Record a send operation (creates a span visible in Tracy)
#[cfg(feature = "tracing")]
#[inline]
pub fn record_send(bytes: u64) {
    let _span = tracing::trace_span!("send", bytes).entered();
}

#[cfg(not(feature = "tracing"))]
#[inline(always)]
pub fn record_send(_bytes: u64) {}

/// Record a receive operation (creates a span visible in Tracy)
#[cfg(feature = "tracing")]
#[inline]
pub fn record_receive(bytes: u64) {
    let _span = tracing::trace_span!("recv", bytes).entered();
}

#[cfg(not(feature = "tracing"))]
#[inline(always)]
pub fn record_receive(_bytes: u64) {}

/// Record backpressure (buffer full)
#[cfg(feature = "tracing")]
#[inline]
pub fn record_backpressure() {
    let _span = tracing::warn_span!("backpressure").entered();
}

#[cfg(not(feature = "tracing"))]
#[inline(always)]
pub fn record_backpressure() {}

/// Record a retransmit event
#[cfg(feature = "tracing")]
#[inline]
pub fn record_retransmit() {
    let _span = tracing::debug_span!("retransmit").entered();
}

#[cfg(not(feature = "tracing"))]
#[inline(always)]
pub fn record_retransmit() {}

/// Create a span for a connection
#[cfg(feature = "tracing")]
#[macro_export]
macro_rules! span_connection {
    ($addr:expr) => {
        tracing::info_span!("conn", addr = %$addr)
    };
}

#[cfg(not(feature = "tracing"))]
#[macro_export]
macro_rules! span_connection {
    ($addr:expr) => {
        ()
    };
}

/// Enter a span (no-op when tracing disabled)
#[cfg(feature = "tracing")]
#[macro_export]
macro_rules! enter_span {
    ($span:expr) => {
        let _guard = $span.enter();
    };
}

#[cfg(not(feature = "tracing"))]
#[macro_export]
macro_rules! enter_span {
    ($span:expr) => {};
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_metrics_compile() {
        // Just verify it compiles (no-op when tracing disabled)
        record_send(100);
        record_receive(100);
        record_backpressure();
        record_retransmit();
    }
}

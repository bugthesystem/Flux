//! # kaos-test-support
//!
//! Testing infrastructure for the kaos ecosystem.
//!
//! ## Components
//!
//! - **LossGenerator** - Simulates packet loss for RUDP testing
//! - **ChaosMonkey** - Random failures, delays, corruption
//! - **StressRunner** - Long-duration stress tests with metrics
//!
//! ## Inspired By
//!
//! Aeron's comprehensive testing infrastructure:
//! - `aeron-test-support` - Test harnesses and utilities
//! - `aeron-system-tests` - End-to-end system tests

pub mod loss;
pub mod chaos;
pub mod stress;
pub mod verify;

pub use loss::{LossGenerator, LossPattern, DropDecision};
pub use chaos::{ChaosMonkey, ChaosEvent};
pub use stress::{StressRunner, StressConfig, StressMetrics};
pub use verify::{DataVerifier, SequenceChecker};


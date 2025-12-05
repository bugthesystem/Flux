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

pub mod chaos;
pub mod loss;
pub mod stress;
pub mod verify;

pub use chaos::{ChaosEvent, ChaosMonkey};
pub use loss::{DropDecision, LossGenerator, LossPattern};
pub use stress::{StressConfig, StressMetrics, StressRunner};
pub use verify::{DataVerifier, SequenceChecker};

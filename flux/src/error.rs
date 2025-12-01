//! Error types and handling for the Flux library

use thiserror::Error;

/// Result type alias for Flux operations
pub type Result<T> = std::result::Result<T, FluxError>;

/// Main error type for the Flux library
#[derive(Error, Debug)]
pub enum FluxError {
    /// I/O errors from network operations
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    /// Ring buffer is full and cannot accept more messages
    #[error("Ring buffer is full")]
    RingBufferFull,

    /// Invalid configuration parameter
    #[error("Invalid configuration: {message}")]
    InvalidConfig {
        /// Error message describing the configuration issue
        message: String,
    },

    /// Network socket errors
    #[error("Socket error: {message}")]
    Socket {
        /// Error message describing the socket issue
        message: String,
    },

    /// Message validation errors
    #[error("Invalid message: {reason}")]
    InvalidMessage {
        /// Reason why the message is invalid
        reason: String,
    },

    /// Reliability protocol errors
    #[error("Reliability error: {message}")]
    Reliability {
        /// Error message describing the reliability issue
        message: String,
    },

    /// Performance monitoring errors
    #[error("Performance monitoring error: {message}")]
    Performance {
        /// Error message describing the performance issue
        message: String,
    },

    /// System resource errors
    #[error("System resource error: {message}")]
    SystemResource {
        /// Error message describing the system resource issue
        message: String,
    },

    /// Serialization/deserialization errors
    #[error("Serialization error: {message}")]
    Serialization {
        /// Error message describing the serialization issue
        message: String,
    },

    /// NUMA topology errors
    #[error("NUMA error: {message}")]
    Numa {
        /// Error message describing the NUMA issue
        message: String,
    },

    /// CPU affinity errors
    #[error("CPU affinity error: {0}")]
    CpuAffinity(#[from] nix::Error),

    /// Memory allocation errors
    #[error("Memory allocation error: {message}")]
    Memory {
        /// Error message describing the memory issue
        message: String,
    },

    /// Timeout errors
    #[error("Operation timed out")]
    Timeout,

    /// Generic error for unexpected conditions.
    /// This error should be used for situations that are not expected to happen
    /// in a correctly functioning system, such as a logic error in the code.
    #[error("Unexpected error: {message}")]
    Unexpected {
        /// Error message describing the unexpected condition
        message: String,
    },
}

impl FluxError {
    /// Create a new configuration error
    pub fn config(message: impl Into<String>) -> Self {
        Self::InvalidConfig {
            message: message.into(),
        }
    }

    /// Create a new socket error
    pub fn socket(message: impl Into<String>) -> Self {
        Self::Socket {
            message: message.into(),
        }
    }

    /// Create a new message validation error
    pub fn invalid_message(reason: impl Into<String>) -> Self {
        Self::InvalidMessage {
            reason: reason.into(),
        }
    }

    /// Create a new reliability error
    pub fn reliability(message: impl Into<String>) -> Self {
        Self::Reliability {
            message: message.into(),
        }
    }

    /// Create a new performance monitoring error
    pub fn performance(message: impl Into<String>) -> Self {
        Self::Performance {
            message: message.into(),
        }
    }

    /// Create a new system resource error
    pub fn system_resource(message: impl Into<String>) -> Self {
        Self::SystemResource {
            message: message.into(),
        }
    }

    /// Create a new NUMA error
    pub fn numa(message: impl Into<String>) -> Self {
        Self::Numa {
            message: message.into(),
        }
    }

    /// Create a new memory allocation error
    pub fn memory(message: impl Into<String>) -> Self {
        Self::Memory {
            message: message.into(),
        }
    }

    /// Create a new unexpected error
    pub fn unexpected(message: impl Into<String>) -> Self {
        Self::Unexpected {
            message: message.into(),
        }
    }

    /// Check if this error is recoverable
    pub fn is_recoverable(&self) -> bool {
        matches!(self, Self::RingBufferFull | Self::Timeout | Self::Socket { .. } | Self::Io(_))
    }

    /// Check if this error is related to system resources
    pub fn is_system_resource_error(&self) -> bool {
        matches!(self, Self::Memory { .. } | Self::SystemResource { .. } | Self::CpuAffinity(_))
    }

    /// Check if this error is related to network operations
    pub fn is_network_error(&self) -> bool {
        matches!(self, Self::Socket { .. } | Self::Io(_))
    }
}

/// Convenience macro for creating configuration errors
#[macro_export]
macro_rules! config_error {
    ($($arg:tt)*) => {
        $crate::error::FluxError::config(format!($($arg)*))
    };
}

/// Convenience macro for creating socket errors
#[macro_export]
macro_rules! socket_error {
    ($($arg:tt)*) => {
        $crate::error::FluxError::socket(format!($($arg)*))
    };
}

/// Convenience macro for creating reliability errors
#[macro_export]
macro_rules! reliability_error {
    ($($arg:tt)*) => {
        $crate::error::FluxError::reliability(format!($($arg)*))
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_error_creation() {
        let err = FluxError::config("test message");
        assert!(matches!(err, FluxError::InvalidConfig { .. }));
        assert!(!err.is_recoverable());
    }

    #[test]
    fn test_error_classification() {
        let ring_full = FluxError::RingBufferFull;
        assert!(ring_full.is_recoverable());
        assert!(!ring_full.is_system_resource_error());
        assert!(!ring_full.is_network_error());

        let memory_err = FluxError::memory("out of memory");
        assert!(!memory_err.is_recoverable());
        assert!(memory_err.is_system_resource_error());
        assert!(!memory_err.is_network_error());

        let socket_err = FluxError::socket("connection refused");
        assert!(socket_err.is_recoverable());
        assert!(!socket_err.is_system_resource_error());
        assert!(socket_err.is_network_error());
    }

    #[test]
    fn test_error_macros() {
        let err = config_error!("Invalid value: {}", 42);
        assert!(matches!(err, FluxError::InvalidConfig { .. }));

        let err = socket_error!("Port {} is busy", 8080);
        assert!(matches!(err, FluxError::Socket { .. }));
    }
}

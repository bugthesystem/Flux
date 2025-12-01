//! Error types for Kaos.

use thiserror::Error;

pub type Result<T> = std::result::Result<T, KaosError>;

#[derive(Error, Debug)]
pub enum KaosError {
    #[error("I/O error: {0}")] Io(#[from] std::io::Error),

    #[error("Invalid configuration: {message}")] InvalidConfig {
        message: String,
    },

    #[error("Invalid message: {reason}")] InvalidMessage {
        reason: String,
    },
}

impl KaosError {
    pub fn config(message: impl Into<String>) -> Self {
        Self::InvalidConfig { message: message.into() }
    }

    pub fn invalid_message(reason: impl Into<String>) -> Self {
        Self::InvalidMessage { reason: reason.into() }
    }
}

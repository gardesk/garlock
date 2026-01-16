//! Error types for garlock

use thiserror::Error;

/// Main error type for garlock
#[derive(Error, Debug)]
pub enum GarlockError {
    /// X11 connection or operation failed
    #[error("X11 error: {0}")]
    X11(String),

    /// Failed to grab keyboard or pointer
    #[error("Failed to grab input: {0}")]
    GrabFailed(String),

    /// Screenshot capture failed
    #[error("Screenshot failed: {0}")]
    ScreenshotFailed(String),

    /// PAM authentication error
    #[error("Authentication error: {0}")]
    AuthError(String),

    /// Configuration error
    #[error("Configuration error: {0}")]
    ConfigError(String),

    /// IPC error
    #[error("IPC error: {0}")]
    IpcError(String),

    /// Rendering error
    #[error("Rendering error: {0}")]
    RenderError(String),
}

/// Result type alias for garlock
pub type Result<T> = std::result::Result<T, GarlockError>;

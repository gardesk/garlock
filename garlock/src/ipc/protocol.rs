//! IPC protocol definitions for garlock
//!
//! JSON-based protocol for communication between garlock daemon and clients.

use serde::{Deserialize, Serialize};

/// Commands sent from clients to garlock daemon
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "command", rename_all = "kebab-case")]
pub enum Command {
    /// Lock the screen immediately
    Lock,
    /// Query current lock state
    QueryState,
    /// Gracefully shutdown the daemon
    Shutdown,
}

/// Response from garlock daemon to clients
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "kebab-case")]
pub enum Response {
    /// Command executed successfully
    Ok {
        /// Optional message
        #[serde(skip_serializing_if = "Option::is_none")]
        message: Option<String>,
    },
    /// Current state information
    State {
        /// Whether screen is currently locked
        locked: bool,
        /// Number of failed authentication attempts (if locked)
        #[serde(skip_serializing_if = "Option::is_none")]
        failed_attempts: Option<u32>,
        /// Whether in cooldown period
        #[serde(skip_serializing_if = "Option::is_none")]
        in_cooldown: Option<bool>,
    },
    /// Error occurred
    Error {
        /// Error message
        message: String,
    },
}

/// Events broadcast from garlock daemon to subscribers
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "event", rename_all = "kebab-case")]
pub enum Event {
    /// Screen was locked
    Locked,
    /// Screen was unlocked (successful auth)
    Unlocked,
    /// Authentication attempt failed
    AuthFailed {
        /// Current attempt number
        attempt: u32,
    },
    /// Cooldown started after too many failures
    CooldownStarted {
        /// Cooldown duration in seconds
        seconds: u64,
    },
    /// Daemon is shutting down
    Shutdown,
}

impl Response {
    /// Create a success response
    pub fn ok() -> Self {
        Self::Ok { message: None }
    }

    /// Create a success response with message
    pub fn ok_with_message(msg: impl Into<String>) -> Self {
        Self::Ok {
            message: Some(msg.into()),
        }
    }

    /// Create an error response
    pub fn error(msg: impl Into<String>) -> Self {
        Self::Error {
            message: msg.into(),
        }
    }

    /// Create a state response
    pub fn state(locked: bool, failed_attempts: Option<u32>, in_cooldown: Option<bool>) -> Self {
        Self::State {
            locked,
            failed_attempts,
            in_cooldown,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_command_serialization() {
        let cmd = Command::Lock;
        let json = serde_json::to_string(&cmd).unwrap();
        assert_eq!(json, r#"{"command":"lock"}"#);

        let cmd = Command::QueryState;
        let json = serde_json::to_string(&cmd).unwrap();
        assert_eq!(json, r#"{"command":"query-state"}"#);
    }

    #[test]
    fn test_command_deserialization() {
        let cmd: Command = serde_json::from_str(r#"{"command":"lock"}"#).unwrap();
        assert!(matches!(cmd, Command::Lock));

        let cmd: Command = serde_json::from_str(r#"{"command":"query-state"}"#).unwrap();
        assert!(matches!(cmd, Command::QueryState));
    }

    #[test]
    fn test_response_serialization() {
        let resp = Response::ok();
        let json = serde_json::to_string(&resp).unwrap();
        assert_eq!(json, r#"{"status":"ok"}"#);

        let resp = Response::state(true, Some(2), Some(false));
        let json = serde_json::to_string(&resp).unwrap();
        assert!(json.contains(r#""locked":true"#));
        assert!(json.contains(r#""failed_attempts":2"#));
    }

    #[test]
    fn test_event_serialization() {
        let event = Event::Locked;
        let json = serde_json::to_string(&event).unwrap();
        assert_eq!(json, r#"{"event":"locked"}"#);

        let event = Event::AuthFailed { attempt: 3 };
        let json = serde_json::to_string(&event).unwrap();
        assert!(json.contains(r#""event":"auth-failed""#));
        assert!(json.contains(r#""attempt":3"#));
    }
}

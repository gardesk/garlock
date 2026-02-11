//! PAM authentication for garlock
//!
//! Handles password verification against PAM to unlock the screen.
//! Authentication runs in a background thread to keep the UI responsive.

use anyhow::{anyhow, Result};
use pam::Client;
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::thread;

/// PAM service name (must match /etc/pam.d/garlock)
const PAM_SERVICE: &str = "garlock";

/// Result of an authentication attempt
#[derive(Debug)]
pub enum AuthResult {
    /// Authentication succeeded
    Success,
    /// Authentication failed
    Failure(String),
}

/// Handle for a pending authentication operation
pub struct PendingAuth {
    receiver: Receiver<AuthResult>,
}

impl PendingAuth {
    /// Check if authentication has completed (non-blocking)
    ///
    /// Returns `Some(result)` if complete, `None` if still pending.
    pub fn try_recv(&self) -> Option<AuthResult> {
        match self.receiver.try_recv() {
            Ok(result) => Some(result),
            Err(TryRecvError::Empty) => None,
            Err(TryRecvError::Disconnected) => {
                // Thread died unexpectedly
                Some(AuthResult::Failure("Authentication thread died".to_string()))
            }
        }
    }
}

/// Get the current username
pub fn get_current_username() -> Result<String> {
    users::get_current_username()
        .and_then(|s| s.into_string().ok())
        .ok_or_else(|| anyhow!("Failed to get current username"))
}

/// Start authentication in a background thread
///
/// Returns a handle that can be polled for the result.
pub fn authenticate_async(username: String, password: String) -> PendingAuth {
    let (tx, rx) = mpsc::channel();

    thread::spawn(move || {
        let result = authenticate_blocking(&username, &password);
        // Ignore send errors - receiver may have been dropped
        let _ = tx.send(result);
    });

    PendingAuth { receiver: rx }
}

/// Perform PAM authentication (blocking)
///
/// This should be called from a background thread to avoid blocking the UI.
fn authenticate_blocking(username: &str, password: &str) -> AuthResult {
    tracing::info!(
        %username,
        pw_bytes = password.len(),
        pw_chars = password.chars().count(),
        "Starting PAM authentication"
    );

    match pam_authenticate(username, password) {
        Ok(()) => {
            tracing::info!(%username, "PAM authentication succeeded");
            AuthResult::Success
        }
        Err(e) => {
            tracing::warn!(%username, error = %e, "PAM authentication failed");
            AuthResult::Failure(e.to_string())
        }
    }
}

/// Low-level PAM authentication
fn pam_authenticate(username: &str, password: &str) -> Result<()> {
    // Create PAM client with password conversation handler
    let mut client = Client::with_password(PAM_SERVICE)
        .map_err(|e| anyhow!("Failed to create PAM client: {:?}", e))?;

    // Set credentials for the conversation
    client
        .conversation_mut()
        .set_credentials(username, password);

    // Authenticate the user
    // Note: For a screen locker, we only need to verify the password.
    // We don't need to open a session since one already exists.
    client
        .authenticate()
        .map_err(|e| anyhow!("Authentication failed: {:?}", e))?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_get_current_username() {
        // Should succeed in most environments
        let result = get_current_username();
        assert!(result.is_ok());
        let username = result.unwrap();
        assert!(!username.is_empty());
    }
}

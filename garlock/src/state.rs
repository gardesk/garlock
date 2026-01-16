//! State management for garlock
//!
//! Implements dual state machines following swaylock patterns:
//! - AuthState: Tracks authentication progress
//! - InputState: Tracks visual input feedback

use std::time::{Duration, Instant};

use crate::ring::RingState;

/// Input idle timeout - return to idle after this duration of no input
pub const INPUT_IDLE_TIMEOUT: Duration = Duration::from_millis(300);

/// Auth invalid display timeout - show "wrong" state for this duration
pub const AUTH_INVALID_TIMEOUT: Duration = Duration::from_millis(1500);

/// Authentication state (what's happening with PAM)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum AuthState {
    /// No authentication in progress
    #[default]
    Idle,
    /// Currently validating password with PAM
    Validating,
    /// Authentication failed, showing error state
    Invalid,
}

impl AuthState {
    /// Get display name for logging
    pub fn name(&self) -> &'static str {
        match self {
            AuthState::Idle => "idle",
            AuthState::Validating => "validating",
            AuthState::Invalid => "invalid",
        }
    }
}

/// Input state (visual feedback for ring indicator)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum InputState {
    /// No recent input, ring shows idle color
    #[default]
    Idle,
    /// A character was typed
    Letter,
    /// Backspace removed a character
    Backspace,
    /// Password buffer was cleared (Ctrl+U or backspace on empty)
    Clear,
    /// A modifier key was pressed (no visual change)
    Neutral,
}

impl InputState {
    /// Get display name for logging
    pub fn name(&self) -> &'static str {
        match self {
            InputState::Idle => "idle",
            InputState::Letter => "letter",
            InputState::Backspace => "backspace",
            InputState::Clear => "clear",
            InputState::Neutral => "neutral",
        }
    }

    /// Check if this state should trigger visual feedback
    pub fn has_visual_feedback(&self) -> bool {
        !matches!(self, InputState::Idle | InputState::Neutral)
    }
}

/// Locker state machine combining auth and input states
pub struct LockerState {
    /// Current authentication state
    pub auth: AuthState,
    /// Current input state
    pub input: InputState,
    /// Time of last input event
    last_input: Option<Instant>,
    /// Time when auth became invalid (for timeout)
    invalid_since: Option<Instant>,
    /// Number of failed authentication attempts
    pub failed_attempts: u32,
    /// Cooldown end time (if in cooldown after too many failures)
    cooldown_until: Option<Instant>,
}

impl LockerState {
    /// Create a new locker state
    pub fn new() -> Self {
        Self {
            auth: AuthState::Idle,
            input: InputState::Idle,
            last_input: None,
            invalid_since: None,
            failed_attempts: 0,
            cooldown_until: None,
        }
    }

    /// Record an input event, updating timers
    pub fn on_input(&mut self, input: InputState) {
        self.input = input;
        self.last_input = Some(Instant::now());
        tracing::trace!(input = input.name(), "Input state changed");
    }

    /// Start password validation
    pub fn start_validation(&mut self) {
        self.auth = AuthState::Validating;
        tracing::debug!("Auth state: validating");
    }

    /// Handle successful authentication
    pub fn on_auth_success(&mut self) {
        self.auth = AuthState::Idle;
        self.failed_attempts = 0;
        tracing::info!("Authentication successful");
    }

    /// Handle failed authentication
    pub fn on_auth_failure(&mut self) {
        self.auth = AuthState::Invalid;
        self.invalid_since = Some(Instant::now());
        self.failed_attempts += 1;
        tracing::warn!(attempts = self.failed_attempts, "Authentication failed");
    }

    /// Check if currently in cooldown period
    pub fn is_in_cooldown(&self) -> bool {
        self.cooldown_until
            .map(|t| Instant::now() < t)
            .unwrap_or(false)
    }

    /// Start a cooldown period after too many failed attempts
    pub fn start_cooldown(&mut self, seconds: u64) {
        let duration = Duration::from_secs(seconds);
        self.cooldown_until = Some(Instant::now() + duration);
        tracing::warn!(seconds, "Cooldown started after {} failed attempts", self.failed_attempts);
    }

    /// Get remaining cooldown time in seconds (0 if not in cooldown)
    pub fn cooldown_remaining(&self) -> u64 {
        self.cooldown_until
            .map(|t| {
                let now = Instant::now();
                if now < t {
                    (t - now).as_secs()
                } else {
                    0
                }
            })
            .unwrap_or(0)
    }

    /// Check and update timers, returning true if state changed
    pub fn update_timers(&mut self) -> bool {
        let mut changed = false;

        // Check input idle timeout
        if self.input != InputState::Idle {
            if let Some(last) = self.last_input {
                if last.elapsed() >= INPUT_IDLE_TIMEOUT {
                    self.input = InputState::Idle;
                    changed = true;
                    tracing::trace!("Input state decayed to idle");
                }
            }
        }

        // Check auth invalid timeout
        if self.auth == AuthState::Invalid {
            if let Some(since) = self.invalid_since {
                if since.elapsed() >= AUTH_INVALID_TIMEOUT {
                    self.auth = AuthState::Idle;
                    self.invalid_since = None;
                    changed = true;
                    tracing::trace!("Auth state decayed to idle");
                }
            }
        }

        // Check cooldown expiry
        if let Some(until) = self.cooldown_until {
            if Instant::now() >= until {
                self.cooldown_until = None;
                self.failed_attempts = 0; // Reset attempts after cooldown
                tracing::info!("Cooldown expired, attempts reset");
                changed = true;
            }
        }

        changed
    }

    /// Get the appropriate ring state based on current auth/input states
    pub fn ring_state(&self) -> RingState {
        // Auth state takes priority for verifying/wrong
        match self.auth {
            AuthState::Validating => return RingState::Verifying,
            AuthState::Invalid => return RingState::Wrong,
            AuthState::Idle => {}
        }

        // Then check input state
        match self.input {
            InputState::Letter => RingState::Typing,
            InputState::Backspace | InputState::Clear => RingState::Clear,
            InputState::Idle | InputState::Neutral => RingState::Idle,
        }
    }

    /// Check if we should show the Caps Lock indicator
    pub fn should_show_caps_lock(&self, caps_active: bool) -> bool {
        // Show when typing and caps lock is on
        caps_active && !matches!(self.input, InputState::Idle)
    }
}

impl Default for LockerState {
    fn default() -> Self {
        Self::new()
    }
}

//! Ring indicator module for garlock
//!
//! Renders a swaylock-style circular ring that provides visual feedback
//! for the current lock state.

pub mod renderer;

pub use renderer::{composite_ring, RingRenderer};

/// Ring indicator state
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum RingState {
    /// Waiting for input (blue)
    #[default]
    Idle,
    /// Receiving password input (green)
    Typing,
    /// Verifying password with PAM (orange)
    Verifying,
    /// Authentication failed (red)
    Wrong,
    /// Backspace/clearing input (yellow)
    Clear,
}

impl RingState {
    /// Get the display name for this state
    pub fn name(&self) -> &'static str {
        match self {
            RingState::Idle => "idle",
            RingState::Typing => "typing",
            RingState::Verifying => "verifying",
            RingState::Wrong => "wrong",
            RingState::Clear => "clear",
        }
    }
}

/// RGBA color with components in 0.0-1.0 range
#[derive(Debug, Clone, Copy)]
pub struct Color {
    pub r: f64,
    pub g: f64,
    pub b: f64,
    pub a: f64,
}

impl Color {
    /// Create a new color from RGBA components (0.0-1.0)
    pub fn new(r: f64, g: f64, b: f64, a: f64) -> Self {
        Self { r, g, b, a }
    }

    /// Parse a hex color string like "#1e90ffcc" or "#1e90ff"
    pub fn from_hex(hex: &str) -> Option<Self> {
        let hex = hex.trim_start_matches('#');

        match hex.len() {
            6 => {
                // RGB without alpha
                let r = u8::from_str_radix(&hex[0..2], 16).ok()?;
                let g = u8::from_str_radix(&hex[2..4], 16).ok()?;
                let b = u8::from_str_radix(&hex[4..6], 16).ok()?;
                Some(Self {
                    r: r as f64 / 255.0,
                    g: g as f64 / 255.0,
                    b: b as f64 / 255.0,
                    a: 1.0,
                })
            }
            8 => {
                // RGBA with alpha
                let r = u8::from_str_radix(&hex[0..2], 16).ok()?;
                let g = u8::from_str_radix(&hex[2..4], 16).ok()?;
                let b = u8::from_str_radix(&hex[4..6], 16).ok()?;
                let a = u8::from_str_radix(&hex[6..8], 16).ok()?;
                Some(Self {
                    r: r as f64 / 255.0,
                    g: g as f64 / 255.0,
                    b: b as f64 / 255.0,
                    a: a as f64 / 255.0,
                })
            }
            _ => None,
        }
    }

    /// Default blue for idle state
    pub fn idle() -> Self {
        Self::from_hex("#1e90ffcc").unwrap()
    }

    /// Default green for typing state
    pub fn typing() -> Self {
        Self::from_hex("#00ff00cc").unwrap()
    }

    /// Default orange for verifying state
    pub fn verifying() -> Self {
        Self::from_hex("#ffa500cc").unwrap()
    }

    /// Default red for wrong state
    pub fn wrong() -> Self {
        Self::from_hex("#ff0000cc").unwrap()
    }

    /// Default yellow for clear state
    pub fn clear() -> Self {
        Self::from_hex("#ffff00cc").unwrap()
    }

    /// Default dark color for inner circle
    pub fn inside() -> Self {
        Self::from_hex("#00000088").unwrap()
    }

    /// Default color for ring background track
    pub fn ring_bg() -> Self {
        Self::from_hex("#00000055").unwrap()
    }
}

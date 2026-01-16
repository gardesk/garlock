//! X11 window management for garlock
//!
//! Provides fullscreen window creation with secure keyboard/pointer grabs.

mod window;
mod monitors;

pub use window::LockerWindow;
pub use monitors::{Monitor, MonitorConfig};

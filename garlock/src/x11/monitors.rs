//! Monitor detection using RandR extension
//!
//! Detects connected monitors and their positions for proper
//! multi-monitor support.

use anyhow::{Context, Result};
use x11rb::protocol::randr::{self, ConnectionExt as RandrConnectionExt};
use x11rb::protocol::xproto::Window;
use x11rb::rust_connection::RustConnection;

/// Information about a single monitor
#[derive(Debug, Clone)]
pub struct Monitor {
    /// Monitor name (e.g., "eDP-1", "HDMI-1")
    pub name: String,
    /// X position in virtual screen
    pub x: i16,
    /// Y position in virtual screen
    pub y: i16,
    /// Width in pixels
    pub width: u16,
    /// Height in pixels
    pub height: u16,
    /// Whether this is the primary monitor
    pub primary: bool,
}

impl Monitor {
    /// Get the center point of this monitor
    pub fn center(&self) -> (f64, f64) {
        let cx = self.x as f64 + self.width as f64 / 2.0;
        let cy = self.y as f64 + self.height as f64 / 2.0;
        (cx, cy)
    }
}

/// Configuration of all connected monitors
#[derive(Debug, Clone)]
pub struct MonitorConfig {
    /// List of connected monitors
    pub monitors: Vec<Monitor>,
    /// Total virtual screen width
    pub total_width: u16,
    /// Total virtual screen height
    pub total_height: u16,
}

impl MonitorConfig {
    /// Detect monitors using RandR extension
    pub fn detect(conn: &RustConnection, root: Window) -> Result<Self> {
        // Query RandR version to ensure it's available
        let version = conn
            .randr_query_version(1, 5)?
            .reply()
            .context("RandR not available")?;

        tracing::debug!(
            major = version.major_version,
            minor = version.minor_version,
            "RandR version"
        );

        // Get screen resources
        let resources = conn
            .randr_get_screen_resources_current(root)?
            .reply()
            .context("Failed to get screen resources")?;

        // Get primary output
        let primary = conn
            .randr_get_output_primary(root)?
            .reply()
            .context("Failed to get primary output")?
            .output;

        let mut monitors = Vec::new();

        // Iterate through outputs
        for &output in &resources.outputs {
            let output_info = match conn.randr_get_output_info(output, resources.config_timestamp) {
                Ok(cookie) => match cookie.reply() {
                    Ok(info) => info,
                    Err(_) => continue,
                },
                Err(_) => continue,
            };

            // Skip disconnected outputs
            if output_info.connection != randr::Connection::CONNECTED {
                continue;
            }

            // Skip outputs without a CRTC (not displaying anything)
            if output_info.crtc == 0 {
                continue;
            }

            // Get CRTC info for position and size
            let crtc_info = match conn.randr_get_crtc_info(output_info.crtc, resources.config_timestamp) {
                Ok(cookie) => match cookie.reply() {
                    Ok(info) => info,
                    Err(_) => continue,
                },
                Err(_) => continue,
            };

            // Skip CRTCs with no mode (disabled)
            if crtc_info.mode == 0 {
                continue;
            }

            let name = String::from_utf8_lossy(&output_info.name).to_string();
            let is_primary = output == primary;

            monitors.push(Monitor {
                name: name.clone(),
                x: crtc_info.x,
                y: crtc_info.y,
                width: crtc_info.width,
                height: crtc_info.height,
                primary: is_primary,
            });

            tracing::debug!(
                name,
                x = crtc_info.x,
                y = crtc_info.y,
                width = crtc_info.width,
                height = crtc_info.height,
                is_primary,
                "Detected monitor"
            );
        }

        // Calculate total virtual screen size
        let total_width = monitors
            .iter()
            .map(|m| m.x as u32 + m.width as u32)
            .max()
            .unwrap_or(0) as u16;

        let total_height = monitors
            .iter()
            .map(|m| m.y as u32 + m.height as u32)
            .max()
            .unwrap_or(0) as u16;

        // Sort monitors: primary first, then by x position
        monitors.sort_by(|a, b| {
            if a.primary != b.primary {
                b.primary.cmp(&a.primary) // primary first
            } else {
                a.x.cmp(&b.x) // then by x position
            }
        });

        tracing::info!(
            count = monitors.len(),
            total_width,
            total_height,
            "Monitors detected"
        );

        Ok(Self {
            monitors,
            total_width,
            total_height,
        })
    }

    /// Get the primary monitor (or first monitor if no primary)
    pub fn primary(&self) -> Option<&Monitor> {
        self.monitors.iter().find(|m| m.primary).or(self.monitors.first())
    }

    /// Check if this is a single-monitor setup
    pub fn is_single_monitor(&self) -> bool {
        self.monitors.len() <= 1
    }
}

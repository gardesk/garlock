//! Configuration loading and management for garlock
//!
//! Loads configuration from TOML file with sensible defaults.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::Path;

/// Main configuration structure
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    /// General settings
    pub general: GeneralConfig,

    /// Background settings
    pub background: BackgroundConfig,

    /// Ring indicator settings
    pub ring: RingConfig,

    /// Indicator settings (caps lock, attempts, etc.)
    pub indicator: IndicatorConfig,

    /// Font settings
    pub font: FontConfig,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            general: GeneralConfig::default(),
            background: BackgroundConfig::default(),
            ring: RingConfig::default(),
            indicator: IndicatorConfig::default(),
            font: FontConfig::default(),
        }
    }
}

impl Config {
    /// Load configuration from file or use defaults
    ///
    /// If no config file exists, creates one with default values.
    pub fn load(path: Option<&Path>) -> Result<Self> {
        let config_path = path
            .map(|p| p.to_path_buf())
            .or_else(|| dirs::config_dir().map(|d| d.join("garlock/config.toml")));

        if let Some(path) = &config_path {
            if path.exists() {
                let content = std::fs::read_to_string(path)
                    .with_context(|| format!("Failed to read config file: {:?}", path))?;
                let config: Config = toml::from_str(&content)
                    .with_context(|| format!("Failed to parse config file: {:?}", path))?;
                tracing::info!(?path, "Loaded configuration");
                return Ok(config);
            }
        }

        // Create default config file if it doesn't exist
        let config = Config::default();
        if let Some(path) = config_path {
            if let Err(e) = config.write_default(&path) {
                tracing::warn!(?path, "Failed to create default config: {}", e);
            }
        }

        tracing::debug!("Using default configuration");
        Ok(config)
    }

    /// Write the default configuration file with comments
    fn write_default(&self, path: &Path) -> Result<()> {
        // Create parent directory if needed
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("Failed to create config directory: {:?}", parent))?;
        }

        let content = Self::default_config_content();
        std::fs::write(path, content)
            .with_context(|| format!("Failed to write config file: {:?}", path))?;

        tracing::info!(?path, "Created default configuration file");
        Ok(())
    }

    /// Generate default config file content with documentation comments
    fn default_config_content() -> String {
        r##"# garlock configuration
# Screen locker for the gar desktop suite
#
# This file is auto-generated with default values.
# Uncomment and modify options as needed.

[general]
# Grace period in seconds before password is required (0 to disable)
grace_period = 0

# PAM service name (must have corresponding /etc/pam.d/garlock file)
pam_service = "garlock"

# Maximum failed attempts before cooldown kicks in
max_attempts = 3

# Cooldown duration in seconds (multiplied by attempts over max)
cooldown_seconds = 5

[background]
# Gaussian blur radius (higher = more blur, 0 to disable)
blur_radius = 25.0

# Brightness adjustment (0.0 = black, 1.0 = original brightness)
brightness = 0.6

# Fallback solid color if screenshot capture fails (hex format)
fallback_color = "#1a1a2e"

[ring]
# Ring geometry
radius_outer = 90.0
radius_inner = 75.0
line_width = 6.0

# State colors (hex format with optional alpha: #RRGGBB or #RRGGBBAA)
# Idle: waiting for input
color_idle = "#1e90ffcc"

# Typing: receiving password input
color_typing = "#00ff00cc"

# Verifying: checking password with PAM
color_verifying = "#ffa500cc"

# Wrong: authentication failed
color_wrong = "#ff0000cc"

# Clear: backspace pressed / clearing input
color_clear = "#ffff00cc"

# Inner circle fill color
color_inside = "#00000088"

# Ring track background color
color_ring_bg = "#00000055"

[indicator]
# Show Caps Lock warning when active
show_caps_lock = true
caps_lock_text = "Caps Lock"

# Show number of failed attempts
show_failed_attempts = true

# Show current time on lock screen
show_time = false
time_format = "%H:%M"

[font]
# Font family for text elements
family = "Sans"

# Font size in points
size = 14
"##
        .to_string()
    }
}

/// General settings
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct GeneralConfig {
    /// Grace period in seconds before password is required (0 to disable)
    pub grace_period: u32,

    /// PAM service name
    pub pam_service: String,

    /// Maximum failed attempts before cooldown
    pub max_attempts: u32,

    /// Cooldown duration in seconds per attempt over max
    pub cooldown_seconds: u32,
}

impl Default for GeneralConfig {
    fn default() -> Self {
        Self {
            grace_period: 0,
            pam_service: "garlock".to_string(),
            max_attempts: 3,
            cooldown_seconds: 5,
        }
    }
}

/// Background settings
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct BackgroundConfig {
    /// Blur radius (0 to disable)
    pub blur_radius: f32,

    /// Brightness adjustment (0.0-1.0, lower is darker)
    pub brightness: f32,

    /// Fallback color if screenshot fails (hex format)
    pub fallback_color: String,
}

impl Default for BackgroundConfig {
    fn default() -> Self {
        Self {
            blur_radius: 25.0,
            brightness: 0.6,
            fallback_color: "#1a1a2e".to_string(),
        }
    }
}

/// Ring indicator settings
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct RingConfig {
    /// Outer ring radius
    pub radius_outer: f64,

    /// Inner ring radius
    pub radius_inner: f64,

    /// Ring line width
    pub line_width: f64,

    /// Color when idle (hex format with alpha)
    pub color_idle: String,

    /// Color when typing (hex format with alpha)
    pub color_typing: String,

    /// Color when verifying (hex format with alpha)
    pub color_verifying: String,

    /// Color when wrong password (hex format with alpha)
    pub color_wrong: String,

    /// Color when cleared (hex format with alpha)
    pub color_clear: String,

    /// Inner circle color (hex format with alpha)
    pub color_inside: String,

    /// Ring background color (hex format with alpha)
    pub color_ring_bg: String,
}

impl Default for RingConfig {
    fn default() -> Self {
        Self {
            radius_outer: 90.0,
            radius_inner: 75.0,
            line_width: 6.0,
            color_idle: "#1e90ffcc".to_string(),
            color_typing: "#00ff00cc".to_string(),
            color_verifying: "#ffa500cc".to_string(),
            color_wrong: "#ff0000cc".to_string(),
            color_clear: "#ffff00cc".to_string(),
            color_inside: "#00000088".to_string(),
            color_ring_bg: "#00000055".to_string(),
        }
    }
}

/// Indicator settings
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct IndicatorConfig {
    /// Show Caps Lock warning
    pub show_caps_lock: bool,

    /// Caps Lock warning text
    pub caps_lock_text: String,

    /// Show failed attempt count
    pub show_failed_attempts: bool,

    /// Show time on lock screen
    pub show_time: bool,

    /// Time format (strftime)
    pub time_format: String,
}

impl Default for IndicatorConfig {
    fn default() -> Self {
        Self {
            show_caps_lock: true,
            caps_lock_text: "Caps Lock".to_string(),
            show_failed_attempts: true,
            show_time: false,
            time_format: "%H:%M".to_string(),
        }
    }
}

/// Font settings
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct FontConfig {
    /// Font family
    pub family: String,

    /// Font size
    pub size: u32,
}

impl Default for FontConfig {
    fn default() -> Self {
        Self {
            family: "Sans".to_string(),
            size: 14,
        }
    }
}

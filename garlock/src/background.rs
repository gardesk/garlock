//! Background processing for garlock
//!
//! Applies blur and brightness adjustments to captured screenshots.

use anyhow::{Context, Result};
use image::{imageops, RgbaImage};

use crate::screenshot::{rgba_to_bgra, Screenshot};

/// Processed background ready for display
pub struct Background {
    /// Pixel data in BGRA format for X11
    pub data: Vec<u8>,
    /// Image width
    pub width: u32,
    /// Image height
    pub height: u32,
}

impl Background {
    /// Create a blurred background from a screenshot
    pub fn from_screenshot(
        screenshot: Screenshot,
        blur_radius: f32,
        brightness: f32,
    ) -> Result<Self> {
        tracing::debug!(
            blur_radius,
            brightness,
            "Processing screenshot into background"
        );

        // Convert BGRA to RGBA for image crate
        let rgba_data = screenshot.to_rgba();

        // Create image from raw data
        let img = RgbaImage::from_raw(screenshot.width, screenshot.height, rgba_data)
            .context("Failed to create image from screenshot data")?;

        // Apply gaussian blur
        tracing::debug!("Applying blur (radius={})", blur_radius);
        let blurred = imageops::blur(&img, blur_radius);

        // Apply brightness adjustment
        tracing::debug!("Applying brightness adjustment (factor={})", brightness);
        let adjusted = adjust_brightness(&blurred, brightness);

        // Convert back to BGRA for X11
        let mut bgra_data = adjusted.into_raw();
        rgba_to_bgra(&mut bgra_data);

        Ok(Self {
            data: bgra_data,
            width: screenshot.width,
            height: screenshot.height,
        })
    }

    /// Create a solid color fallback background
    pub fn solid_color(width: u32, height: u32, hex_color: &str) -> Self {
        let (r, g, b) = parse_hex_color(hex_color).unwrap_or((26, 26, 46)); // #1a1a2e default

        let pixel_count = (width * height) as usize;
        let mut data = Vec::with_capacity(pixel_count * 4);

        for _ in 0..pixel_count {
            // BGRA format
            data.push(b);
            data.push(g);
            data.push(r);
            data.push(255); // Alpha
        }

        tracing::debug!(width, height, r, g, b, "Created solid color background");

        Self { data, width, height }
    }
}

/// Adjust image brightness by a factor
/// - 0.0-1.0: darken
/// - 1.0: no change
/// - >1.0: brighten
fn adjust_brightness(img: &RgbaImage, factor: f32) -> RgbaImage {
    let mut result = img.clone();
    for pixel in result.pixels_mut() {
        pixel[0] = (pixel[0] as f32 * factor).min(255.0) as u8;
        pixel[1] = (pixel[1] as f32 * factor).min(255.0) as u8;
        pixel[2] = (pixel[2] as f32 * factor).min(255.0) as u8;
        // Alpha unchanged
    }
    result
}

/// Parse a hex color string like "#1a1a2e" or "1a1a2e"
fn parse_hex_color(hex: &str) -> Option<(u8, u8, u8)> {
    let hex = hex.trim_start_matches('#');
    if hex.len() != 6 {
        return None;
    }

    let r = u8::from_str_radix(&hex[0..2], 16).ok()?;
    let g = u8::from_str_radix(&hex[2..4], 16).ok()?;
    let b = u8::from_str_radix(&hex[4..6], 16).ok()?;

    Some((r, g, b))
}

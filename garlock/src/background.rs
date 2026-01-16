//! Background processing for garlock
//!
//! Applies blur and brightness adjustments to captured screenshots.

use anyhow::{Context, Result};
use image::{imageops, RgbaImage};

use crate::screenshot::{rgba_to_bgra, Screenshot};

/// Downsample factor for fast blur (blur at 1/N resolution)
const BLUR_DOWNSAMPLE_FACTOR: u32 = 4;

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
    ///
    /// Uses downsample-blur-upsample for fast processing:
    /// 1. Shrink image to 1/4 size
    /// 2. Apply blur at reduced resolution (much faster)
    /// 3. Scale back to original size
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

        let original_width = screenshot.width;
        let original_height = screenshot.height;

        // Downsample for faster blur
        let small_width = original_width / BLUR_DOWNSAMPLE_FACTOR;
        let small_height = original_height / BLUR_DOWNSAMPLE_FACTOR;

        tracing::debug!(
            original_width,
            original_height,
            small_width,
            small_height,
            "Downsampling for fast blur"
        );

        let small_img = imageops::resize(
            &img,
            small_width,
            small_height,
            imageops::FilterType::Triangle,
        );

        // Apply blur at reduced resolution (scale radius proportionally)
        let scaled_radius = blur_radius / BLUR_DOWNSAMPLE_FACTOR as f32;
        tracing::debug!(scaled_radius, "Applying blur at reduced resolution");
        let blurred_small = imageops::blur(&small_img, scaled_radius);

        // Upsample back to original size
        tracing::debug!("Upsampling to original resolution");
        let blurred = imageops::resize(
            &blurred_small,
            original_width,
            original_height,
            imageops::FilterType::Triangle,
        );

        // Apply brightness adjustment
        tracing::debug!("Applying brightness adjustment (factor={})", brightness);
        let adjusted = adjust_brightness(&blurred, brightness);

        // Convert back to BGRA for X11
        let mut bgra_data = adjusted.into_raw();
        rgba_to_bgra(&mut bgra_data);

        Ok(Self {
            data: bgra_data,
            width: original_width,
            height: original_height,
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

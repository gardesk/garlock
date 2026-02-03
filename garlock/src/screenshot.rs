//! Screenshot capture for garlock
//!
//! Captures the root window contents before displaying the locker.

use anyhow::{Context, Result};
use x11rb::connection::Connection;
use x11rb::protocol::xproto::{ConnectionExt, ImageFormat};

/// Captured screenshot data
pub struct Screenshot {
    /// Raw pixel data in BGRA format (X11 native)
    pub data: Vec<u8>,
    /// Image width
    pub width: u32,
    /// Image height
    pub height: u32,
    /// Bits per pixel (typically 24 or 32)
    pub depth: u8,
}

impl Screenshot {
    /// Capture the root window contents
    ///
    /// This should be called BEFORE creating the locker window to capture
    /// the current desktop state.
    pub fn capture() -> Result<Self> {
        let (conn, screen_num) =
            x11rb::connect(None).context("Failed to connect to X server for screenshot")?;

        let screen = &conn.setup().roots[screen_num];
        let root = screen.root;
        let width = screen.width_in_pixels;
        let height = screen.height_in_pixels;
        let depth = screen.root_depth;

        tracing::debug!(width, height, depth, "Capturing root window");

        // Get image from root window
        let reply = conn
            .get_image(
                ImageFormat::Z_PIXMAP,
                root,
                0,
                0,
                width,
                height,
                !0, // plane_mask - all planes
            )?
            .reply()
            .context("Failed to get root window image")?;

        tracing::debug!(
            data_len = reply.data.len(),
            depth = reply.depth,
            "Screenshot captured"
        );

        // Verify we got the expected amount of data
        let expected_size = width as usize * height as usize * 4; // 4 bytes per pixel for 32-bit
        if reply.data.len() < expected_size {
            tracing::warn!(
                "Screenshot data smaller than expected: {} < {}",
                reply.data.len(),
                expected_size
            );
        }

        // Check if screenshot is mostly black (compositor might not be exposing root window)
        let non_black_pixels = Self::count_non_black_pixels(&reply.data);
        let total_pixels = (width as usize * height as usize) as f64;
        let non_black_ratio = non_black_pixels as f64 / total_pixels;

        if non_black_ratio < 0.01 {
            tracing::warn!(
                "Screenshot appears to be mostly black ({:.2}% non-black pixels). \
                 This may indicate compositor is not exposing root window contents.",
                non_black_ratio * 100.0
            );
        } else {
            tracing::debug!(
                "Screenshot content check: {:.1}% non-black pixels",
                non_black_ratio * 100.0
            );
        }

        Ok(Self {
            data: reply.data,
            width: width as u32,
            height: height as u32,
            depth: reply.depth,
        })
    }

    /// Convert BGRA data to RGBA for image processing
    pub fn to_rgba(&self) -> Vec<u8> {
        let mut rgba = self.data.clone();
        bgra_to_rgba(&mut rgba);
        rgba
    }

    /// Count pixels that are not pure black (for detecting empty screenshots)
    fn count_non_black_pixels(data: &[u8]) -> usize {
        let mut count = 0;
        // Sample every 100th pixel for performance
        for chunk in data.chunks_exact(4).step_by(100) {
            // BGRA format - check if any color channel is non-zero
            if chunk[0] > 5 || chunk[1] > 5 || chunk[2] > 5 {
                count += 1;
            }
        }
        // Extrapolate to total
        count * 100
    }
}

/// Convert BGRA pixel data to RGBA in-place
pub fn bgra_to_rgba(data: &mut [u8]) {
    for chunk in data.chunks_exact_mut(4) {
        chunk.swap(0, 2); // B <-> R
    }
}

/// Convert RGBA pixel data to BGRA in-place
pub fn rgba_to_bgra(data: &mut [u8]) {
    for chunk in data.chunks_exact_mut(4) {
        chunk.swap(0, 2); // R <-> B
    }
}

//! Text overlay rendering for garlock
//!
//! Renders text elements like time, caps lock indicator, and failed attempts
//! using Pango for text layout and Cairo for rendering.

use anyhow::{Context, Result};
use cairo::{Context as CairoContext, Format, ImageSurface};
use chrono::Local;
use pango::FontDescription;
use pangocairo::functions::{create_layout, show_layout};

use crate::config::{FontConfig, IndicatorConfig};
use crate::ring::Color;

/// Text overlay renderer
pub struct OverlayRenderer {
    /// Font description for text
    font: FontDescription,
    /// Font description for large text (time)
    font_large: FontDescription,
    /// Text color
    color: Color,
    /// Warning color (for caps lock)
    warning_color: Color,
    /// Error color (for failed attempts)
    error_color: Color,
}

impl OverlayRenderer {
    /// Create a new overlay renderer from config
    pub fn new(font_config: &FontConfig) -> Self {
        let mut font = FontDescription::new();
        font.set_family(&font_config.family);
        font.set_size(font_config.size as i32 * pango::SCALE);

        let mut font_large = FontDescription::new();
        font_large.set_family(&font_config.family);
        font_large.set_size((font_config.size * 3) as i32 * pango::SCALE);

        Self {
            font,
            font_large,
            color: Color::from_hex("#ffffffdd").unwrap_or(Color::white()),
            warning_color: Color::from_hex("#ffaa00dd").unwrap_or(Color::clear()),
            error_color: Color::from_hex("#ff4444dd").unwrap_or(Color::wrong()),
        }
    }

    /// Render time display
    ///
    /// Returns a surface with the rendered time, or None if time display is disabled.
    pub fn render_time(&self, config: &IndicatorConfig) -> Option<Result<OverlaySurface>> {
        if !config.show_time {
            return None;
        }

        let time_str = Local::now().format(&config.time_format).to_string();
        Some(self.render_text(&time_str, &self.font_large, self.color))
    }

    /// Render caps lock indicator
    ///
    /// Returns a surface with the caps lock warning, or None if not applicable.
    pub fn render_caps_lock(
        &self,
        config: &IndicatorConfig,
        caps_active: bool,
    ) -> Option<Result<OverlaySurface>> {
        if !config.show_caps_lock || !caps_active {
            return None;
        }

        Some(self.render_text(
            &config.caps_lock_text,
            &self.font,
            self.warning_color,
        ))
    }

    /// Render failed attempts indicator
    ///
    /// Returns a surface showing failed attempt count, or None if no failures.
    pub fn render_failed_attempts(
        &self,
        config: &IndicatorConfig,
        attempts: u32,
    ) -> Option<Result<OverlaySurface>> {
        if !config.show_failed_attempts || attempts == 0 {
            return None;
        }

        let text = if attempts == 1 {
            "1 failed attempt".to_string()
        } else {
            format!("{} failed attempts", attempts)
        };

        Some(self.render_text(&text, &self.font, self.error_color))
    }

    /// Render cooldown timer
    ///
    /// Returns a surface showing remaining cooldown time.
    pub fn render_cooldown(&self, seconds_remaining: u64) -> Option<Result<OverlaySurface>> {
        if seconds_remaining == 0 {
            return None;
        }

        let text = format!("Try again in {}s", seconds_remaining);
        Some(self.render_text(&text, &self.font, self.error_color))
    }

    /// Render text to a surface
    fn render_text(
        &self,
        text: &str,
        font: &FontDescription,
        color: Color,
    ) -> Result<OverlaySurface> {
        // Create a temporary surface to measure text
        let temp_surface = ImageSurface::create(Format::ARgb32, 1, 1)
            .context("Failed to create temp surface")?;
        let temp_ctx =
            CairoContext::new(&temp_surface).context("Failed to create temp context")?;

        let layout = create_layout(&temp_ctx);
        layout.set_font_description(Some(font));
        layout.set_text(text);

        let (width, height) = layout.pixel_size();
        let padding = 4;
        let surface_width = width + padding * 2;
        let surface_height = height + padding * 2;

        // Create actual surface
        let surface = ImageSurface::create(Format::ARgb32, surface_width, surface_height)
            .context("Failed to create text surface")?;
        let ctx = CairoContext::new(&surface).context("Failed to create Cairo context")?;

        // Render text
        ctx.move_to(padding as f64, padding as f64);
        ctx.set_source_rgba(color.r, color.g, color.b, color.a);

        let layout = create_layout(&ctx);
        layout.set_font_description(Some(font));
        layout.set_text(text);
        show_layout(&ctx, &layout);

        surface.flush();

        Ok(OverlaySurface {
            surface,
            width: surface_width,
            height: surface_height,
        })
    }
}

/// A rendered overlay surface ready for compositing
pub struct OverlaySurface {
    pub surface: ImageSurface,
    pub width: i32,
    pub height: i32,
}

impl OverlaySurface {
    /// Get the raw pixel data (BGRA format for X11)
    pub fn to_bgra(&mut self) -> Result<Vec<u8>> {
        self.surface.flush();
        let data = self.surface.data().context("Failed to get surface data")?;
        Ok(data.to_vec())
    }
}

/// Composite an overlay surface onto a background buffer
pub fn composite_overlay(
    background: &mut [u8],
    bg_width: u32,
    bg_height: u32,
    overlay_data: &[u8],
    overlay_width: i32,
    overlay_height: i32,
    dest_x: i32,
    dest_y: i32,
) {
    let bg_stride = bg_width as usize * 4;
    let overlay_stride = overlay_width as usize * 4;

    for oy in 0..overlay_height {
        let by = dest_y + oy;
        if by < 0 || by >= bg_height as i32 {
            continue;
        }

        for ox in 0..overlay_width {
            let bx = dest_x + ox;
            if bx < 0 || bx >= bg_width as i32 {
                continue;
            }

            let overlay_offset = (oy as usize * overlay_stride) + (ox as usize * 4);
            let bg_offset = (by as usize * bg_stride) + (bx as usize * 4);

            // Overlay pixel (BGRA, premultiplied alpha from Cairo)
            let ob = overlay_data[overlay_offset] as f64 / 255.0;
            let og = overlay_data[overlay_offset + 1] as f64 / 255.0;
            let or = overlay_data[overlay_offset + 2] as f64 / 255.0;
            let oa = overlay_data[overlay_offset + 3] as f64 / 255.0;

            if oa < 0.001 {
                continue;
            }

            // Background pixel (BGRA)
            let bb = background[bg_offset] as f64 / 255.0;
            let bg = background[bg_offset + 1] as f64 / 255.0;
            let br = background[bg_offset + 2] as f64 / 255.0;
            let ba = background[bg_offset + 3] as f64 / 255.0;

            // Alpha compositing
            let out_a = oa + ba * (1.0 - oa);
            if out_a > 0.001 {
                let out_r = (or + br * (1.0 - oa)) / out_a * oa + br * (1.0 - oa);
                let out_g = (og + bg * (1.0 - oa)) / out_a * oa + bg * (1.0 - oa);
                let out_b = (ob + bb * (1.0 - oa)) / out_a * oa + bb * (1.0 - oa);

                background[bg_offset] = (out_b.min(1.0) * 255.0) as u8;
                background[bg_offset + 1] = (out_g.min(1.0) * 255.0) as u8;
                background[bg_offset + 2] = (out_r.min(1.0) * 255.0) as u8;
                background[bg_offset + 3] = (out_a.min(1.0) * 255.0) as u8;
            }
        }
    }
}

// Extend Color with additional helpers
impl Color {
    pub fn white() -> Self {
        Self {
            r: 1.0,
            g: 1.0,
            b: 1.0,
            a: 0.87,
        }
    }
}

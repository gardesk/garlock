//! Ring indicator renderer using Cairo
//!
//! Renders the circular ring indicator with state-based colors and
//! segment highlighting for keystroke feedback.

use std::f64::consts::PI;

use anyhow::{Context, Result};
use cairo::{Context as CairoContext, Format, ImageSurface, Operator};

use super::{Color, RingState};
use crate::config::RingConfig;

/// Number of segments around the ring for keystroke feedback
const NUM_SEGMENTS: usize = 12;

/// Ring indicator renderer
pub struct RingRenderer {
    /// Outer radius of the ring
    outer_radius: f64,
    /// Inner radius of the ring (dark center)
    inner_radius: f64,
    /// Line width for ring stroke
    line_width: f64,
    /// Colors for each state
    color_idle: Color,
    color_typing: Color,
    color_verifying: Color,
    color_wrong: Color,
    color_clear: Color,
    color_inside: Color,
    color_ring_bg: Color,
    /// Current state
    state: RingState,
    /// Current highlighted segment (0-11, or None)
    highlight_segment: Option<usize>,
}

impl RingRenderer {
    /// Create a new ring renderer from config
    pub fn from_config(config: &RingConfig) -> Self {
        Self {
            outer_radius: config.radius_outer,
            inner_radius: config.radius_inner,
            line_width: config.line_width,
            color_idle: Color::from_hex(&config.color_idle).unwrap_or_else(Color::idle),
            color_typing: Color::from_hex(&config.color_typing).unwrap_or_else(Color::typing),
            color_verifying: Color::from_hex(&config.color_verifying)
                .unwrap_or_else(Color::verifying),
            color_wrong: Color::from_hex(&config.color_wrong).unwrap_or_else(Color::wrong),
            color_clear: Color::from_hex(&config.color_clear).unwrap_or_else(Color::clear),
            color_inside: Color::from_hex(&config.color_inside).unwrap_or_else(Color::inside),
            color_ring_bg: Color::from_hex(&config.color_ring_bg).unwrap_or_else(Color::ring_bg),
            state: RingState::Idle,
            highlight_segment: None,
        }
    }

    /// Set the current ring state
    pub fn set_state(&mut self, state: RingState) {
        self.state = state;
    }

    /// Get the current ring state
    pub fn state(&self) -> RingState {
        self.state
    }

    /// Set the highlighted segment (for keystroke feedback)
    pub fn set_highlight_segment(&mut self, segment: Option<usize>) {
        self.highlight_segment = segment.map(|s| s % NUM_SEGMENTS);
    }

    /// Advance highlight to next segment
    pub fn advance_highlight(&mut self) {
        self.highlight_segment = Some(
            self.highlight_segment
                .map(|s| (s + 1) % NUM_SEGMENTS)
                .unwrap_or(0),
        );
    }

    /// Retreat highlight to previous segment (for backspace)
    ///
    /// If at segment 0, clears the highlight entirely.
    pub fn retreat_highlight(&mut self) {
        self.highlight_segment = self.highlight_segment.and_then(|s| {
            if s == 0 {
                None // At start, clear highlight
            } else {
                Some(s - 1)
            }
        });
    }

    /// Clear the highlight
    pub fn clear_highlight(&mut self) {
        self.highlight_segment = None;
    }

    /// Get the color for the current state
    fn state_color(&self) -> Color {
        match self.state {
            RingState::Idle => self.color_idle,
            RingState::Typing => self.color_typing,
            RingState::Verifying => self.color_verifying,
            RingState::Wrong => self.color_wrong,
            RingState::Clear => self.color_clear,
        }
    }

    /// Render the ring to a new Cairo surface
    ///
    /// Returns a surface sized to fit the ring with some padding.
    pub fn render(&self) -> Result<ImageSurface> {
        let padding = 10.0;
        let size = (self.outer_radius * 2.0 + padding * 2.0).ceil() as i32;

        let surface = ImageSurface::create(Format::ARgb32, size, size)
            .context("Failed to create ring surface")?;

        let ctx = CairoContext::new(&surface).context("Failed to create Cairo context")?;

        // Center of the surface
        let cx = size as f64 / 2.0;
        let cy = size as f64 / 2.0;

        self.draw(&ctx, cx, cy)?;

        surface.flush();
        Ok(surface)
    }

    /// Draw the ring at the specified center coordinates
    pub fn draw(&self, ctx: &CairoContext, cx: f64, cy: f64) -> Result<()> {
        // Draw inner dark circle (background)
        ctx.arc(cx, cy, self.inner_radius, 0.0, 2.0 * PI);
        ctx.set_source_rgba(
            self.color_inside.r,
            self.color_inside.g,
            self.color_inside.b,
            self.color_inside.a,
        );
        ctx.fill()?;

        // Draw ring background track
        let ring_center_radius = (self.outer_radius + self.inner_radius) / 2.0;
        ctx.set_line_width(self.line_width);
        ctx.arc(cx, cy, ring_center_radius, 0.0, 2.0 * PI);
        ctx.set_source_rgba(
            self.color_ring_bg.r,
            self.color_ring_bg.g,
            self.color_ring_bg.b,
            self.color_ring_bg.a,
        );
        ctx.stroke()?;

        // Draw ring with state color
        let color = self.state_color();
        ctx.set_line_width(self.line_width);
        ctx.arc(cx, cy, ring_center_radius, 0.0, 2.0 * PI);
        ctx.set_source_rgba(color.r, color.g, color.b, color.a);
        ctx.stroke()?;

        // Draw segment highlight if active
        if let Some(segment) = self.highlight_segment {
            self.draw_segment_highlight(ctx, cx, cy, segment)?;
        }

        Ok(())
    }

    /// Draw a highlighted segment
    fn draw_segment_highlight(
        &self,
        ctx: &CairoContext,
        cx: f64,
        cy: f64,
        segment: usize,
    ) -> Result<()> {
        let segment_angle = 2.0 * PI / NUM_SEGMENTS as f64;
        // Start from top (-PI/2) and go clockwise
        let start_angle = -PI / 2.0 + (segment as f64 * segment_angle);
        let end_angle = start_angle + segment_angle;

        let ring_center_radius = (self.outer_radius + self.inner_radius) / 2.0;

        ctx.set_line_width(self.line_width + 2.0);
        ctx.arc(cx, cy, ring_center_radius, start_angle, end_angle);
        // Brighter version of current state color
        let color = self.state_color();
        ctx.set_source_rgba(
            (color.r + 0.3).min(1.0),
            (color.g + 0.3).min(1.0),
            (color.b + 0.3).min(1.0),
            color.a,
        );
        ctx.stroke()?;

        Ok(())
    }

    /// Get the size needed for the ring (width and height)
    pub fn size(&self) -> (i32, i32) {
        let padding = 10.0;
        let size = (self.outer_radius * 2.0 + padding * 2.0).ceil() as i32;
        (size, size)
    }

    /// Get the raw pixel data from a surface (BGRA format for X11)
    pub fn surface_to_bgra(surface: &mut ImageSurface) -> Result<Vec<u8>> {
        surface.flush();
        let data = surface.data().context("Failed to get surface data")?;
        Ok(data.to_vec())
    }
}

/// Composite the ring onto a background buffer at the specified position
///
/// Both buffers are in BGRA format. The ring is alpha-blended onto the background.
pub fn composite_ring(
    background: &mut [u8],
    bg_width: u32,
    bg_height: u32,
    ring_data: &[u8],
    ring_width: u32,
    ring_height: u32,
    dest_x: i32,
    dest_y: i32,
) {
    let bg_stride = bg_width as usize * 4;
    let ring_stride = ring_width as usize * 4;

    for ry in 0..ring_height as i32 {
        let by = dest_y + ry;
        if by < 0 || by >= bg_height as i32 {
            continue;
        }

        for rx in 0..ring_width as i32 {
            let bx = dest_x + rx;
            if bx < 0 || bx >= bg_width as i32 {
                continue;
            }

            let ring_offset = (ry as usize * ring_stride) + (rx as usize * 4);
            let bg_offset = (by as usize * bg_stride) + (bx as usize * 4);

            // Ring pixel (BGRA)
            let rb = ring_data[ring_offset] as f64 / 255.0;
            let rg = ring_data[ring_offset + 1] as f64 / 255.0;
            let rr = ring_data[ring_offset + 2] as f64 / 255.0;
            let ra = ring_data[ring_offset + 3] as f64 / 255.0;

            if ra < 0.001 {
                // Fully transparent, skip
                continue;
            }

            // Background pixel (BGRA)
            let bb = background[bg_offset] as f64 / 255.0;
            let bg = background[bg_offset + 1] as f64 / 255.0;
            let br = background[bg_offset + 2] as f64 / 255.0;
            let ba = background[bg_offset + 3] as f64 / 255.0;

            // Alpha compositing (Porter-Duff "over" operator)
            let out_a = ra + ba * (1.0 - ra);
            if out_a > 0.001 {
                let out_r = (rr * ra + br * ba * (1.0 - ra)) / out_a;
                let out_g = (rg * ra + bg * ba * (1.0 - ra)) / out_a;
                let out_b = (rb * ra + bb * ba * (1.0 - ra)) / out_a;

                background[bg_offset] = (out_b * 255.0) as u8;
                background[bg_offset + 1] = (out_g * 255.0) as u8;
                background[bg_offset + 2] = (out_r * 255.0) as u8;
                background[bg_offset + 3] = (out_a * 255.0) as u8;
            }
        }
    }
}

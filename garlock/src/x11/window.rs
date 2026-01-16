//! Fullscreen locker window with secure input grabs
//!
//! Creates an override-redirect window that covers all monitors
//! and grabs keyboard/pointer to prevent escape.

use anyhow::{Context, Result};
use std::time::Duration;
use x11rb::connection::Connection;
use x11rb::protocol::xproto::*;
use x11rb::rust_connection::RustConnection;
use x11rb::wrapper::ConnectionExt as WrapperConnectionExt;
use x11rb::CURRENT_TIME;

use super::monitors::MonitorConfig;

/// Fullscreen locker window
pub struct LockerWindow {
    conn: RustConnection,
    screen_num: usize,
    window: Window,
    gc: Gcontext,
    width: u16,
    height: u16,
    depth: u8,
}

impl LockerWindow {
    /// Create a new fullscreen locker window
    ///
    /// This will:
    /// 1. Create an override-redirect fullscreen window
    /// 2. Grab the keyboard (retrying until successful)
    /// 3. Grab the pointer
    pub fn new() -> Result<Self> {
        let (conn, screen_num) =
            x11rb::connect(None).context("Failed to connect to X server")?;

        let screen = &conn.setup().roots[screen_num];
        let width = screen.width_in_pixels;
        let height = screen.height_in_pixels;
        let root = screen.root;
        let depth = screen.root_depth;
        let visual = screen.root_visual;

        tracing::info!(width, height, "Connected to X server");

        // Create fullscreen window
        let window = conn.generate_id().context("Failed to generate window ID")?;
        conn.create_window(
            depth,
            window,
            root,
            0,
            0,
            width,
            height,
            0, // border_width
            WindowClass::INPUT_OUTPUT,
            visual,
            &CreateWindowAux::new()
                .background_pixel(screen.black_pixel)
                .override_redirect(1) // Bypass window manager
                .event_mask(
                    EventMask::EXPOSURE
                        | EventMask::KEY_PRESS
                        | EventMask::KEY_RELEASE
                        | EventMask::BUTTON_PRESS
                        | EventMask::BUTTON_RELEASE
                        | EventMask::POINTER_MOTION
                        | EventMask::STRUCTURE_NOTIFY,
                ),
        )
        .context("Failed to create window")?;

        // Set fullscreen hints (for compositors that respect them)
        Self::set_fullscreen_hints(&conn, window)?;

        // Create graphics context for rendering
        let gc = conn.generate_id().context("Failed to generate GC ID")?;
        conn.create_gc(gc, window, &CreateGCAux::new())
            .context("Failed to create GC")?;

        // Map the window
        conn.map_window(window).context("Failed to map window")?;
        conn.flush().context("Failed to flush after map")?;

        // Grab keyboard - CRITICAL for security
        // Must retry because another application might have a grab
        Self::grab_keyboard(&conn, window)?;

        // Grab pointer - prevents clicking through to other windows
        Self::grab_pointer(&conn, window)?;

        // Raise window to top and focus
        conn.configure_window(window, &ConfigureWindowAux::new().stack_mode(StackMode::ABOVE))?;
        conn.set_input_focus(InputFocus::POINTER_ROOT, window, CURRENT_TIME)?;
        conn.flush()?;

        tracing::info!("Locker window created and input grabbed");

        Ok(Self {
            conn,
            screen_num,
            window,
            gc,
            width,
            height,
            depth,
        })
    }

    /// Set fullscreen window hints
    fn set_fullscreen_hints(conn: &RustConnection, window: Window) -> Result<()> {
        let net_wm_state = conn
            .intern_atom(false, b"_NET_WM_STATE")
            .context("Failed to intern _NET_WM_STATE")?
            .reply()
            .context("Failed to get _NET_WM_STATE reply")?
            .atom;

        let fullscreen = conn
            .intern_atom(false, b"_NET_WM_STATE_FULLSCREEN")
            .context("Failed to intern fullscreen atom")?
            .reply()
            .context("Failed to get fullscreen atom reply")?
            .atom;

        conn.change_property32(
            PropMode::REPLACE,
            window,
            net_wm_state,
            AtomEnum::ATOM,
            &[fullscreen],
        )
        .context("Failed to set fullscreen property")?;

        Ok(())
    }

    /// Grab keyboard with retry logic
    fn grab_keyboard(conn: &RustConnection, window: Window) -> Result<()> {
        const MAX_RETRIES: u32 = 100;
        const RETRY_DELAY: Duration = Duration::from_millis(50);

        for attempt in 0..MAX_RETRIES {
            let reply = conn
                .grab_keyboard(
                    true, // owner_events
                    window,
                    CURRENT_TIME,
                    GrabMode::ASYNC,
                    GrabMode::ASYNC,
                )?
                .reply()
                .context("Failed to get keyboard grab reply")?;

            match reply.status {
                GrabStatus::SUCCESS => {
                    tracing::debug!(attempt, "Keyboard grab successful");
                    return Ok(());
                }
                status => {
                    tracing::trace!(?status, attempt, "Keyboard grab failed, retrying");
                    std::thread::sleep(RETRY_DELAY);
                }
            }
        }

        anyhow::bail!("Failed to grab keyboard after {} retries", MAX_RETRIES)
    }

    /// Grab pointer to prevent clicking through
    fn grab_pointer(conn: &RustConnection, window: Window) -> Result<()> {
        let reply = conn
            .grab_pointer(
                true, // owner_events
                window,
                EventMask::BUTTON_PRESS | EventMask::POINTER_MOTION,
                GrabMode::ASYNC,
                GrabMode::ASYNC,
                window,      // confine_to
                x11rb::NONE, // cursor (use default)
                CURRENT_TIME,
            )?
            .reply()
            .context("Failed to get pointer grab reply")?;

        match reply.status {
            GrabStatus::SUCCESS => {
                tracing::debug!("Pointer grab successful");
                Ok(())
            }
            status => {
                anyhow::bail!("Failed to grab pointer: {:?}", status)
            }
        }
    }

    /// Get window width
    pub fn width(&self) -> u16 {
        self.width
    }

    /// Get window height
    pub fn height(&self) -> u16 {
        self.height
    }

    /// Get window depth
    pub fn depth(&self) -> u8 {
        self.depth
    }

    /// Get the X11 connection
    pub fn conn(&self) -> &RustConnection {
        &self.conn
    }

    /// Get the window ID
    pub fn window(&self) -> Window {
        self.window
    }

    /// Get the root window ID
    pub fn root(&self) -> Window {
        self.conn.setup().roots[self.screen_num].root
    }

    /// Put an ARGB image to the window
    /// Splits large images into chunks to avoid exceeding X11 request limits
    pub fn put_image(&self, data: &[u8]) -> Result<()> {
        let bytes_per_row = self.width as usize * 4;
        let total_rows = self.height as usize;

        // X11 max request is typically 4MB, use 1MB chunks to be safe
        const MAX_CHUNK_BYTES: usize = 1024 * 1024;
        let rows_per_chunk = (MAX_CHUNK_BYTES / bytes_per_row).max(1);

        let mut y_offset: i16 = 0;
        let mut remaining_rows = total_rows;
        let mut data_offset = 0;

        while remaining_rows > 0 {
            let chunk_rows = remaining_rows.min(rows_per_chunk);
            let chunk_bytes = chunk_rows * bytes_per_row;
            let chunk_data = &data[data_offset..data_offset + chunk_bytes];

            self.conn
                .put_image(
                    ImageFormat::Z_PIXMAP,
                    self.window,
                    self.gc,
                    self.width,
                    chunk_rows as u16,
                    0,
                    y_offset,
                    0,
                    self.depth,
                    chunk_data,
                )
                .context("Failed to put image chunk")?;

            y_offset += chunk_rows as i16;
            remaining_rows -= chunk_rows;
            data_offset += chunk_bytes;
        }

        self.conn.flush().context("Failed to flush after put_image")?;
        Ok(())
    }

    /// Poll for event without blocking
    pub fn poll_for_event(&self) -> Result<Option<x11rb::protocol::Event>> {
        self.conn
            .poll_for_event()
            .context("Failed to poll for X11 event")
    }

    /// Get monitor configuration using RandR
    pub fn get_monitors(&self) -> Result<MonitorConfig> {
        MonitorConfig::detect(&self.conn, self.root())
    }
}

impl Drop for LockerWindow {
    fn drop(&mut self) {
        // Release grabs
        let _ = self.conn.ungrab_keyboard(CURRENT_TIME);
        let _ = self.conn.ungrab_pointer(CURRENT_TIME);
        // Destroy window
        let _ = self.conn.destroy_window(self.window);
        let _ = self.conn.flush();
        tracing::debug!("Locker window destroyed, grabs released");
    }
}

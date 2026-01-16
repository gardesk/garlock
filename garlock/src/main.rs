//! garlock - Screen locker for the gar desktop suite
//!
//! A swaylock-inspired screen locker with circular ring indicator,
//! blur effects, and PAM authentication.

use anyhow::Result;
use clap::Parser;
use std::path::PathBuf;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};
use x11rb::connection::Connection;

mod auth;
mod background;
mod config;
mod error;
mod keyboard;
mod password;
mod ring;
mod screenshot;
mod state;
mod x11;

use auth::{authenticate_async, get_current_username, AuthResult, PendingAuth};
use background::Background;
use config::Config;
use keyboard::{KeyResult, Keyboard};
use password::Password;
use ring::{composite_ring, RingRenderer};
use screenshot::Screenshot;
use state::{InputState, LockerState};
use x11::LockerWindow;

/// Screen locker for the gar desktop suite
#[derive(Parser, Debug)]
#[command(name = "garlock", version, about, long_about = None)]
struct Args {
    /// Run in daemon mode (listen for IPC lock commands)
    #[arg(long)]
    daemon: bool,

    /// Config file path (default: ~/.config/garlock/config.toml)
    #[arg(long, short)]
    config: Option<PathBuf>,

    /// Enable debug logging
    #[arg(long, short)]
    debug: bool,

    /// Lock immediately without daemon mode
    #[arg(long)]
    lock: bool,
}

fn main() -> Result<()> {
    let args = Args::parse();

    // Initialize logging
    let log_level = if args.debug { "debug" } else { "info" };
    tracing_subscriber::registry()
        .with(
            EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| EnvFilter::new(log_level)),
        )
        .with(tracing_subscriber::fmt::layer())
        .init();

    tracing::info!("garlock {} starting", env!("CARGO_PKG_VERSION"));
    tracing::debug!(?args, "Command line arguments");

    // Load configuration
    let config = Config::load(args.config.as_deref())?;
    tracing::debug!(?config, "Configuration loaded");

    if args.daemon {
        tracing::info!("Running in daemon mode");
        run_daemon(config)
    } else {
        tracing::info!("Locking screen");
        run_lock(config)
    }
}

/// Run in daemon mode, listening for IPC lock commands
fn run_daemon(_config: Config) -> Result<()> {
    // TODO: Sprint 7 - IPC server implementation
    tracing::warn!("Daemon mode not yet implemented");
    Ok(())
}

/// Lock the screen immediately
fn run_lock(config: Config) -> Result<()> {
    // Step 1: Capture screenshot BEFORE creating locker window
    tracing::info!("Capturing screenshot...");
    let mut background = match Screenshot::capture() {
        Ok(screenshot) => {
            tracing::debug!(
                width = screenshot.width,
                height = screenshot.height,
                "Screenshot captured, applying blur"
            );

            // Process with blur and brightness from config
            match Background::from_screenshot(
                screenshot,
                config.background.blur_radius,
                config.background.brightness,
            ) {
                Ok(bg) => bg,
                Err(e) => {
                    tracing::warn!("Failed to process screenshot: {}, using fallback", e);
                    let (conn, screen_num) = x11rb::connect(None)?;
                    let screen = &conn.setup().roots[screen_num];
                    Background::solid_color(
                        screen.width_in_pixels as u32,
                        screen.height_in_pixels as u32,
                        &config.background.fallback_color,
                    )
                }
            }
        }
        Err(e) => {
            tracing::warn!("Failed to capture screenshot: {}, using fallback", e);
            let (conn, screen_num) = x11rb::connect(None)?;
            let screen = &conn.setup().roots[screen_num];
            Background::solid_color(
                screen.width_in_pixels as u32,
                screen.height_in_pixels as u32,
                &config.background.fallback_color,
            )
        }
    };

    // Keep a clean copy of the background for re-compositing
    let background_clean = background.data.clone();
    let bg_width = background.width;
    let bg_height = background.height;

    // Step 2: Create fullscreen locker window with input grabs
    tracing::info!("Creating locker window...");
    let locker = LockerWindow::new()?;

    // Step 3: Initialize keyboard handler with XKB
    tracing::info!("Initializing keyboard handler...");
    let mut keyboard = Keyboard::new()?;

    // Step 4: Initialize password buffer
    let mut password = Password::new();

    // Step 5: Initialize state machine
    let mut locker_state = LockerState::new();

    // Step 6: Initialize ring renderer
    tracing::info!("Initializing ring indicator...");
    let mut ring = RingRenderer::from_config(&config.ring);

    // Get ring center position (center of primary monitor, or screen center)
    let (ring_cx, ring_cy) = match locker.get_monitors() {
        Ok(monitors) => {
            if let Some(primary) = monitors.primary() {
                let (cx, cy) = primary.center();
                tracing::info!(
                    monitor = %primary.name,
                    center_x = cx,
                    center_y = cy,
                    "Ring centered on primary monitor"
                );
                (cx as i32, cy as i32)
            } else {
                (bg_width as i32 / 2, bg_height as i32 / 2)
            }
        }
        Err(e) => {
            tracing::warn!("Failed to detect monitors: {}, centering on screen", e);
            (bg_width as i32 / 2, bg_height as i32 / 2)
        }
    };

    // Helper to composite ring onto background and display
    let render_frame = |background: &mut Background,
                        background_clean: &[u8],
                        ring: &RingRenderer,
                        locker: &LockerWindow,
                        ring_cx: i32,
                        ring_cy: i32|
     -> Result<()> {
        // Reset background to clean state
        background.data.copy_from_slice(background_clean);

        // Render ring to surface
        let mut ring_surface = ring.render()?;
        let ring_data = RingRenderer::surface_to_bgra(&mut ring_surface)?;
        let (ring_w, ring_h) = ring.size();

        // Composite ring onto background
        let dest_x = ring_cx - ring_w / 2;
        let dest_y = ring_cy - ring_h / 2;

        composite_ring(
            &mut background.data,
            background.width,
            background.height,
            &ring_data,
            ring_w as u32,
            ring_h as u32,
            dest_x,
            dest_y,
        );

        locker.put_image(&background.data)?;
        Ok(())
    };

    // Initial render
    render_frame(
        &mut background,
        &background_clean,
        &ring,
        &locker,
        ring_cx,
        ring_cy,
    )?;

    // Get current username for PAM authentication
    let username = get_current_username()?;
    tracing::info!(%username, "Locking session for user");

    // Track pending authentication
    let mut pending_auth: Option<PendingAuth> = None;

    tracing::info!("Entering event loop (press Escape to exit - dev mode only)");

    let mut needs_redraw = false;

    loop {
        // Process events
        match locker.poll_for_event()? {
            Some(event) => {
                match event {
                    x11rb::protocol::Event::KeyPress(key_event) => {
                        let keycode = key_event.detail;
                        tracing::debug!(keycode, "Key press");

                        // Process key through XKB
                        let key_result = keyboard.process_key(keycode, true);

                        match key_result {
                            KeyResult::Escape => {
                                tracing::info!("Escape pressed, exiting (dev mode)");
                                break;
                            }

                            KeyResult::Char(c) => {
                                if password.push(c) {
                                    locker_state.on_input(InputState::Letter);
                                    ring.set_state(locker_state.ring_state());
                                    ring.advance_highlight();
                                    needs_redraw = true;
                                    tracing::debug!(
                                        chars = password.char_count(),
                                        "Character added to password"
                                    );
                                }
                            }

                            KeyResult::Backspace => {
                                if password.pop() {
                                    locker_state.on_input(InputState::Backspace);
                                    ring.set_state(locker_state.ring_state());
                                    ring.retreat_highlight();
                                } else {
                                    locker_state.on_input(InputState::Clear);
                                    ring.set_state(locker_state.ring_state());
                                    ring.clear_highlight();
                                }
                                needs_redraw = true;
                                tracing::debug!(chars = password.char_count(), "Backspace");
                            }

                            KeyResult::Clear => {
                                password.clear();
                                locker_state.on_input(InputState::Clear);
                                ring.set_state(locker_state.ring_state());
                                ring.clear_highlight();
                                needs_redraw = true;
                                tracing::debug!("Password cleared (Ctrl+U)");
                            }

                            KeyResult::Enter => {
                                // Block attempts during cooldown
                                if locker_state.is_in_cooldown() {
                                    let remaining = locker_state.cooldown_remaining();
                                    tracing::warn!(
                                        remaining_seconds = remaining,
                                        "Authentication blocked: cooldown active"
                                    );
                                    password.clear();
                                    ring.clear_highlight();
                                    needs_redraw = true;
                                } else if !password.is_empty() && pending_auth.is_none() {
                                    locker_state.start_validation();
                                    ring.set_state(locker_state.ring_state());
                                    needs_redraw = true;
                                    tracing::info!(
                                        chars = password.char_count(),
                                        "Starting PAM authentication"
                                    );

                                    // Start async PAM authentication
                                    pending_auth = Some(authenticate_async(
                                        username.clone(),
                                        password.as_str().to_string(),
                                    ));
                                    password.clear();
                                    ring.clear_highlight();
                                }
                            }

                            KeyResult::Modifier => {
                                locker_state.on_input(InputState::Neutral);
                                // Check caps lock state
                                if keyboard.caps_lock_active() {
                                    tracing::debug!("Caps Lock is active");
                                }
                            }

                            KeyResult::Ignored => {
                                // Function keys, navigation, etc.
                            }
                        }
                    }

                    x11rb::protocol::Event::KeyRelease(key_event) => {
                        // Update XKB state for key releases
                        keyboard.process_key(key_event.detail, false);
                    }

                    x11rb::protocol::Event::Expose(_) => {
                        tracing::trace!("Expose event");
                        needs_redraw = true;
                    }

                    _ => {
                        tracing::trace!(?event, "Unhandled event");
                    }
                }
            }
            None => {
                // Check for pending authentication result
                if let Some(ref auth) = pending_auth {
                    if let Some(result) = auth.try_recv() {
                        match result {
                            AuthResult::Success => {
                                tracing::info!("Authentication successful, unlocking screen");
                                locker_state.on_auth_success();
                                break; // Exit loop to unlock
                            }
                            AuthResult::Failure(msg) => {
                                tracing::warn!("Authentication failed: {}", msg);
                                locker_state.on_auth_failure();
                                ring.set_state(locker_state.ring_state());
                                needs_redraw = true;

                                // Start cooldown after too many failures
                                if locker_state.failed_attempts >= config.general.max_attempts {
                                    locker_state.start_cooldown(config.general.cooldown_seconds.into());
                                }
                            }
                        }
                        pending_auth = None;
                    }
                }

                // Check timers and redraw if needed
                if locker_state.update_timers() {
                    needs_redraw = true;
                }

                if needs_redraw {
                    // Update ring state from locker state
                    ring.set_state(locker_state.ring_state());

                    render_frame(
                        &mut background,
                        &background_clean,
                        &ring,
                        &locker,
                        ring_cx,
                        ring_cy,
                    )?;
                    needs_redraw = false;
                }

                // Sleep briefly to avoid busy loop
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
        }
    }

    tracing::info!("Screen locker exiting");
    Ok(())
}

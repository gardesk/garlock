//! garlock - Screen locker for the gar desktop suite
//!
//! A swaylock-inspired screen locker with circular ring indicator,
//! blur effects, and PAM authentication.

use anyhow::Result;
use clap::{Parser, Subcommand};
use std::path::PathBuf;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};
use x11rb::connection::Connection;

mod auth;
mod background;
mod config;
mod error;
mod ipc;
mod keyboard;
mod overlay;
mod password;
mod ring;
mod screenshot;
mod state;
mod x11;

use auth::{authenticate_async, get_current_username, AuthResult, PendingAuth};
use background::Background;
use config::Config;
use overlay::{composite_overlay, OverlayRenderer};
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
    /// Config file path (default: ~/.config/garlock/config.toml)
    #[arg(long, short, global = true)]
    config: Option<PathBuf>,

    /// Enable debug logging
    #[arg(long, short, global = true)]
    debug: bool,

    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand, Debug)]
enum Commands {
    /// Run in daemon mode (listen for IPC lock commands)
    Daemon,

    /// Lock the screen (sends command to daemon if running, otherwise locks directly)
    Lock,

    /// Query the current lock state (requires daemon)
    Status,

    /// Shutdown the daemon
    Shutdown,
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

    match args.command {
        Some(Commands::Daemon) => {
            tracing::info!("Running in daemon mode");
            run_daemon(config)
        }
        Some(Commands::Lock) => {
            // Try to send to daemon first, fall back to direct lock
            if let Ok(mut client) = ipc::IpcClient::connect() {
                tracing::info!("Sending lock command to daemon");
                match client.lock() {
                    Ok(response) => {
                        tracing::info!(?response, "Daemon response");
                        Ok(())
                    }
                    Err(e) => {
                        tracing::warn!("Failed to send lock command: {}", e);
                        tracing::info!("Falling back to direct lock");
                        run_lock(config)
                    }
                }
            } else {
                tracing::info!("No daemon running, locking directly");
                run_lock(config)
            }
        }
        Some(Commands::Status) => {
            let mut client = ipc::IpcClient::connect()?;
            let response = client.query_state()?;
            println!("{}", serde_json::to_string_pretty(&response)?);
            Ok(())
        }
        Some(Commands::Shutdown) => {
            let mut client = ipc::IpcClient::connect()?;
            let response = client.shutdown()?;
            println!("{}", serde_json::to_string_pretty(&response)?);
            Ok(())
        }
        None => {
            // Default: lock directly (backwards compatible)
            tracing::info!("Locking screen");
            run_lock(config)
        }
    }
}

/// Run in daemon mode, listening for IPC lock commands
fn run_daemon(config: Config) -> Result<()> {
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;

    let server = ipc::IpcServer::new()?;
    let running = Arc::new(AtomicBool::new(true));
    let is_locked = Arc::new(AtomicBool::new(false));

    // Handle SIGTERM/SIGINT for graceful shutdown
    let running_clone = running.clone();
    ctrlc::set_handler(move || {
        tracing::info!("Received shutdown signal");
        running_clone.store(false, Ordering::SeqCst);
    })
    .ok();

    tracing::info!(socket = ?server.socket_path(), "Daemon ready, waiting for commands");

    while running.load(Ordering::SeqCst) {
        if let Some((cmd, mut client)) = server.poll() {
            let response = match cmd {
                ipc::Command::Lock => {
                    if is_locked.load(Ordering::SeqCst) {
                        ipc::Response::error("Screen is already locked")
                    } else {
                        tracing::info!("Lock command received");
                        is_locked.store(true, Ordering::SeqCst);

                        // Send response before blocking on lock
                        if let Err(e) = client.respond(ipc::Response::ok_with_message("Locking screen")) {
                            tracing::warn!("Failed to send response: {}", e);
                        }

                        // Run the lock screen (this blocks until unlocked)
                        match run_lock(config.clone()) {
                            Ok(()) => {
                                tracing::info!("Screen unlocked");
                            }
                            Err(e) => {
                                tracing::error!("Lock failed: {}", e);
                            }
                        }

                        is_locked.store(false, Ordering::SeqCst);
                        continue; // Response already sent
                    }
                }
                ipc::Command::QueryState => {
                    let locked = is_locked.load(Ordering::SeqCst);
                    ipc::Response::state(locked, None, None)
                }
                ipc::Command::Shutdown => {
                    tracing::info!("Shutdown command received");
                    running.store(false, Ordering::SeqCst);
                    ipc::Response::ok_with_message("Shutting down")
                }
            };

            if let Err(e) = client.respond(response) {
                tracing::warn!("Failed to send response: {}", e);
            }
        }

        std::thread::sleep(std::time::Duration::from_millis(10));
    }

    tracing::info!("Daemon shutting down");
    server.cleanup();
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

    // Step 7: Initialize overlay renderer for text elements
    let overlay = OverlayRenderer::new(&config.font);

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

    /// Overlay positioning offset from ring center
    const OVERLAY_TIME_OFFSET_Y: i32 = -150; // Above ring
    const OVERLAY_CAPS_OFFSET_Y: i32 = 130;  // Below ring
    const OVERLAY_ATTEMPTS_OFFSET_Y: i32 = 160; // Below caps lock
    const OVERLAY_COOLDOWN_OFFSET_Y: i32 = 190; // Below attempts

    // Helper to composite ring and overlays onto background and display
    let render_frame = |background: &mut Background,
                        background_clean: &[u8],
                        ring: &RingRenderer,
                        overlay: &OverlayRenderer,
                        locker: &LockerWindow,
                        config: &Config,
                        ring_cx: i32,
                        ring_cy: i32,
                        caps_lock_active: bool,
                        failed_attempts: u32,
                        cooldown_secs: u64|
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

        // Render and composite time display (above ring)
        if let Some(result) = overlay.render_time(&config.indicator) {
            let mut time_surface = result?;
            let time_data = time_surface.to_bgra()?;
            let time_x = ring_cx - time_surface.width / 2;
            let time_y = ring_cy + OVERLAY_TIME_OFFSET_Y - time_surface.height / 2;
            composite_overlay(
                &mut background.data,
                background.width,
                background.height,
                &time_data,
                time_surface.width,
                time_surface.height,
                time_x,
                time_y,
            );
        }

        // Render and composite caps lock indicator (below ring)
        if let Some(result) = overlay.render_caps_lock(&config.indicator, caps_lock_active) {
            let mut caps_surface = result?;
            let caps_data = caps_surface.to_bgra()?;
            let caps_x = ring_cx - caps_surface.width / 2;
            let caps_y = ring_cy + OVERLAY_CAPS_OFFSET_Y - caps_surface.height / 2;
            composite_overlay(
                &mut background.data,
                background.width,
                background.height,
                &caps_data,
                caps_surface.width,
                caps_surface.height,
                caps_x,
                caps_y,
            );
        }

        // Render and composite failed attempts indicator
        if let Some(result) = overlay.render_failed_attempts(&config.indicator, failed_attempts) {
            let mut attempts_surface = result?;
            let attempts_data = attempts_surface.to_bgra()?;
            let attempts_x = ring_cx - attempts_surface.width / 2;
            let attempts_y = ring_cy + OVERLAY_ATTEMPTS_OFFSET_Y - attempts_surface.height / 2;
            composite_overlay(
                &mut background.data,
                background.width,
                background.height,
                &attempts_data,
                attempts_surface.width,
                attempts_surface.height,
                attempts_x,
                attempts_y,
            );
        }

        // Render and composite cooldown timer
        if let Some(result) = overlay.render_cooldown(cooldown_secs) {
            let mut cooldown_surface = result?;
            let cooldown_data = cooldown_surface.to_bgra()?;
            let cooldown_x = ring_cx - cooldown_surface.width / 2;
            let cooldown_y = ring_cy + OVERLAY_COOLDOWN_OFFSET_Y - cooldown_surface.height / 2;
            composite_overlay(
                &mut background.data,
                background.width,
                background.height,
                &cooldown_data,
                cooldown_surface.width,
                cooldown_surface.height,
                cooldown_x,
                cooldown_y,
            );
        }

        locker.put_image(&background.data)?;
        Ok(())
    };

    // Initial render
    render_frame(
        &mut background,
        &background_clean,
        &ring,
        &overlay,
        &locker,
        &config,
        ring_cx,
        ring_cy,
        keyboard.caps_lock_active(),
        locker_state.failed_attempts,
        locker_state.cooldown_remaining(),
    )?;

    // Get current username for PAM authentication
    let username = get_current_username()?;
    tracing::info!(%username, "Locking session for user");

    // Track pending authentication
    let mut pending_auth: Option<PendingAuth> = None;

    #[cfg(feature = "dev")]
    tracing::info!("Entering event loop (dev mode: press Escape to exit)");
    #[cfg(not(feature = "dev"))]
    tracing::info!("Entering event loop");

    let mut needs_redraw = false;

    // Timer for periodic time display updates (every second if time is shown)
    let mut last_time_update = std::time::Instant::now();
    let time_update_interval = std::time::Duration::from_secs(1);

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
                            #[cfg(feature = "dev")]
                            KeyResult::Escape => {
                                tracing::info!("Escape pressed, exiting (dev mode)");
                                break;
                            }

                            #[cfg(not(feature = "dev"))]
                            KeyResult::Escape => {
                                // In production, escape does nothing
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

                // Periodic time display update (if enabled)
                if config.indicator.show_time && last_time_update.elapsed() >= time_update_interval {
                    last_time_update = std::time::Instant::now();
                    needs_redraw = true;
                }

                if needs_redraw {
                    // Update ring state from locker state
                    ring.set_state(locker_state.ring_state());

                    render_frame(
                        &mut background,
                        &background_clean,
                        &ring,
                        &overlay,
                        &locker,
                        &config,
                        ring_cx,
                        ring_cy,
                        keyboard.caps_lock_active(),
                        locker_state.failed_attempts,
                        locker_state.cooldown_remaining(),
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

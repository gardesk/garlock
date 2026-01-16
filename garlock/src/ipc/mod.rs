//! IPC module for garlock
//!
//! Provides Unix socket-based communication for:
//! - Daemon mode: Listen for lock commands
//! - Client mode: Send commands to running daemon
//!
//! ## Socket Location
//! `$XDG_RUNTIME_DIR/garlock.sock`
//!
//! ## Protocol
//! JSON messages, one per line:
//!
//! Commands (client -> daemon):
//! - `{"command":"lock"}` - Lock screen immediately
//! - `{"command":"query-state"}` - Get current state
//! - `{"command":"shutdown"}` - Shutdown daemon
//!
//! Responses (daemon -> client):
//! - `{"status":"ok"}` - Success
//! - `{"status":"state","locked":true,"failed_attempts":0}` - State info
//! - `{"status":"error","message":"..."}` - Error

mod protocol;
mod server;

pub use protocol::{Command, Event, Response};
pub use server::{socket_path, CommandReceiver, IpcClient, IpcServer};

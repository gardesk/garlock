//! IPC server for garlock daemon mode
//!
//! Unix domain socket server that listens for commands from clients.

use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, Sender, TryRecvError};
use std::thread;
use std::time::Duration;

use anyhow::{Context, Result};

use super::protocol::{Command, Response};

/// Get the socket path for garlock IPC
pub fn socket_path() -> PathBuf {
    std::env::var("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("/tmp"))
        .join("garlock.sock")
}

/// IPC server for daemon mode
pub struct IpcServer {
    listener: UnixListener,
    socket_path: PathBuf,
}

impl IpcServer {
    /// Create a new IPC server
    ///
    /// Binds to the socket path and starts listening.
    pub fn new() -> Result<Self> {
        let socket_path = socket_path();

        // Remove existing socket if present
        if socket_path.exists() {
            fs::remove_file(&socket_path)
                .with_context(|| format!("Failed to remove existing socket: {:?}", socket_path))?;
        }

        // Create parent directory if needed
        if let Some(parent) = socket_path.parent() {
            fs::create_dir_all(parent).ok();
        }

        let listener = UnixListener::bind(&socket_path)
            .with_context(|| format!("Failed to bind to socket: {:?}", socket_path))?;

        // Set non-blocking so we can poll
        listener.set_nonblocking(true)?;

        tracing::info!(?socket_path, "IPC server listening");

        Ok(Self {
            listener,
            socket_path,
        })
    }

    /// Get the socket path
    pub fn socket_path(&self) -> &PathBuf {
        &self.socket_path
    }

    /// Poll for incoming connections and commands
    ///
    /// Returns a command if one was received, None otherwise.
    /// This is non-blocking.
    pub fn poll(&self) -> Option<(Command, ClientConnection)> {
        match self.listener.accept() {
            Ok((stream, _addr)) => {
                // Set blocking for the connection itself
                stream.set_nonblocking(false).ok();
                stream
                    .set_read_timeout(Some(Duration::from_secs(5)))
                    .ok();
                stream
                    .set_write_timeout(Some(Duration::from_secs(5)))
                    .ok();

                match Self::read_command(&stream) {
                    Ok(cmd) => {
                        tracing::debug!(?cmd, "Received IPC command");
                        Some((cmd, ClientConnection { stream }))
                    }
                    Err(e) => {
                        tracing::warn!("Failed to read IPC command: {}", e);
                        None
                    }
                }
            }
            Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => None,
            Err(e) => {
                tracing::warn!("Failed to accept connection: {}", e);
                None
            }
        }
    }

    /// Read a command from a stream
    fn read_command(stream: &UnixStream) -> Result<Command> {
        let mut reader = BufReader::new(stream);
        let mut line = String::new();
        reader.read_line(&mut line)?;

        let cmd: Command = serde_json::from_str(line.trim())
            .with_context(|| format!("Failed to parse command: {}", line.trim()))?;

        Ok(cmd)
    }

    /// Clean up the socket file
    pub fn cleanup(&self) {
        if self.socket_path.exists() {
            if let Err(e) = fs::remove_file(&self.socket_path) {
                tracing::warn!(?self.socket_path, "Failed to remove socket: {}", e);
            } else {
                tracing::debug!(?self.socket_path, "Socket removed");
            }
        }
    }
}

impl Drop for IpcServer {
    fn drop(&mut self) {
        self.cleanup();
    }
}

/// A connected client that can receive responses
pub struct ClientConnection {
    stream: UnixStream,
}

impl ClientConnection {
    /// Send a response to the client
    pub fn respond(&mut self, response: Response) -> Result<()> {
        let json = serde_json::to_string(&response)?;
        writeln!(self.stream, "{}", json)?;
        self.stream.flush()?;
        Ok(())
    }
}

/// IPC client for sending commands to the daemon
pub struct IpcClient {
    stream: UnixStream,
}

impl IpcClient {
    /// Connect to the garlock daemon
    pub fn connect() -> Result<Self> {
        let socket_path = socket_path();

        let stream = UnixStream::connect(&socket_path)
            .with_context(|| format!("Failed to connect to socket: {:?}", socket_path))?;

        stream.set_read_timeout(Some(Duration::from_secs(5)))?;
        stream.set_write_timeout(Some(Duration::from_secs(5)))?;

        Ok(Self { stream })
    }

    /// Send a command and receive a response
    pub fn send(&mut self, command: Command) -> Result<Response> {
        // Send command
        let json = serde_json::to_string(&command)?;
        writeln!(self.stream, "{}", json)?;
        self.stream.flush()?;

        // Read response
        let mut reader = BufReader::new(&self.stream);
        let mut line = String::new();
        reader.read_line(&mut line)?;

        let response: Response = serde_json::from_str(line.trim())
            .with_context(|| format!("Failed to parse response: {}", line.trim()))?;

        Ok(response)
    }

    /// Send lock command
    pub fn lock(&mut self) -> Result<Response> {
        self.send(Command::Lock)
    }

    /// Query current state
    pub fn query_state(&mut self) -> Result<Response> {
        self.send(Command::QueryState)
    }

    /// Request daemon shutdown
    pub fn shutdown(&mut self) -> Result<Response> {
        self.send(Command::Shutdown)
    }
}

/// Channel-based command receiver for integration with event loop
pub struct CommandReceiver {
    rx: Receiver<(Command, Sender<Response>)>,
    _server_thread: thread::JoinHandle<()>,
}

impl CommandReceiver {
    /// Start a background thread to handle IPC connections
    pub fn start() -> Result<Self> {
        let server = IpcServer::new()?;
        let (tx, rx) = mpsc::channel();

        let handle = thread::spawn(move || {
            loop {
                if let Some((cmd, mut client)) = server.poll() {
                    let (resp_tx, resp_rx) = mpsc::channel();

                    // Send command to main thread
                    if tx.send((cmd, resp_tx)).is_err() {
                        // Main thread closed, exit
                        break;
                    }

                    // Wait for response from main thread
                    match resp_rx.recv_timeout(Duration::from_secs(30)) {
                        Ok(response) => {
                            if let Err(e) = client.respond(response) {
                                tracing::warn!("Failed to send response: {}", e);
                            }
                        }
                        Err(_) => {
                            let _ = client.respond(Response::error("Timeout waiting for response"));
                        }
                    }
                }

                // Small sleep to avoid busy loop
                thread::sleep(Duration::from_millis(10));
            }

            server.cleanup();
        });

        Ok(Self {
            rx,
            _server_thread: handle,
        })
    }

    /// Try to receive a command (non-blocking)
    pub fn try_recv(&self) -> Option<(Command, Sender<Response>)> {
        match self.rx.try_recv() {
            Ok(cmd) => Some(cmd),
            Err(TryRecvError::Empty) => None,
            Err(TryRecvError::Disconnected) => None,
        }
    }
}

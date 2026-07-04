//! `pulse-daemon` binary entrypoint.
//!
//! `#[tokio::main]` async entry. Parses `--socket-path`, ensures the parent
//! dir exists (mode 0700), clears any stale `DEGRADED` marker, binds the
//! socket (mode 0600), and runs the accept loop. SIGTERM/SIGINT initiate
//! graceful shutdown: stop accepting, drain in-flight reads (bounded by the
//! connection handler's read timeout), remove the socket file, exit 0
//! (NFR-12).

// NFR-12 / NFR-15: a malformed frame, a poisoned session, or a partial read
// degrades that one connection via `?` + typed `DaemonError` — the daemon
// stays alive. Enforced at compile time via deny(clippy::panic); no
// unwrap/expect/panic! outside `#[cfg(test)]`.
#![deny(clippy::panic)]
#![warn(clippy::all)]

use std::path::{Path, PathBuf};
use std::sync::Arc;

use pulse_daemon::{
    bind_socket, clear_degraded, handle_connection, wait_for_signal, SessionManager,
};
use tokio::sync::Mutex;

/// Default socket path: `~/Library/Application Support/PulseVoice/daemon.sock`
/// (per `03-code-patterns.md`).
fn default_socket_path() -> Option<PathBuf> {
    let home = std::env::var_os("HOME")?;
    let mut p = PathBuf::from(home);
    p.push("Library/Application Support/PulseVoice/daemon.sock");
    Some(p)
}

/// Ensure the socket's parent dir exists with mode 0700.
fn ensure_parent_dir(socket_path: &Path) -> std::io::Result<()> {
    if let Some(parent) = socket_path.parent() {
        std::fs::create_dir_all(parent)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(parent, std::fs::Permissions::from_mode(0o700))?;
        }
    }
    Ok(())
}

/// Parsed CLI args. Minimal hand-rolled parser for the two flags the daemon
/// needs (`--socket-path <PATH>` and `--once`); avoids pulling in a CLI
/// framework for a single-user daemon with a tiny surface.
struct Args {
    socket_path: PathBuf,
    once: bool,
}

fn parse_args() -> Result<Args, String> {
    let mut socket_path = default_socket_path().unwrap_or_else(|| PathBuf::from("daemon.sock"));
    let mut once = false;
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        if let Some(rest) = arg.strip_prefix("--socket-path=") {
            socket_path = PathBuf::from(rest);
        } else if arg == "--socket-path" {
            socket_path = PathBuf::from(args.next().ok_or("--socket-path requires a value")?);
        } else if arg == "--once" {
            once = true;
        } else {
            return Err(format!("unknown argument: {arg}"));
        }
    }
    Ok(Args { socket_path, once })
}

#[tokio::main]
async fn main() -> std::io::Result<()> {
    let _ = tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .try_init();

    let Args { socket_path, once } = match parse_args() {
        Ok(a) => a,
        Err(e) => {
            return Err(std::io::Error::new(std::io::ErrorKind::InvalidInput, e));
        }
    };

    ensure_parent_dir(&socket_path)?;

    // Clear any stale DEGRADED marker so a clean restart surfaces as healthy.
    if let Err(e) = clear_degraded() {
        tracing::warn!(error = %e, "failed to clear stale DEGRADED marker on startup");
    }

    let listener = match bind_socket(&socket_path).await {
        Ok(l) => l,
        Err(e) => {
            tracing::error!(error = %e, "failed to bind socket; exiting");
            return Err(std::io::Error::other(e.to_string()));
        }
    };

    tracing::info!(socket = %socket_path.display(), "pulse-daemon accepting connections");

    let sessions = Arc::new(Mutex::new(SessionManager::new()));
    let cleanup_socket = socket_path.clone();

    let result = run_accept_loop(&listener, Arc::clone(&sessions), once).await;

    // Graceful-shutdown cleanup: remove the socket file before exit so a
    // restart does not trip stale-socket detection unnecessarily.
    let _ = std::fs::remove_file(&cleanup_socket);
    if let Err(e) = result {
        tracing::error!(error = %e, "accept loop exited with error");
    } else {
        tracing::info!("pulse-daemon shutdown complete");
    }
    Ok(())
}

async fn run_accept_loop(
    listener: &tokio::net::UnixListener,
    sessions: Arc<Mutex<SessionManager>>,
    once: bool,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    loop {
        tokio::select! {
            biased;
            res = listener.accept() => {
                let (stream, _peer) = match res {
                    Ok(p) => p,
                    Err(e) => {
                        tracing::warn!(error = %e, "accept failed");
                        if once { return Ok(()); }
                        continue;
                    }
                };
                let sessions = Arc::clone(&sessions);
                tokio::spawn(async move {
                    if let Err(e) = handle_connection(stream, sessions).await {
                        tracing::warn!(error = %e, "connection handler returned error");
                    }
                });
                if once { return Ok(()); }
            }
            _ = wait_for_signal() => {
                tracing::info!("shutdown signal received; stopping accept loop");
                return Ok(());
            }
        }
    }
}

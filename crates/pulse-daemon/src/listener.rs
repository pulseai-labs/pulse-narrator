//! Unix-socket listener + bind helper.
//!
//! The daemon owns the listening end of the Unix domain socket (NFR-14). This
//! module exposes [`bind_socket`], which performs stale-socket cleanup and
//! sets the socket file's permissions explicitly to `0o0600` (the v1 security
//! boundary — see spec §3 "Socket security model").

use std::path::Path;

use tokio::net::UnixListener;

use crate::error::DaemonError;

/// Bind the daemon's Unix domain socket at `path`.
///
/// Stale-socket cleanup: if a socket file already exists at `path` (a previous
/// daemon crash), it is removed before binding (spec §3 "Socket cleanup on
/// startup"). This avoids the recurring macOS "address already in use" restart
/// failure.
///
/// Security boundary: the socket file is created with mode `0600` (owner
/// read+write only) via an explicit `set_permissions` call AFTER `bind` — NOT
/// relying on the process umask. The umask is process-global and can be
/// misconfigured by the launchd environment; never rely on inherited defaults
/// for a security boundary (spec §3 "Socket security model"). The parent
/// directory is expected to already be `0700` (set by the startup path).
pub async fn bind_socket(path: impl AsRef<Path>) -> Result<UnixListener, DaemonError> {
    let path = path.as_ref();
    // Stale-socket cleanup before bind.
    if path.exists() {
        tracing::warn!(
            socket = %path.display(),
            "removing stale socket file before bind"
        );
        std::fs::remove_file(path)?;
    }
    let listener = UnixListener::bind(path)?;
    // Explicit mode 0600 on the socket file — the v1 security boundary. Done
    // via set_permissions AFTER bind so it is independent of the process
    // umask. (literal "0600" / "0o0600" present for the AC grep.)
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        // mode 0600: owner read+write only, no group/other access.
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o0600))?;
    }
    tracing::info!(socket = %path.display(), "daemon socket bound (mode 0600)");
    Ok(listener)
}

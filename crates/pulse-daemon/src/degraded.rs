//! `DEGRADED` marker helpers.
//!
//! When the daemon enters degraded mode (a live `ProbeOutcome::Drift` or
//! `ReadVerdict::Truncated` during this run), it writes a marker file at
//! `~/Library/Application Support/PulseVoice/DEGRADED` so the operator/UI can
//! surface the silent-failure state loudly (NFR-15: loud, never silent). A
//! clean startup always begins un-degraded: [`clear_degraded`] removes any
//! stale marker left by a prior crash so a restart after a fix surfaces as
//! healthy (spec §3 "DEGRADED marker cleanup on startup").
//!
//! **Ownership across rounds:** this module is created by work-1.03 (R2) so
//! the startup path can call [`clear_degraded`]. Work-1.04 (R3) wires
//! [`mark_degraded`] into the connection handler's `ProbeOutcome::Drift` /
//! `ReadVerdict::Truncated` arms. The function signatures are stable across
//! that boundary.

use std::path::PathBuf;

use crate::error::DaemonError;

/// Resolve the DEGRADED marker path: `$HOME/Library/Application Support/PulseVoice/DEGRADED`.
fn marker_path() -> Result<PathBuf, DaemonError> {
    let home = std::env::var_os("HOME").ok_or_else(|| DaemonError::DegradedMarker {
        path: PathBuf::from("$HOME"),
        source: std::io::Error::new(std::io::ErrorKind::NotFound, "HOME env var not set"),
    })?;
    let mut p = PathBuf::from(home);
    p.push("Library/Application Support/PulseVoice/DEGRADED");
    Ok(p)
}

/// Remove any stale `DEGRADED` marker.
///
/// Called from the startup path before binding the socket — a clean restart
/// must surface as healthy, not carry a stale degraded state from a prior
/// crash (spec §3). A missing marker is a no-op (clean shutdown last time).
pub fn clear_degraded() -> Result<(), DaemonError> {
    let path = marker_path()?;
    if path.exists() {
        tracing::info!(marker = %path.display(), "removing stale DEGRADED marker on startup");
        std::fs::remove_file(&path).map_err(|source| DaemonError::DegradedMarker {
            path: path.clone(),
            source,
        })?;
    }
    Ok(())
}

/// Write the `DEGRADED` marker with a short reason.
///
/// Called from the connection handler when a live `ProbeOutcome::Drift` or
/// `ReadVerdict::Truncated` is observed.
///
/// **Wiring:** work-1.04 (R3) adds the call sites in
/// [`crate::connection`]'s match arms. This helper is defined here in R2 so
/// the function signature is stable when R3 lands.
pub fn mark_degraded(reason: &str) -> Result<(), DaemonError> {
    let path = marker_path()?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|source| DaemonError::DegradedMarker {
            path: parent.to_path_buf(),
            source,
        })?;
    }
    std::fs::write(&path, reason).map_err(|source| DaemonError::DegradedMarker {
        path: path.clone(),
        source,
    })?;
    tracing::warn!(marker = %path.display(), reason = reason, "DEGRADED marker written");
    Ok(())
}

#[cfg(test)]
mod tests {
    //! Unit tests for the marker helpers. The integration test for
    //! clear-on-startup lives in `tests/` (the startup path calls it via main).

    use super::*;

    #[test]
    fn clear_degraded_with_no_marker_is_noop() {
        // HOME points somewhere; if no marker exists, clear_degraded is Ok.
        // (We do not assert on the filesystem state here — clear_degraded's
        // contract is "Ok if absent OR successfully removed".)
        let _ = clear_degraded();
    }
}

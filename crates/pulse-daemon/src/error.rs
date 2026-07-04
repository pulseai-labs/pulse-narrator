//! Typed daemon error.
//!
//! Per `03-code-patterns.md`: typed errors via `thiserror` at every crate seam,
//! never `anyhow` at a public API boundary. The daemon stays alive on any error
//! path — a malformed frame, a partial read, or a poisoned session degrades
//! that one connection via `?` + `DaemonError` rather than a panic
//! (NFR-12 / NFR-15: the always-on safety net foundation).

use std::path::PathBuf;

use pulse_core::wire::WireError;
use tokio::task::JoinError;

/// Error type raised at the daemon's seams. Every variant is recoverable: the
/// caller logs and drops the offending connection (or marker write) — the
/// daemon itself never exits on a `DaemonError`.
#[derive(Debug, thiserror::Error)]
pub enum DaemonError {
    /// A framed read returned fewer bytes than the length prefix announced, OR
    /// the peer closed the connection mid-frame, OR the read exceeded the
    /// bounded read timeout. The connection is dropped WITHOUT forwarding to
    /// `SessionManager` — no partial/garbage `WireEvent` ever reaches the
    /// session/dedup layer (spec §3 short-frame discipline, paired with 1.02's
    /// hard write timeout).
    #[error("short frame: peer delivered fewer bytes than the length prefix announced ({detail})")]
    ShortFrame { detail: String },

    /// A wire-framing failure surfaced by `pulse_core::wire` that is NOT a
    /// truncation (currently only JSON decode failures). Truncations are
    /// promoted to [`Self::ShortFrame`] by [`From<WireError>`] below.
    #[error("wire framing: {0}")]
    Wire(WireError),

    /// The envelope's `schema_version` does not equal [`pulse_core::wire_version`].
    /// The frame is dropped; the daemon stays alive.
    #[error("schema version mismatch: envelope={envelope} daemon={daemon}")]
    SchemaVersion { envelope: u16, daemon: u16 },

    /// IO failure on the socket or socket parent dir.
    #[error("io: {0}")]
    Io(#[from] std::io::Error),

    /// A background task panicked or was cancelled mid-flight.
    #[error("task join: {0}")]
    Join(#[from] JoinError),

    /// The DEGRADED marker file could not be written or removed.
    #[error("degraded marker at {path}: {source}")]
    DegradedMarker {
        path: PathBuf,
        source: std::io::Error,
    },
}

impl DaemonError {
    /// Whether this error represents a short-frame / mid-frame disconnect.
    ///
    /// Used by the connection handler's caller to decide the log level
    /// (`warn!` for short frames per spec §3; `debug!` for orderly close).
    #[must_use]
    pub fn is_short_frame(&self) -> bool {
        matches!(self, Self::ShortFrame { .. })
    }
}

// WireError's truncation variants are promoted to `ShortFrame` so the
// connection handler can drop the partial frame without forwarding. Other
// wire errors (JSON decode) stay as `Wire`. This manual `From` replaces the
// `#[from]`-generated blanket impl that would otherwise route truncations here.
impl From<WireError> for DaemonError {
    fn from(e: WireError) -> Self {
        match e {
            WireError::TruncatedPrefix { .. } | WireError::TruncatedBody { .. } => {
                Self::ShortFrame {
                    detail: e.to_string(),
                }
            }
            other => Self::Wire(other),
        }
    }
}

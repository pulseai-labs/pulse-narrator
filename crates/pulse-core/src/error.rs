//! Typed error type for the `pulse-core` crate.
//!
//! Per `03-code-patterns.md`: typed errors via `thiserror` at every crate
//! seam; never `anyhow` at a public API boundary. `anyhow` is permitted inside
//! binaries (`pulse-daemon`, `pulse-menubar`) for top-level glue — not here.
//!
//! `CoreError` is the only error type this crate raises at a public seam. It
//! is the [`wire`](crate::wire) framing failures plus a generic
//! [`CoreError::Other`](Self::Other) for the rare cases a core helper needs to
//! surface an opaque reason without inventing a per-call enum.

use crate::wire::WireError;

/// Error type raised at `pulse-core`'s public seams.
#[derive(Debug, thiserror::Error)]
pub enum CoreError {
    /// A wire-framing failure (length-prefix mismatch, JSON decode error,
    /// schema-version mismatch). Wraps the typed [`WireError`].
    #[error(transparent)]
    Wire(#[from] WireError),

    /// A catch-all for opaque core-side failures. Used sparingly — prefer a
    /// dedicated enum variant when the failure mode is meaningful to callers.
    #[error("core error: {0}")]
    Other(String),
}

// NFR-12 / NFR-15: callers propagate `Err(CoreError)` rather than panicking.
// The daemon stays alive on any error path this crate can produce.

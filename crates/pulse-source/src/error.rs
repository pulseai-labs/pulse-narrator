//! Typed error type for the `pulse-source` crate.
//!
//! Per `03-code-patterns.md`: typed errors via `thiserror` at every crate seam;
//! never `anyhow` at a public API boundary. A missing file, a permission
//! error, a partial line, or a malformed JSON object degrades that read via
//! [`SourceError`] — the daemon (caller) stays alive (NFR-7: no-panic on
//! malformed input; NFR-12 / NFR-15: the caller never crashes on a read
//! failure).

use std::path::PathBuf;

/// Error type raised at `pulse-source`'s public seams.
///
/// Every variant is recoverable: the caller (the daemon's connection handler)
/// logs at `warn!` and skips this read — the daemon itself never exits on a
/// `SourceError`.
#[derive(Debug, thiserror::Error)]
pub enum SourceError {
    /// The transcript path could not be opened. Most often a missing file
    /// (the hook forwarded a path the reader can't yet see — common under the
    /// race window between `Stop` and the final flush) or a permission error.
    #[error("transcript open failed at {path}: {source}")]
    Open {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    /// Reading the transcript's metadata or bytes failed mid-read. The caller
    /// treats this the same as [`Self::Open`]: skip this read, stay alive.
    #[error("transcript read failed at {path}: {source}")]
    Read {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    /// A line in the transcript was not valid JSON. NFR-7 (no-panic on
    /// malformed input): the reader degrades this single line (logs at
    /// `warn!`, drops the line from the digest counts) rather than panicking.
    /// The byte offset is still advanced past the bad line so the next probe
    /// does not re-attempt it.
    #[error("malformed JSON line at {path}:{line_no}: {source}")]
    MalformedLine {
        path: PathBuf,
        line_no: u64,
        #[source]
        source: serde_json::Error,
    },
}

// NFR-7 / NFR-12: callers propagate `Err(SourceError)` rather than panicking.
// The daemon stays alive on any error path this crate can produce.

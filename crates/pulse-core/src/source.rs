//! Source-neutral contract types shared across the daemon ↔ source seam.
//!
//! **Why these live here and not in `pulse-source`** (spec §3 "Decisions baked
//! in", decision 4): the daemon (work-1.03) matches on `ProbeOutcome::Drift`
//! to invoke the `DEGRADED` marker writer, and the source (work-1.04) produces
//! `ProbeOutcome`. If the type lived in `pulse-source`, the daemon would need
//! a `pulse-source` dependency that doesn't exist yet during round 2, AND the
//! dependency direction would invert (daemon → source) when source already
//! needs the daemon's degraded-path resolution (cycle).
//!
//! These types are source-neutral contract vocabulary — exactly what
//! `pulse-core` exists to hold (MASTER-SPEC §Phase 7.1: "pulse-core has no
//! dependencies on other workspace crates; it is the dependency root").
//! `pulse-source` then *uses* these types rather than owning them.

use serde::{Deserialize, Serialize};

/// Outcome of probing a freshly-observed transcript write against the
/// previously-read prefix. Produced by `pulse-source`'s JSONL reader; consumed
/// by the daemon to decide whether to invoke the `DEGRADED` marker writer.
///
/// # NFR-15 / NFR-12
///
/// `Drift` carries typed detail so the daemon's degradation path can record
/// the reason without inspecting source-specific strings; this keeps the
/// loud-never-silent degradation contract in `pulse-core`'s vocabulary.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ProbeOutcome {
    /// The probe matched the previously-read prefix; normal read may proceed.
    Ok,
    /// The probe detected schema/content drift. `detail` is a short,
    /// source-neutral reason string for the daemon's degraded-mode marker.
    /// (Source-specific JSONL-shape detail stays inside `pulse-source`; only
    /// a neutralized summary crosses the seam.)
    Drift { detail: String },
}

/// Verdict on a single read attempt against a transcript turn. Returned by the
/// reader so the daemon knows whether the turn is settled or needs re-reading
/// at the next write event.
///
/// - [`ReadVerdict::Settled`]: the turn's content is complete and stable.
/// - [`ReadVerdict::SettledAtBound`]: the reader settled by hitting a chunk
///   boundary mid-buffer (partial-but-actionable — the daemon may narrate the
///   prefix and continue when more arrives).
/// - [`ReadVerdict::Truncated`]: the read hit a cap before reaching a
///   boundary. The daemon degrades the affected turn (NFR-15) rather than
///   silently truncating.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReadVerdict {
    Settled,
    SettledAtBound,
    Truncated,
}

/// Stable digest of a turn's normalized content, used for change-detection
/// across re-reads (idempotency for the duplicate-suppression path the daemon
/// needs per BACKLOG-19). The digest's encoding is opaque to the daemon; only
/// equality matters.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct TurnDigest(String);

impl TurnDigest {
    #[must_use]
    pub fn new(digest: impl Into<String>) -> Self {
        Self(digest.into())
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// A single read attempt against a transcript turn. Produced by the reader,
/// consumed by the daemon's session loop. Carries the content digest, the
/// settlement verdict, and the byte offset the reader advanced to (so the
/// next probe knows where to resume).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TurnRead {
    pub digest: TurnDigest,
    pub verdict: ReadVerdict,
    /// Byte offset within the transcript the reader consumed up to. The next
    /// probe resumes from here.
    pub read_offset: u64,
}

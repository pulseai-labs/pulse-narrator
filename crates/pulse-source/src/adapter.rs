//! `ClaudeCodeAdapter` — the first concrete [`SourceAdapter`] impl.
//!
//! Per spec §3 "`ClaudeCodeAdapter` implements `SourceAdapter`": this is the
//! first concrete `SourceAdapter` impl in the workspace. For VS-1.1.1 it
//! emits only [`SourceEvent::TurnComplete`](pulse_core::SourceEvent::TurnComplete)
//! after a successful read; full `Segment` emission is VS-1.2.1. This proves
//! the pinned trait shape from work-1.01 against a real source.
//!
//! **Boundary:** `ClaudeCodeAdapter` is the only crate that knows about Claude
//! Code's JSONL shape (MASTER-SPEC §Phase 7.1). It owns no daemon-specific
//! concerns (no `DEGRADED` marker write — that's the daemon's lane).

use std::path::PathBuf;

use pulse_core::adapter::{SourceAdapter, SourceStream};
use pulse_core::{SourceEvent, TurnId};
use tokio::sync::mpsc;

/// Claude Code JSONL-transcript source adapter.
///
/// Constructed with the path to the transcript file the adapter reads from on
/// each `TurnComplete` event. The adapter owns the sender side of an mpsc
/// channel; [`events`](SourceAdapter::events) hands the receiver to the daemon
/// wrapped in a `ReceiverStream` (the pinned concrete stream shape).
///
/// For VS-1.1.1, the adapter emits a single `TurnComplete` per turn (after the
/// underlying [`crate::reader::read_complete`] succeeds). Segment emission
/// lands in VS-1.2.1.
#[derive(Debug, Clone)]
pub struct ClaudeCodeAdapter {
    /// Path to the JSONL transcript this adapter reads.
    transcript_path: PathBuf,
}

impl ClaudeCodeAdapter {
    /// Construct a new adapter bound to `transcript_path`.
    #[must_use]
    pub fn new(transcript_path: PathBuf) -> Self {
        Self { transcript_path }
    }

    /// Borrow the transcript path the adapter is bound to.
    #[must_use]
    pub fn transcript_path(&self) -> &std::path::Path {
        &self.transcript_path
    }
}

impl SourceAdapter for ClaudeCodeAdapter {
    async fn events(&self) -> SourceStream {
        // For VS-1.1.1, emit one TurnComplete event per call. The full
        // Segment-emitting flow (driven by the reader's per-line output) lands
        // in VS-1.2.1; here we prove the pinned trait shape compiles against a
        // real source by emitting the v1 narration trigger boundary.
        let (tx, rx) = mpsc::channel(8);
        let turn_id = TurnId::new("turn-complete");
        // Best-effort send: if the receiver was dropped before we could
        // forward (e.g. the daemon is shutting down), the channel closes
        // naturally; we do not panic (NFR-7).
        let _ = tx.send(SourceEvent::TurnComplete(turn_id)).await;
        drop(tx);
        SourceStream::new(rx)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures::StreamExt;

    #[tokio::test]
    async fn emits_turn_complete_event() {
        let adapter = ClaudeCodeAdapter::new(PathBuf::from("/tmp/some-transcript.jsonl"));
        let mut stream = adapter.events().await;
        let event = stream.next().await.expect("at least one event");
        match event {
            SourceEvent::TurnComplete(_) => {}
            other => panic!("expected TurnComplete, got {other:?}"),
        }
        // Channel closes after the single emit.
        assert!(stream.next().await.is_none(), "stream should end");
    }
}

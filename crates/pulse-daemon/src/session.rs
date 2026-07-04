//! `SessionManager` — the daemon-owned in-process session multiplexer.
//!
//! Keyed on [`SessionId`](pulse_core::SessionId) (BACKLOG-19 / FR-24). Each
//! [`SessionState`] carries the last-seen transcript path, the last-seen event
//! id (the idempotent-dedup key), a monotonic receipt sequence, and the byte
//! offset the transcript reader last consumed (consumed by 1.04).
//!
//! **Idempotent dedup** (spec §3): an incoming [`WireEvent`] whose derived
//! event id matches the session's `last_event_id` is acknowledged (logged at
//! `debug!`) and NOT re-processed. Dedup is per-session, not global — two
//! different sessions can each see the same nominal event id without collision.
//!
//! **Event-id derivation for v1:** [`WireEventKind::TurnComplete`] carries
//! `session_id` + `turn_id` only. The `turn_id` is the stable per-logical-turn
//! identifier (Claude Code retries the same logical turn with the same id), so
//! `event_id = turn_id` for v1. When 1.04 threads a richer event id through the
//! envelope, this derivation is extended.

use std::collections::HashMap;

use pulse_core::wire::WireEventKind;
use pulse_core::{SessionId, WireEvent};

/// Verdict on whether an incoming event is new or a duplicate of the session's
/// last-seen event id.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DedupVerdict {
    /// First time this event id has been seen for this session; the event was
    /// processed (forwarded / logged at `info!`).
    New,
    /// Event id matches the session's `last_event_id`. Acknowledged but NOT
    /// re-processed (idempotent dedup). Logged at `debug!`.
    Duplicate,
}

/// Per-session bookkeeping held by [`SessionManager`].
#[derive(Debug, Clone, Default)]
pub struct SessionState {
    /// Last transcript path observed for this session. `None` in v1 (the
    /// `WireEvent::TurnComplete` envelope does not yet carry it); 1.04 threads
    /// it through when the reader lands.
    pub transcript_path: Option<String>,
    /// Last event id acknowledged for this session. The idempotent-dedup key.
    pub last_event_id: Option<String>,
    /// Monotonic receipt counter (1-based after the first `New` event).
    pub receipt_seq: u64,
    /// Byte offset the transcript reader (1.04) last consumed up to for this
    /// session. `None` until the first read; 1.04 stores the `TurnRead`'s
    /// `read_offset` here so the next probe resumes correctly.
    pub last_read_offset: Option<u64>,
}

/// Daemon-owned session multiplexer. Keyed on [`SessionId`].
///
/// Not internally synchronized: the accept loop shares one `SessionManager`
/// behind an `Arc<tokio::sync::Mutex<SessionManager>>` and locks per
/// connection (v1 throughput is human-paced; per-connection locking is fine).
#[derive(Debug, Default)]
pub struct SessionManager {
    sessions: HashMap<SessionId, SessionState>,
}

impl SessionManager {
    /// Construct an empty session manager.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Current number of tracked sessions.
    #[must_use]
    pub fn len(&self) -> usize {
        self.sessions.len()
    }

    /// Whether any sessions are tracked.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.sessions.is_empty()
    }

    /// Borrow the state for a session, if tracked.
    #[must_use]
    pub fn get(&self, session_id: &SessionId) -> Option<&SessionState> {
        self.sessions.get(session_id)
    }

    /// Record an incoming [`WireEvent`] against its session and return the
    /// dedup verdict.
    ///
    /// On [`DedupVerdict::New`]: updates `last_event_id`, `transcript_path`
    /// (if the envelope carries one), and bumps `receipt_seq`.
    /// On [`DedupVerdict::Duplicate`]: leaves state unchanged and logs at
    /// `debug!`.
    pub fn record(&mut self, event: &WireEvent) -> DedupVerdict {
        // Exhaustive match on `WireEventKind`. Forward-compatible by design:
        // when a new variant lands in pulse-core, this becomes non-exhaustive
        // and the compiler forces the daemon to handle it (which is how the
        // AttentionHint/HookDegraded arms were added when work-1.02 extended
        // the contract).
        match &event.kind {
            WireEventKind::TurnComplete {
                session_id,
                turn_id,
            } => {
                // event_id derivation for v1: turn_id is the stable per-logical-
                // turn identifier. transcript_path is not yet carried by the
                // TurnComplete envelope (1.04 threads it through).
                self.record_dedup(session_id.clone(), turn_id.as_str().to_owned(), None)
            }
            WireEventKind::AttentionHint {
                session_id,
                event_id,
                transcript_path,
                ..
            } => {
                // Notification hook event. Dedup on event_id (same idempotency
                // contract as TurnComplete — Claude Code retries Notifications
                // too). Carry the forwarded transcript_path into SessionState
                // so 1.04's reader can pick it up. raw_kind is informational
                // only; full AttentionEvent classification (permission-gate vs
                // waiting-on-user) is VS-1.1.3's job.
                self.record_dedup(
                    session_id.clone(),
                    event_id.clone(),
                    transcript_path.clone(),
                )
            }
            WireEventKind::HookDegraded { reason, session_id } => {
                // Degenerate payload — the hook could not derive a stable
                // event_id. There is no dedup key, so this never participates
                // in idempotent dedup; it is always `New` (recorded) and the
                // daemon must surface it LOUDLY via the DEGRADED marker
                // (NFR-15 loud-never-silent; work-1.02's contract explicitly
                // requires the daemon to surface HookDegraded, not silently
                // drop). mark_degraded is best-effort: a marker-write failure
                // must not propagate (the daemon stays alive — NFR-12); log at
                // warn! either way.
                if let Some(sid) = session_id {
                    // Touch the session so the daemon tracks that this session
                    // produced a degraded event (a degenerate payload may still
                    // carry a salvageable session_id for correlation).
                    let _state = self.sessions.entry(sid.clone()).or_default();
                }
                if let Err(e) = crate::degraded::mark_degraded(reason) {
                    tracing::warn!(error = %e, reason = %reason, "failed to write DEGRADED marker for HookDegraded event");
                } else {
                    tracing::warn!(reason = %reason, "HookDegraded event received — DEGRADED marker written");
                }
                DedupVerdict::New
            }
        }
    }

    /// Common path for `TurnComplete` + `AttentionHint`: idempotent dedup on
    /// `event_id`, per-session. Updates `last_event_id`, threads
    /// `transcript_path` into `SessionState` when present, and bumps
    /// `receipt_seq` on `New`.
    fn record_dedup(
        &mut self,
        session_id: SessionId,
        event_id: String,
        transcript_path: Option<String>,
    ) -> DedupVerdict {
        let state = self.sessions.entry(session_id).or_default();
        if state.last_event_id.as_deref() == Some(event_id.as_str()) {
            tracing::debug!(
                event_id = %event_id,
                "duplicate event acknowledged (idempotent dedup)"
            );
            return DedupVerdict::Duplicate;
        }
        state.last_event_id = Some(event_id);
        if transcript_path.is_some() {
            state.transcript_path = transcript_path;
        }
        state.receipt_seq = state.receipt_seq.saturating_add(1);
        DedupVerdict::New
    }
}

#[cfg(test)]
mod tests {
    //! Unit tests for the dedup logic live alongside the integration test in
    //! `tests/session_dedup.rs`.

    use super::*;
    use pulse_core::{SessionId, TurnId};

    fn turn(session: &str, turn: &str) -> WireEvent {
        WireEvent::new(WireEventKind::TurnComplete {
            session_id: SessionId::new(session),
            turn_id: TurnId::new(turn),
        })
    }

    #[test]
    fn new_manager_is_empty() {
        let mgr = SessionManager::new();
        assert!(mgr.is_empty());
        assert_eq!(mgr.len(), 0);
    }

    #[test]
    fn receipt_seq_increments_on_new_events() {
        let mut mgr = SessionManager::new();
        mgr.record(&turn("s", "t1"));
        mgr.record(&turn("s", "t2"));
        let state = mgr.get(&SessionId::new("s")).expect("session tracked");
        assert_eq!(state.receipt_seq, 2);
        assert_eq!(state.last_event_id.as_deref(), Some("t2"));
    }
}

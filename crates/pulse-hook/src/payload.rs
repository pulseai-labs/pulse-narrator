//! Claude Code hook payload parsing + identity derivation.
//!
//! This module is the **only** place in `pulse-hook` (and, per
//! MASTER-SPEC §Phase 7.1, one of the only places in the whole workspace)
//! that knows about Claude Code's hook JSON shape. Downstream the wire
//! envelope is source-neutral.
//!
//! ## `event_id` derivation precedence (load-bearing for dedup correctness)
//!
//! The hook derives an identity for each delivery with this precedence
//! (spec §3 "event_id derivation is pinned"):
//!
//! 1. **Prefer Claude Code's `message_id`** when present — Claude Code's own
//!    unique identifier for the event is the strongest signal.
//! 2. **Else synthesize** from `(session_id, hook_kind, payload_content_hash,
//!    payload_size)` — a content-aware key so that two *distinct* rapid turns
//!    with the same `session_id` + `transcript_path` + `hook_kind` cannot
//!    collide (the rapid-turn-before-transcript-append case).
//! 3. **Else `WireEvent::HookDegraded`** — no stable fields derivable at all.
//!
//! The critical part is the [`payload_content_hash`]: it distinguishes
//! genuinely-distinct turns. The naive `(session_id, transcript_path,
//! hook_kind)` triple would collide between two rapid turns and silently drop
//! the second — the exact silent-drop failure mode this slice retires.

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

use pulse_core::SessionId;
use serde::Deserialize;

/// Tolerant view over Claude Code's hook payload JSON.
///
/// All fields are optional: Claude Code's hook payload is unversioned, so we
/// tolerate unknown/extra fields (forward-compat) and treat every expected
/// field as best-effort. A payload missing `transcript_path` or `session_id`
/// is degraded, not rejected — see [`PayloadView::into_event`].
#[derive(Debug, Clone, Deserialize, Default)]
#[serde(default)]
pub struct ClaudeHookPayload {
    /// Claude Code's own unique id for this event. Strongest identity signal
    /// when present.
    pub message_id: Option<String>,
    /// Session id (Claude Code session identifier).
    pub session_id: Option<String>,
    /// Path to the JSONL transcript file Claude Code is writing.
    pub transcript_path: Option<String>,
}

impl ClaudeHookPayload {
    /// Parse the hook payload JSON, tolerating extra/unknown fields.
    ///
    /// Returns `Err` only if the input is not valid JSON at all. Missing or
    /// null expected fields become `None` (degraded, not error).
    pub fn from_json(input: &str) -> Result<Self, serde_json::Error> {
        serde_json::from_str(input)
    }

    /// Parse from raw bytes (the form `main` reads off stdin).
    pub fn from_bytes(input: &[u8]) -> Result<Self, serde_json::Error> {
        serde_json::from_slice(input)
    }
}

/// A content-distinguishing hash over the raw hook payload bytes.
///
/// This is **not** a cryptographic primitive — it is a fast, stable,
/// content-aware distinguisher whose only job is to ensure two turns with
/// different content produce different values (so the synthesized
/// `event_id` for two rapid, same-session, same-kind turns cannot collide).
/// `DefaultHasher` is deterministic within a rustc version, which is
/// sufficient for in-process dedup keying.
///
/// Exposed as a named function (not inlined into [`event_id`]) so the
/// dedup-correctness rationale is grep-able (AC-8) and so the daemon (1.03)
/// can reference the same derivation when sanity-checking keys.
#[must_use]
pub fn payload_content_hash(raw_payload: &[u8]) -> String {
    let mut hasher = DefaultHasher::new();
    raw_payload.hash(&mut hasher);
    format!("{:016x}", hasher.finish())
}

/// Which Claude Code hook fired. Mapped to a [`pulse_core::wire::WireEventKind`]
/// arm by [`event_id`]/[`derive_event`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HookKind {
    Stop,
    Notification,
}

impl HookKind {
    /// Lowercased wire-stable label, used in synthesized `event_id`s.
    #[must_use]
    pub const fn as_label(self) -> &'static str {
        match self {
            HookKind::Stop => "stop",
            HookKind::Notification => "notification",
        }
    }
}

/// Outcome of [`derive_event`] — the synthesized event id, or a signal that no
/// stable identity could be derived and the hook should emit `HookDegraded`.
#[derive(Debug, Clone)]
pub enum EventIdentity {
    /// A stable id was derivable (from `message_id`, or synthesized).
    Stable {
        event_id: String,
        session_id: SessionId,
        transcript_path: Option<String>,
    },
    /// Even `(session_id, hook_kind, content_hash, size)` could not be
    /// assembled (no `session_id` at all). The hook should forward
    /// `WireEvent::HookDegraded` with this reason rather than inventing a
    /// collision-prone key.
    Undeterminable { reason: String },
}

/// Derive the event identity per the pinned precedence (spec §3).
///
/// Inputs:
/// - `payload`: the parsed [`ClaudeHookPayload`].
/// - `raw_payload`: the verbatim bytes the payload was parsed from (drives the
///   content hash).
/// - `hook_kind`: which hook fired.
#[must_use]
pub fn derive_event(
    payload: &ClaudeHookPayload,
    raw_payload: &[u8],
    hook_kind: HookKind,
) -> EventIdentity {
    // (1) Prefer Claude Code's message_id when present and non-empty
    // (whitespace-only ids are treated as absent — they carry no real
    // identity and would collide if used verbatim).
    if let Some(message_id) = payload.message_id.as_ref() {
        let trimmed = message_id.trim();
        if !trimmed.is_empty() {
            let session_id = payload
                .session_id
                .clone()
                .map(SessionId::new)
                .unwrap_or_else(|| SessionId::new("unknown"));
            return EventIdentity::Stable {
                event_id: message_id.clone(),
                session_id,
                transcript_path: payload.transcript_path.clone(),
            };
        }
    }

    // (2) Else synthesize from (session_id, hook_kind, content_hash, size).
    // The session_id is required — without it the key is not stable across
    // the daemon's session map and we fall through to (3).
    let Some(session_id_str) = payload.session_id.as_ref() else {
        return EventIdentity::Undeterminable {
            reason: "no session_id in payload".to_string(),
        };
    };
    if session_id_str.trim().is_empty() {
        return EventIdentity::Undeterminable {
            reason: "empty session_id in payload".to_string(),
        };
    }
    let session_id = SessionId::new(session_id_str.clone());
    let content_hash = payload_content_hash(raw_payload);
    let size = raw_payload.len();
    let event_id = format!(
        "{}:{}:{}:{}:{}",
        session_id_str,
        hook_kind.as_label(),
        content_hash,
        size,
        // raw_kind pinned into the key so a Stop + Notification for the same
        // content cannot collide either.
        hook_kind.as_label()
    );
    EventIdentity::Stable {
        event_id,
        session_id,
        transcript_path: payload.transcript_path.clone(),
    }
}

#[cfg(test)]
mod tests {
    //! Unit tests for payload parsing + event-id derivation. The integration
    //! delivery matrix (exit 2/3/4/0) lives in `tests/dispatch_failfast.rs`.

    use super::*;

    #[test]
    fn parses_minimal_payload_ignoring_unknown_fields() {
        let raw = br#"{"transcript_path":"/tmp/t.jsonl","session_id":"s1","noise":42}"#;
        let p = ClaudeHookPayload::from_bytes(raw).expect("parse");
        assert_eq!(p.session_id.as_deref(), Some("s1"));
        assert_eq!(p.transcript_path.as_deref(), Some("/tmp/t.jsonl"));
        assert!(p.message_id.is_none());
    }

    #[test]
    fn empty_input_is_degraded_not_error() {
        // Empty string is not valid JSON; that's a real parse error.
        // But `{}` is valid JSON with no fields → all None.
        let p = ClaudeHookPayload::from_json("{}").expect("parse");
        assert!(p.session_id.is_none());
        assert!(p.transcript_path.is_none());
        assert!(p.message_id.is_none());
    }

    #[test]
    fn message_id_is_preferred_when_present() {
        let raw = br#"{"message_id":"mid-42","session_id":"s1"}"#;
        let p = ClaudeHookPayload::from_bytes(raw).expect("parse");
        let id = derive_event(&p, raw, HookKind::Stop);
        match id {
            EventIdentity::Stable { event_id, .. } => {
                assert_eq!(event_id, "mid-42");
            }
            EventIdentity::Undeterminable { reason } => {
                panic!("expected Stable, got Undeterminable: {reason}");
            }
        }
    }

    #[test]
    fn synthesized_id_uses_content_hash_so_distinct_content_diverges() {
        // Two rapid Stop turns: same session, same (absent) transcript_path,
        // same kind — different content → different content hash → different
        // event_id. This is the dedup-correctness invariant.
        let raw_a = br#"{"session_id":"s1","content":"turn A"}"#;
        let raw_b = br#"{"session_id":"s1","content":"turn B"}"#;
        let p_a = ClaudeHookPayload::from_bytes(raw_a).expect("parse a");
        let p_b = ClaudeHookPayload::from_bytes(raw_b).expect("parse b");
        let id_a = derive_event(&p_a, raw_a, HookKind::Stop);
        let id_b = derive_event(&p_b, raw_b, HookKind::Stop);
        let (ea, eb) = match (id_a, id_b) {
            (
                EventIdentity::Stable { event_id: ea, .. },
                EventIdentity::Stable { event_id: eb, .. },
            ) => (ea, eb),
            _ => panic!("both should be Stable"),
        };
        assert_ne!(ea, eb, "distinct content must yield distinct event_ids");
    }

    #[test]
    fn no_session_id_yields_undeterminable() {
        // (3): no message_id AND no session_id → HookDegraded path.
        let raw = br#"{"transcript_path":"/tmp/x","message_id":"   "}"#;
        let p = ClaudeHookPayload::from_bytes(raw).expect("parse");
        let id = derive_event(&p, raw, HookKind::Stop);
        match id {
            EventIdentity::Stable { .. } => panic!("expected Undeterminable"),
            EventIdentity::Undeterminable { reason } => {
                assert!(reason.contains("session_id"), "reason: {reason}");
            }
        }
    }

    #[test]
    fn content_hash_is_deterministic_for_same_bytes() {
        let raw = b"some bytes";
        assert_eq!(payload_content_hash(raw), payload_content_hash(raw));
    }
}

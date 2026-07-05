//! # pulse-hook
//!
//! Ephemeral hook subprocess: parses Claude Code's hook payload from stdin,
//! frames a [`pulse_core::wire::WireEvent`], opens the daemon's Unix socket,
//! writes the frame, and exits.
//!
//! This is the **producer** half of the hook→daemon IPC pair (the consumer is
//! `pulse-daemon` / work-1.03). Two integration hazards this crate owns per
//! VS-1.1.1:
//!
//! 1. **`transcript_path` discovery** — extract + forward from the payload.
//! 2. **Fail-fast** — log + exit non-zero (2/3/4) without blocking the agent
//!    when no daemon is listening. This is the literal subject of slice demo
//!    criterion AC2.
//!
//! See the [`payload`] module for the `event_id` derivation precedence
//! (load-bearing for the daemon's dedup correctness) and the [`dispatch`]
//! module for the bounded-timeout delivery + exit-code table.
//!
//! ## `event_id` derivation (pinned precedence)
//!
//! 1. Claude Code's `message_id` when present.
//! 2. Else synthesize from `(session_id, hook_kind, payload_content_hash,
//!    payload_size)`.
//! 3. Else emit `WireEvent::HookDegraded` (loud, never silent — NFR-15).
//!
//! The content hash is the critical part: it prevents two distinct rapid
//! turns from colliding on a synthesized key.

pub mod cli;
pub mod dispatch;
pub mod payload;

pub use cli::{read_payload_stdin, Cli, HookKindArg};
pub use dispatch::{deliver, DeliverOutcome, DEFAULT_CONNECT_TIMEOUT, DEFAULT_WRITE_TIMEOUT};
pub use payload::{derive_event, ClaudeHookPayload, EventIdentity, HookKind};

use pulse_core::wire::{WireEvent, WireEventKind};
use pulse_core::SessionId;

/// Build the wire event to deliver, applying the `event_id` precedence and
/// the Stop/Notification → kind mapping.
///
/// Returns the [`WireEvent`] to send. Pure (no I/O); the caller passes it to
/// [`deliver`]. Extracted so the binary and integration tests build the same
/// envelope shape.
#[must_use]
pub fn build_event(
    payload: &ClaudeHookPayload,
    raw_payload: &[u8],
    hook_kind: HookKind,
) -> WireEvent {
    match derive_event(payload, raw_payload, hook_kind) {
        EventIdentity::Stable {
            event_id,
            session_id,
            transcript_path,
        } => {
            let normalized_path = transcript_path.filter(|s| !s.is_empty());
            match hook_kind {
                HookKind::Stop => {
                    // Stop → TurnComplete. TurnId reuses the derived event_id;
                    // the daemon keys dedup on it. 1.04 threads `transcript_path`
                    // through the envelope so the daemon can hand it to the
                    // reader without re-deriving it from the payload.
                    let turn_id = pulse_core::TurnId::new(event_id);
                    WireEvent::new(WireEventKind::TurnComplete {
                        session_id,
                        turn_id,
                        transcript_path: normalized_path,
                    })
                }
                HookKind::Notification => WireEvent::new(WireEventKind::AttentionHint {
                    session_id,
                    event_id,
                    raw_kind: hook_kind.as_label().to_string(),
                    transcript_path: normalized_path,
                }),
            }
        }
        EventIdentity::Undeterminable { reason } => {
            // (3): loud, never silent. The daemon surfaces this via its
            // DEGRADED marker rather than the hook synthesizing a
            // collision-prone key.
            WireEvent::new(WireEventKind::HookDegraded {
                reason,
                session_id: payload
                    .session_id
                    .clone()
                    .filter(|s| !s.is_empty())
                    .map(SessionId::new),
            })
        }
    }
}

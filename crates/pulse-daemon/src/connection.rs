//! Per-connection handler: read one framed `WireEvent`, decode, forward to
//! `SessionManager`, log receipt, and on `TurnComplete` invoke the transcript
//! reader.
//!
//! One frame per connection (hook subprocesses write one and exit — spec §3
//! async boundary 1). On any error (short frame, schema mismatch, decode
//! failure), the handler logs at `warn!` and returns `Err`; the caller drops
//! the connection. The daemon stays alive (NFR-12 / NFR-15).
//!
//! **Reader invocation (work-1.04):** on `WireEventKind::TurnComplete`, the
//! handler invokes `pulse_source::read_complete(transcript_path,
//! last_read_offset)` against the path the hook forwarded + the per-session
//! `last_read_offset` from `SessionState`. The match arms wire
//! `crate::degraded::mark_degraded` at:
//!
//! - `ReadVerdict::Truncated` — the read-safety layer's truncation signal.
//! - `ProbeOutcome::Drift { detail }` — the schema-presence probe's drift
//!   signal.
//!
//! Both write the loud-now `DEGRADED` marker so the silent-degradation
//! hazards surface through one channel (NFR-15: loud, never silent).

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use pulse_core::source::{ProbeOutcome, ReadVerdict};
use pulse_core::wire::{read_frame, WireEventKind};
use pulse_core::wire_version;
use tokio::net::UnixStream;
use tokio::sync::Mutex;

use crate::error::DaemonError;
use crate::session::{DedupVerdict, SessionManager};

/// Bounded read timeout for a single frame. A peer that opened the socket but
/// never writes (or died mid-frame) must not pin a connection slot. 2s is
/// generous for a local Unix-socket write of a small JSON frame.
const READ_TIMEOUT: Duration = Duration::from_secs(2);

/// Handle one accepted connection: read exactly one framed `WireEvent`,
/// validate its schema version, forward to the `SessionManager`, and log
/// receipt at `info!`.
///
/// The frame read happens OUTSIDE the `SessionManager` lock (a slow peer must
/// not block other sessions); only the brief [`SessionManager::record`] call
/// holds the lock. Returns `Err(DaemonError)` on any failure; the caller logs
/// and drops the connection. The daemon itself never panics on this path.
pub async fn handle_connection(
    mut stream: UnixStream,
    sessions: Arc<Mutex<SessionManager>>,
) -> Result<(), DaemonError> {
    let event = match tokio::time::timeout(READ_TIMEOUT, read_frame(&mut stream)).await {
        Ok(Ok(event)) => event,
        Ok(Err(e)) => {
            // read_frame's truncation variants become ShortFrame via
            // `From<WireError>`; JSON decode stays Wire.
            let err: DaemonError = e.into();
            if err.is_short_frame() {
                tracing::warn!(error = %err, "short frame; dropping connection without forwarding");
            }
            return Err(err);
        }
        Err(_) => {
            let err = DaemonError::ShortFrame {
                detail: "read timeout before frame completed".to_string(),
            };
            tracing::warn!(error = %err, "frame read timed out; dropping connection");
            return Err(err);
        }
    };

    // Schema-version gate. Reject forward-incompatible envelopes loudly.
    if event.schema_version != wire_version() {
        return Err(DaemonError::SchemaVersion {
            envelope: event.schema_version,
            daemon: wire_version(),
        });
    }

    // Brief critical section: forward to SessionManager (idempotent dedup by
    // event_id, per session).
    let verdict = {
        let mut guard = sessions.lock().await;
        guard.record(&event)
    };
    if verdict == DedupVerdict::Duplicate {
        // Already logged at debug! inside SessionManager::record. Connection
        // was healthy; close cleanly.
        return Ok(());
    }

    // DedupVerdict::New — log receipt at info! (slice demo AC1 evidence).
    match &event.kind {
        WireEventKind::TurnComplete {
            session_id,
            turn_id,
            transcript_path,
        } => {
            tracing::info!(
                session_id = %session_id,
                turn_id = %turn_id,
                transcript_path = ?transcript_path,
                kind = "turn_complete",
                "event received"
            );
            // Invoke the transcript reader (work-1.04). The path comes from
            // the hook-forwarded envelope; the per-session last_read_offset
            // comes from SessionState (held under the same lock as record()).
            let last_read_offset = {
                let guard = sessions.lock().await;
                guard.get(session_id).and_then(|s| s.last_read_offset)
            };
            match transcript_path {
                Some(path_str) if !path_str.is_empty() => {
                    let path = PathBuf::from(path_str);
                    match pulse_source::read_complete(&path, last_read_offset).await {
                        Ok((probe, read)) => {
                            // Store the reader's advanced offset back into
                            // SessionState so the next probe resumes correctly.
                            {
                                let mut guard = sessions.lock().await;
                                if let Some(state) = guard.get_mut(session_id) {
                                    state.last_read_offset = Some(read.read_offset);
                                }
                            }
                            handle_read_outcome(session_id, probe, read).await;
                        }
                        Err(e) => {
                            // NFR-7: a read failure (missing file, permission
                            // denied) degrades this turn via the same channel
                            // — write the DEGRADED marker so the silent-failure
                            // surface stays loud.
                            tracing::warn!(
                                session_id = %session_id,
                                transcript_path = %path.display(),
                                error = %e,
                                "transcript read failed; writing DEGRADED marker"
                            );
                            if let Err(marker_err) =
                                crate::degraded::mark_degraded(&format!("read failed: {e}"))
                            {
                                tracing::warn!(
                                    error = %marker_err,
                                    "failed to write DEGRADED marker for read error"
                                );
                            }
                        }
                    }
                }
                _ => {
                    // No transcript_path on the envelope (the hook could not
                    // derive one). Skip the read; nothing to narrate.
                    tracing::debug!(
                        session_id = %session_id,
                        "turn_complete with no transcript_path; skipping read"
                    );
                }
            }
        }
        WireEventKind::AttentionHint {
            session_id,
            event_id,
            raw_kind,
            transcript_path,
        } => {
            // Notification hook receipt. The full AttentionEvent
            // classification (permission-gate vs waiting-on-user) is
            // VS-1.1.3's job; here we only log receipt + carry the
            // forwarded transcript_path for the reader's awareness. No
            // transcript read on attention hints (reads happen on
            // TurnComplete).
            tracing::info!(
                session_id = %session_id,
                event_id = %event_id,
                raw_kind = %raw_kind,
                transcript_path = ?transcript_path,
                kind = "attention_hint",
                "attention event received"
            );
        }
        WireEventKind::HookDegraded { reason, session_id } => {
            // Degenerate payload from the hook (no derivable event_id). The
            // SessionManager::record path already wrote the DEGRADED marker
            // and logged at warn!; here we only log receipt at info! for
            // observability symmetry with the other arms. The marker write
            // is the loud-never-silent surface (NFR-15); this log is
            // secondary.
            tracing::info!(
                session_id = ?session_id,
                reason = %reason,
                kind = "hook_degraded",
                "degraded hook event received (DEGRADED marker written in record path)"
            );
        }
    }

    Ok(())
}

/// Apply the read-safety + probe verdicts: log + write the DEGRADED marker at
/// the two `mark_degraded` call sites (Truncated, Drift). Both surface
/// through one channel so the silent-degradation hazards are visible
/// uniformly (NFR-15).
async fn handle_read_outcome(
    session_id: &pulse_core::SessionId,
    probe: ProbeOutcome,
    read: pulse_core::source::TurnRead,
) {
    match probe {
        ProbeOutcome::Ok => match read.verdict {
            ReadVerdict::Settled | ReadVerdict::SettledAtBound => {
                tracing::debug!(
                    session_id = %session_id,
                    offset = read.read_offset,
                    verdict = ?read.verdict,
                    "turn read settled"
                );
            }
            ReadVerdict::Truncated => {
                // Read-safety hazard: the file's size regressed below
                // last_read_offset (rotation/truncation). Write the DEGRADED
                // marker so the operator sees this surface (NFR-15).
                tracing::warn!(
                    session_id = %session_id,
                    offset = read.read_offset,
                    "turn read truncated (size regression); writing DEGRADED marker"
                );
                if let Err(e) =
                    crate::degraded::mark_degraded("truncated transcript (size regression)")
                {
                    tracing::warn!(
                        error = %e,
                        "failed to write DEGRADED marker for Truncated verdict"
                    );
                }
            }
        },
        ProbeOutcome::Drift { detail } => {
            // Schema-presence probe hazard: the transcript's top-level shape
            // does not match expectations. Write the DEGRADED marker with the
            // source-neutral drift detail.
            tracing::warn!(
                session_id = %session_id,
                detail = %detail,
                "transcript drift; writing DEGRADED marker"
            );
            if let Err(e) = crate::degraded::mark_degraded(&detail) {
                tracing::warn!(
                    error = %e,
                    "failed to write DEGRADED marker for Drift verdict"
                );
            }
        }
    }
}

//! Per-connection handler: read one framed `WireEvent`, decode, forward to
//! `SessionManager`, log receipt.
//!
//! One frame per connection (hook subprocesses write one and exit — spec §3
//! async boundary 1). On any error (short frame, schema mismatch, decode
//! failure), the handler logs at `warn!` and returns `Err`; the caller drops
//! the connection. The daemon stays alive (NFR-12 / NFR-15).
//!
//! **Reader invocation (1.04 stub):** on `WireEventKind::TurnComplete`, the
//! handler invokes the transcript reader — for R2 this is
//! [`read_turn_stub`], which returns the happy-path contract values so the
//! `ProbeOutcome` / `ReadVerdict` match arms compile and are exercised. R3
//! (work-1.04) replaces the stub with a real call into `pulse-source`'s
//! reader; the match arms for `ProbeOutcome::Drift` and
//! `ReadVerdict::Truncated` then wire `crate::degraded::mark_degraded`.

use std::sync::Arc;
use std::time::Duration;

use pulse_core::source::{ProbeOutcome, ReadVerdict, TurnRead};
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
        } => {
            tracing::info!(
                session_id = %session_id,
                turn_id = %turn_id,
                kind = "turn_complete",
                "event received"
            );
            // TODO(1.04): replace read_turn_stub with the real transcript
            // reader from pulse-source. The reader probes against
            // SessionState::last_read_offset and returns (ProbeOutcome,
            // Option<TurnRead>). The match arms below are wired for R3; the
            // reader call is a stub until pulse-source (work-1.04) lands.
            let (probe, turn_read) = read_turn_stub();
            match probe {
                ProbeOutcome::Ok => {
                    if let Some(read) = turn_read {
                        // TODO(1.04): store read.read_offset in
                        // SessionState::last_read_offset so the next probe
                        // resumes correctly.
                        match read.verdict {
                            ReadVerdict::Settled | ReadVerdict::SettledAtBound => {
                                tracing::debug!(
                                    session_id = %session_id,
                                    offset = read.read_offset,
                                    "turn read settled"
                                );
                            }
                            ReadVerdict::Truncated => {
                                // TODO(1.04): crate::degraded::mark_degraded("truncated")
                                // R3 wires mark_degraded() into this arm.
                                tracing::warn!(
                                    session_id = %session_id,
                                    "turn read truncated; degraded-marker write deferred to 1.04"
                                );
                            }
                        }
                    }
                }
                ProbeOutcome::Drift { detail } => {
                    // TODO(1.04): crate::degraded::mark_degraded(&detail)
                    // R3 wires mark_degraded() into this arm.
                    tracing::warn!(
                        session_id = %session_id,
                        detail = %detail,
                        "transcript drift; degraded-marker write deferred to 1.04"
                    );
                }
            }
        }
    }

    Ok(())
}

/// Stub transcript-reader invocation.
///
/// Returns the happy-path contract values so the daemon's
/// `ProbeOutcome` / `ReadVerdict` match arms compile and are exercised against
/// the real pulse-core types without depending on `pulse-source` (which is
/// work-1.04, round 3). R3 replaces this with a real call.
///
/// The contract: `ProbeOutcome` probes the freshly-observed write against the
/// session's `last_read_offset`; on `Ok`, the `TurnRead` carries the content
/// digest, settlement verdict, and the byte offset the reader advanced to.
fn read_turn_stub() -> (ProbeOutcome, Option<TurnRead>) {
    (
        ProbeOutcome::Ok,
        Some(TurnRead {
            digest: pulse_core::TurnDigest::new("stub"),
            verdict: ReadVerdict::Settled,
            read_offset: 0,
        }),
    )
}

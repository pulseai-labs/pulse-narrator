//! # `WireEvent` envelope + length-prefixed-JSON framing
//!
//! The hook → daemon wire format. Per `03-code-patterns.md` (IPC / socket
//! protocol): message framing is **length-prefixed JSON** — a `u32`
//! little-endian byte count followed by exactly that many UTF-8 JSON body
//! bytes. Simple, zero-extra-dependency, debuggable with `nc`.
//!
//! The envelope lives here in `pulse-core` (not `pulse-source`) so the daemon
//! can decode IPC without depending on the source crate — only the transcript
//! reader does (spec §3 "Decisions baked in", decision 5).
//!
//! ## Schema versioning
//!
//! The envelope carries its own [`WireEvent::schema_version`] (`u16`). The
//! daemon uses it to reject forward-incompatible envelopes later. The only
//! "version" landed in this work item is this envelope field; the
//! schema-version probe for Claude Code JSONL belongs to `pulse-source`
//! (work-1.04 / VS-1.2.1).

use bytes::{Buf, BufMut, Bytes, BytesMut};
use serde::{Deserialize, Serialize};

use crate::event::{SessionId, TurnId};

/// The current wire-envelope schema version. Bumped only on a breaking change
/// to the [`WireEvent`] shape itself. Hook and daemon MUST agree on this.
///
/// Starts at `1` (the first versioned envelope shape).
pub const fn wire_version() -> u16 {
    1
}

/// Length-prefix byte width. `u32` little-endian per the IPC framing choice.
pub const LEN_PREFIX_BYTES: usize = 4;

/// Typed wire-framing error. Surfaced as
/// [`CoreError::Wire`](crate::error::CoreError::Wire) at the crate seam.
#[derive(Debug, thiserror::Error)]
pub enum WireError {
    /// Fewer than [`LEN_PREFIX_BYTES`] available — the frame is incomplete
    /// before the body can even be sized.
    #[error("incomplete length prefix: need {need} bytes, have {have}")]
    TruncatedPrefix { need: usize, have: usize },

    /// The length prefix advertised more body bytes than the buffer holds.
    #[error("incomplete body: length prefix says {claimed} bytes, have {have}")]
    TruncatedBody { claimed: u32, have: usize },

    /// The JSON body failed to deserialize into a [`WireEvent`].
    #[error("json decode failed: {0}")]
    Json(#[from] serde_json::Error),
}

/// Hook → daemon wire envelope.
///
/// The discriminator is [`WireEventKind`]; each variant carries the fields its
/// kind needs. `schema_version` is on the envelope (not the inner kind) so the
/// daemon can reject/forward-incompatible envelopes before dispatching on kind.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WireEvent {
    /// Envelope schema version. MUST equal [`wire_version()`] for the daemon
    /// to accept the frame; mismatches are logged and the envelope dropped
    /// (NFR-15: loud-never-silent degradation lives in the daemon, not here).
    pub schema_version: u16,
    /// Which event the hook is delivering.
    #[serde(flatten)]
    pub kind: WireEventKind,
}

impl WireEvent {
    /// Construct an envelope at the current [`wire_version()`] for the given
    /// kind. This is the canonical constructor; per-kind convenience
    /// constructors below delegate to it.
    #[must_use]
    pub fn new(kind: WireEventKind) -> Self {
        Self {
            schema_version: wire_version(),
            kind,
        }
    }

    /// The default `schema_version` every freshly-constructed envelope carries
    /// (i.e. [`wire_version()`]). Used by the round-trip test to assert
    /// default-constructed and explicitly-versioned envelopes agree.
    #[must_use]
    pub fn schema_version_default() -> u16 {
        wire_version()
    }

    /// Convenience constructor for a `TurnComplete` delivery.
    #[must_use]
    pub fn for_turn_complete(session_id: SessionId, turn_id: TurnId, schema_version: u16) -> Self {
        Self {
            schema_version,
            kind: WireEventKind::TurnComplete {
                session_id,
                turn_id,
            },
        }
    }
}

/// Discriminator for [`WireEvent`]. This is the source-neutral set of
/// hook→daemon deliveries. VS-1.1.1 emits `TurnComplete` (Stop hook),
/// `AttentionHint` (Notification hook), and `HookDegraded` (degenerate
/// payload / no stable identity fields). Additional kinds (e.g. a `Ping`
/// health-check) land with later slices — each addition bumps
/// [`wire_version()`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum WireEventKind {
    /// A complete turn was observed for `session_id`; the daemon should
    /// dispatch narration. The `turn_id` correlates with the source stream's
    /// `SourceEvent::TurnComplete`.
    TurnComplete {
        session_id: SessionId,
        turn_id: TurnId,
    },

    /// A Notification hook fired for `session_id` (e.g. permission prompt,
    /// idle). The hook forwards the raw kind string only; the full
    /// AttentionEvent classification (permission-gate vs waiting-on-user) is
    /// VS-1.1.3's job. `event_id` is the idempotency key the daemon dedups on.
    AttentionHint {
        session_id: SessionId,
        event_id: String,
        /// Raw Claude Code hook kind string, forwarded verbatim.
        raw_kind: String,
        /// Optional transcript-path forwarded for the daemon's reader (1.04).
        transcript_path: Option<String>,
    },

    /// The hook could not derive a stable `event_id` from the payload (no
    /// `message_id`, no derivable `(session_id, hook_kind, content_hash,
    /// size)` tuple). Rather than synthesizing a collision-prone key and
    /// silently dropping a sibling event, the hook forwards the reason and
    /// lets the daemon surface it via the loud `DEGRADED` channel (NFR-15).
    HookDegraded {
        /// Human-readable reason the hook could not produce a normal event.
        reason: String,
        /// Best-effort session id if one was salvageable, for daemon-side
        /// correlation; `None` if even that was underviable.
        session_id: Option<SessionId>,
    },
}

/// Encode a [`WireEvent`] into a length-prefixed-JSON byte frame.
///
/// Wire layout: `[u32 LE body_len][UTF-8 JSON body]`.
///
/// Errors only if JSON serialization fails (should not happen for
/// `Serialize`-derived envelopes constructed via [`WireEvent::new`]).
pub fn encode_wire(event: &WireEvent) -> Result<Bytes, WireError> {
    let body = serde_json::to_vec(event)?;
    let mut buf = BytesMut::with_capacity(LEN_PREFIX_BYTES + body.len());
    buf.put_u32_le(body.len() as u32);
    buf.extend_from_slice(&body);
    Ok(buf.freeze())
}

/// Decode a length-prefixed-JSON frame from `bytes`.
///
/// Returns the decoded [`WireEvent`] and the number of bytes consumed (the
/// 4-byte prefix + the body length). Callers may feed the remaining tail back
/// in for the next frame.
///
/// Returns an error if the prefix is incomplete, the body is shorter than the
/// prefix advertises, or JSON deserialization fails. A truncated frame is NOT
/// fatal to the daemon — the caller can wait for more bytes and retry.
pub fn decode_wire(bytes: &[u8]) -> Result<(WireEvent, usize), WireError> {
    if bytes.len() < LEN_PREFIX_BYTES {
        return Err(WireError::TruncatedPrefix {
            need: LEN_PREFIX_BYTES,
            have: bytes.len(),
        });
    }
    let mut prefix = &bytes[..LEN_PREFIX_BYTES];
    let body_len = prefix.get_u32_le();
    let body_end = LEN_PREFIX_BYTES + body_len as usize;
    if bytes.len() < body_end {
        return Err(WireError::TruncatedBody {
            claimed: body_len,
            have: bytes.len() - LEN_PREFIX_BYTES,
        });
    }
    let body = &bytes[LEN_PREFIX_BYTES..body_end];
    let event: WireEvent = serde_json::from_slice(body)?;
    Ok((event, body_end))
}

/// Read one length-prefixed-JSON frame off `reader`.
///
/// Async framing helper for the daemon's socket read loop. Returns the decoded
/// [`WireEvent`]; the reader is advanced past the consumed frame.
pub async fn read_frame<R>(reader: &mut R) -> Result<WireEvent, WireError>
where
    R: tokio::io::AsyncRead + Unpin,
{
    use tokio::io::AsyncReadExt;
    let mut prefix = [0u8; LEN_PREFIX_BYTES];
    reader.read_exact(&mut prefix).await.map_err(|_e| {
        // Treat an EOF mid-prefix as a truncation so the caller can distinguish
        // "orderly close with full frames" (no error) from "frame started but
        // never completed" (truncation).
        WireError::TruncatedPrefix {
            need: LEN_PREFIX_BYTES,
            have: 0,
        }
    })?;
    let body_len = u32::from_le_bytes(prefix);
    let mut body = vec![0u8; body_len as usize];
    reader
        .read_exact(&mut body)
        .await
        .map_err(|_e| WireError::TruncatedBody {
            claimed: body_len,
            have: 0,
        })?;
    let event: WireEvent = serde_json::from_slice(&body)?;
    Ok(event)
}

/// Write one length-prefixed-JSON frame to `writer`.
///
/// Async framing helper for the hook subprocess's socket write. Serializes the
/// event, writes the 4-byte LE length prefix, then the body.
pub async fn write_frame<W>(writer: &mut W, event: &WireEvent) -> Result<(), WireError>
where
    W: tokio::io::AsyncWrite + Unpin,
{
    use tokio::io::AsyncWriteExt;
    let body = serde_json::to_vec(event)?;
    writer
        .write_u32_le(body.len() as u32)
        .await
        .map_err(|_| WireError::TruncatedPrefix {
            need: LEN_PREFIX_BYTES,
            have: 0,
        })?;
    writer
        .write_all(&body)
        .await
        .map_err(|_| WireError::TruncatedBody {
            claimed: body.len() as u32,
            have: 0,
        })?;
    Ok(())
}

#[cfg(test)]
mod tests {
    //! Unit tests for the wire module live alongside the integration
    //! round-trip test in `tests/wire_roundtrip.rs`.

    use super::*;

    #[test]
    fn wire_version_is_one() {
        assert_eq!(wire_version(), 1);
    }

    #[test]
    fn encode_then_decode_roundtrip() {
        let ev =
            WireEvent::for_turn_complete(SessionId::new("s1"), TurnId::new("t1"), wire_version());
        let frame = encode_wire(&ev).expect("encode");
        let (decoded, consumed) = decode_wire(&frame).expect("decode");
        assert_eq!(decoded, ev);
        assert_eq!(consumed, frame.len());
    }
}

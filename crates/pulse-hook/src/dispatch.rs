//! Fail-fast delivery of a framed [`WireEvent`] to the daemon's Unix socket.
//!
//! This is the producer half of the hook→daemon IPC pair (the consumer is
//! `pulse-daemon`, work-1.03). The hook opens the daemon's Unix domain
//! socket, writes one length-prefixed-JSON frame, and exits.
//!
//! ## Fail-fast contract (NFR-12, NFR-14)
//!
//! The hook **never** blocks the agent beyond two small bounded timeouts.
//! Each failure mode maps to a distinct non-zero exit code so the daemon's
//! logs can interpret why a delivery didn't land:
//!
//! | Outcome | Exit |
//! |---|---|
//! | Socket file absent | 2 |
//! | Connect refused or timed out (>200 ms) | 3 |
//! | Frame write did not complete (>500 ms) | 4 |
//! | Delivered successfully | 0 |
//!
//! There is **no retry loop** in the hook. Retried/duplicate hooks are deduped
//! by the daemon (1.03), not by re-sending. The hook fires once, delivers
//! once-or-fails-fast, exits.

use std::path::Path;
use std::time::Duration;

use pulse_core::wire::{encode_wire, WireEvent};
use tokio::io::AsyncWriteExt;
use tokio::net::UnixStream;
use tokio::time::timeout;

/// Default bounded connect timeout. The hook must never block the agent for
/// long on a daemon that isn't there (slice demo AC2).
pub const DEFAULT_CONNECT_TIMEOUT: Duration = Duration::from_millis(200);

/// Default bounded write timeout. Bounds the worst case where a daemon
/// accepted the connect but isn't draining its socket buffer (e.g. wedged).
pub const DEFAULT_WRITE_TIMEOUT: Duration = Duration::from_millis(500);

/// Typed delivery outcome. Maps 1:1 to the process exit-code table documented
/// in the crate README and the binary's `--help`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeliverOutcome {
    /// Delivered the frame and the peer acknowledged the write. Exit 0.
    Delivered,
    /// The socket file does not exist at the configured path — the daemon is
    /// not running. Exit 2.
    SocketAbsent,
    /// The connect was refused or did not complete within the connect
    /// timeout. Exit 3.
    ConnectFailed,
    /// The frame write did not complete within the write timeout. Exit 4.
    WriteTimedOut,
}

impl DeliverOutcome {
    /// The process exit code for this outcome (0/2/3/4 per the table above).
    #[must_use]
    pub const fn exit_code(self) -> u8 {
        match self {
            DeliverOutcome::Delivered => 0,
            DeliverOutcome::SocketAbsent => 2,
            DeliverOutcome::ConnectFailed => 3,
            DeliverOutcome::WriteTimedOut => 4,
        }
    }
}

/// Deliver one framed [`WireEvent`] to the daemon socket at `socket_path`.
///
/// Bounded by `connect_timeout` and `write_timeout` — returns as soon as the
/// outcome is known, never blocking the agent beyond those bounds. Pure
/// (no logging side effects beyond `tracing`); the caller decides how to
/// surface the outcome to stderr / the process exit code.
///
/// This function is the typed core; [`crate::main`]/the binary wraps it with
/// logging + exit-code mapping. Integration tests call it directly to cover
/// the full 2/3/4/0 matrix.
pub async fn deliver(
    event: &WireEvent,
    socket_path: &Path,
    connect_timeout: Duration,
    write_timeout: Duration,
) -> DeliverOutcome {
    // (1) Socket file absent → exit 2. Check before connecting so the absent
    // case is distinguishable from a refused connect (which would also fail
    // the syscall but with a different errno the test matrix wants split).
    if !socket_path.exists() {
        tracing::warn!(
            socket_path = %socket_path.display(),
            "daemon not running (no socket at path)"
        );
        return DeliverOutcome::SocketAbsent;
    }

    // (2) Bounded connect. Refused OR slow → exit 3.
    let connect = timeout(connect_timeout, UnixStream::connect(socket_path));
    let mut stream = match connect.await {
        Ok(Ok(s)) => s,
        Ok(Err(_e)) => {
            tracing::warn!(socket_path = %socket_path.display(), "connect refused");
            return DeliverOutcome::ConnectFailed;
        }
        Err(_elapsed) => {
            tracing::warn!(
                socket_path = %socket_path.display(),
                ?connect_timeout,
                "connect timed out"
            );
            return DeliverOutcome::ConnectFailed;
        }
    };

    // Encode the frame once (outside the write-timeout box) so the timeout
    // measures only the socket write, not the serialization. We use
    // `encode_wire` (the synchronous framing helper) rather than the async
    // `write_frame` helper because we need the whole frame in hand to bound
    // the *complete* write, not the prefix+body separately.
    let frame = match encode_wire(event) {
        Ok(f) => f,
        Err(e) => {
            // Serialization failure on a Serialize-derived envelope is a
            // programmer error, not a delivery failure. We cannot deliver; the
            // caller treats it as a write-side failure (exit 4) rather than
            // crashing the hook.
            tracing::error!(error = %e, "failed to encode wire frame");
            return DeliverOutcome::WriteTimedOut;
        }
    };

    // (3) Bounded write of the full frame. Did not finish in time → exit 4.
    let write = timeout(write_timeout, stream.write_all(&frame));
    match write.await {
        Ok(Ok(())) => {
            // Best-effort flush; a flush failure past a successful write_all
            // is still a delivered frame from the hook's perspective — the
            // kernel buffer holds it.
            let _ = stream.flush().await;
            DeliverOutcome::Delivered
        }
        Ok(Err(_e)) => {
            tracing::warn!(socket_path = %socket_path.display(), "frame write failed");
            DeliverOutcome::WriteTimedOut
        }
        Err(_elapsed) => {
            tracing::warn!(
                socket_path = %socket_path.display(),
                ?write_timeout,
                "frame write timed out"
            );
            DeliverOutcome::WriteTimedOut
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn outcome_exit_codes_match_contract_table() {
        assert_eq!(DeliverOutcome::Delivered.exit_code(), 0);
        assert_eq!(DeliverOutcome::SocketAbsent.exit_code(), 2);
        assert_eq!(DeliverOutcome::ConnectFailed.exit_code(), 3);
        assert_eq!(DeliverOutcome::WriteTimedOut.exit_code(), 4);
    }
}

//! Integration test: the fail-fast delivery matrix.
//!
//! Covers the full exit-code table the hook contract pins (spec §3 + the
//! crate README):
//!
//! | Outcome | Exit | How this test forces it |
//! |---|---|---|
//! | Socket absent | 2 | `deliver` against a path that was never bound |
//! | Connect refused | 3 | a regular file at the path (not a socket) |
//! | Write timed out | 4 | a listener that accepts but never drains, + a |
//! |                      frame larger than the kernel send buffer |
//! | Delivered | 0 | a listener that reads one full frame |
//!
//! This is the slice demo AC2 guarantee, automated: the hook never blocks the
//! agent beyond the bounded connect (200 ms) + write (500 ms) timeouts, and
//! every failure mode is a distinct non-zero exit.

use std::path::PathBuf;
use std::time::Duration;

use pulse_core::wire::{decode_wire, WireEvent, WireEventKind};
use pulse_core::{SessionId, TurnId};
use pulse_hook::{deliver, DeliverOutcome};
use tempfile::TempDir;
use tokio::io::AsyncReadExt;
use tokio::net::UnixListener;

/// Build a small turn-complete event for the matrix.
fn small_event() -> WireEvent {
    WireEvent::new(WireEventKind::TurnComplete {
        session_id: SessionId::new("s1"),
        turn_id: TurnId::new("t1"),
    })
}

/// Unique socket path under a fresh tempdir.
fn socket_path(dir: &TempDir, name: &str) -> PathBuf {
    dir.path().join(name)
}

#[tokio::test]
async fn exit_2_when_socket_file_absent() {
    // The daemon-not-running case: the path simply does not exist. The hook
    // must detect this *before* attempting a connect so it can return exit 2
    // (distinguishable from a refused connect, which is exit 3).
    let dir = TempDir::new().expect("tempdir");
    let path = socket_path(&dir, "never-bound.sock");
    let event = small_event();

    let outcome = deliver(
        &event,
        &path,
        Duration::from_millis(200),
        Duration::from_millis(500),
    )
    .await;

    assert_eq!(
        outcome,
        DeliverOutcome::SocketAbsent,
        "absent socket file must yield SocketAbsent (exit 2)"
    );
    assert_eq!(outcome.exit_code(), 2);
}

#[tokio::test]
async fn exit_3_when_connect_refused() {
    // Path exists but is not a Unix socket: connect is refused. This is the
    // "daemon was here and crashed / stale socket / wrong path" shape.
    let dir = TempDir::new().expect("tempdir");
    let path = socket_path(&dir, "regular-file.sock");
    // Create a regular file at the path — exists() is true, but connect()
    // fails because it's not a socket.
    std::fs::write(&path, b"not a socket").expect("write file");

    let event = small_event();
    let outcome = deliver(
        &event,
        &path,
        Duration::from_millis(200),
        Duration::from_millis(500),
    )
    .await;

    assert_eq!(
        outcome,
        DeliverOutcome::ConnectFailed,
        "non-socket file at path must yield ConnectFailed (exit 3)"
    );
    assert_eq!(outcome.exit_code(), 3);
}

#[tokio::test]
async fn exit_0_when_listener_reads_one_full_frame() {
    // The happy path: a daemon is listening, accepts, reads the full frame.
    // The hook's `deliver` returns Delivered; the listener round-trips the
    // exact WireEvent via decode_wire (proving the frame is well-formed on
    // the wire, not just delivered).
    let dir = TempDir::new().expect("tempdir");
    let path = socket_path(&dir, "happy-path.sock");
    let listener = UnixListener::bind(&path).expect("bind");

    let event = small_event();
    let path_clone = path.clone();
    let server = tokio::spawn(async move {
        let _ = path_clone;
        let (mut sock, _) = listener.accept().await.expect("accept");
        // Read the whole frame: prefix + body. decode_wire needs all bytes.
        let mut buf = Vec::new();
        sock.read_to_end(&mut buf).await.expect("read_to_end");
        buf
    });

    let outcome = deliver(
        &event,
        &path,
        Duration::from_millis(200),
        Duration::from_millis(500),
    )
    .await;
    assert_eq!(outcome, DeliverOutcome::Delivered);
    assert_eq!(outcome.exit_code(), 0);

    let wire_bytes = server.await.expect("server join");
    let (decoded, consumed) = decode_wire(&wire_bytes).expect("decode wire frame");
    assert_eq!(consumed, wire_bytes.len(), "no trailing bytes");
    assert_eq!(
        decoded, event,
        "round-tripped event must match what we sent"
    );
}

#[tokio::test]
async fn exit_4_when_write_does_not_complete_in_time() {
    // Force a write timeout deterministically: a listener that accepts but
    // never reads, plus a frame *larger* than the default kernel send buffer
    // (so write_all cannot complete until the peer drains), plus a tight
    // write timeout. The hook must return WriteTimedOut (exit 4) rather than
    // blocking.
    let dir = TempDir::new().expect("tempdir");
    let path = socket_path(&dir, "write-timeout.sock");
    let listener = UnixListener::bind(&path).expect("bind");

    // Build a large event: a HookDegraded carrying a >256 KB reason string.
    // The default macOS Unix-socket send buffer is ~256 KB; a frame this
    // size cannot be fully accepted by write_all without a draining reader.
    let big_reason = "x".repeat(300_000);
    let event = WireEvent::new(WireEventKind::HookDegraded {
        reason: big_reason,
        session_id: Some(SessionId::new("s1")),
    });

    // Server: accept the connection and then *never read* — hold it open so
    // the send buffer fills and stays full. The task holds the stream until
    // the test ends.
    let staller = tokio::spawn(async move {
        let (sock, _addr) = listener.accept().await.expect("accept");
        // Sleep forever (until the runtime drops this task on test exit).
        // The socket is intentionally never read.
        tokio::time::sleep(Duration::from_secs(60)).await;
        drop(sock);
    });

    // Tiny write timeout so the test stays fast (the spec default is 500 ms;
    // we tighten to 50 ms here to keep the matrix snappy while still proving
    // the bound fires).
    let outcome = deliver(
        &event,
        &path,
        Duration::from_millis(200),
        Duration::from_millis(50),
    )
    .await;

    // The stall task may or may not have completed its connect-handshake by
    // the time we finish — cancel it either way.
    staller.abort();

    assert_eq!(
        outcome,
        DeliverOutcome::WriteTimedOut,
        "write that cannot drain within the bound must yield WriteTimedOut (exit 4); got {outcome:?}"
    );
    assert_eq!(outcome.exit_code(), 4);
}

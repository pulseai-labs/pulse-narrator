//! Socket-accept integration test.
//!
//! A synthetic peer (the test) writes a framed `WireEvent::TurnComplete` to a
//! temp Unix socket; the daemon's `handle_connection` reads it, dedups via
//! `SessionManager`, and logs receipt at `info!`. This is the slice demo AC1
//! evidence path (daemon receives and logs) exercised end-to-end against the
//! real `pulse_core::wire::write_frame` producer.
//!
//! We do NOT spawn the daemon binary (1.02's hook is not merged yet, and
//! spawning + signal teardown is flaky in CI). We exercise
//! `handle_connection` directly against a pair of connected `UnixStream`
//! endpoints — that is the function the accept loop calls per connection.

use std::sync::Arc;

use pulse_core::wire::{write_frame, WireEventKind};
use pulse_core::{SessionId, TurnId, WireEvent};
use pulse_daemon::{handle_connection, SessionManager};
use tokio::net::UnixStream;
use tokio::sync::Mutex;

async fn connected_pair(label: &str) -> std::io::Result<(UnixStream, UnixStream)> {
    // Unique per-test socket path under the temp dir. Combines process id,
    // a per-call counter, and the test-supplied label so parallel tests do
    // not collide on the socket file.
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let mut dir = std::env::temp_dir();
    dir.push(format!(
        "pulse-daemon-test-{}-{}-{}",
        std::process::id(),
        n,
        label
    ));
    std::fs::create_dir_all(&dir)?;
    let sock_path = dir.join("daemon.sock");

    // Tokio-native listener + accept so we never promote a blocking socket
    // into the runtime (tokio forbids UnixStream::from_std from a blocking
    // listener).
    let listener = tokio::net::UnixListener::bind(&sock_path)?;
    let client = tokio::net::UnixStream::connect(&sock_path).await?;
    let (server, _peer_addr) = listener.accept().await?;
    drop(listener);
    let _ = std::fs::remove_file(&sock_path);
    let _ = std::fs::remove_dir(&dir);
    Ok((client, server))
}

#[tokio::test]
async fn synthetic_peer_write_is_accepted_and_recorded_as_new() {
    let (mut client, server) = connected_pair("new").await.expect("connected pair");

    let event = WireEvent::new(WireEventKind::TurnComplete {
        session_id: SessionId::new("session-7"),
        turn_id: TurnId::new("turn-42"),
        transcript_path: None,
    });

    // Producer side: write one framed WireEvent (this is what 1.02's hook will
    // do once merged).
    let write_task = tokio::spawn(async move {
        write_frame(&mut client, &event).await.expect("write_frame");
        // Closing the write side signals EOF to the server read; for a
        // one-frame connection this is the natural lifecycle.
    });

    let sessions = Arc::new(Mutex::new(SessionManager::new()));
    let result = handle_connection(server, Arc::clone(&sessions)).await;
    write_task.await.expect("write task join");

    assert!(
        result.is_ok(),
        "handle_connection should succeed: {:?}",
        result
    );

    // The event was forwarded to the SessionManager and recorded as New.
    let guard = sessions.lock().await;
    let state = guard
        .get(&SessionId::new("session-7"))
        .expect("session tracked after event");
    assert_eq!(state.last_event_id.as_deref(), Some("turn-42"));
    assert_eq!(state.receipt_seq, 1);
}

#[tokio::test]
async fn short_frame_is_dropped_without_forwarding() {
    // A peer that writes a length prefix announcing N bytes but sends K<N
    // before closing. Per spec §3 short-frame discipline: the connection is
    // dropped WITHOUT forwarding to SessionManager (no partial/garbage
    // WireEvent reaches the session/dedup layer).
    let (mut client, server) = connected_pair("short").await.expect("connected pair");

    use tokio::io::AsyncWriteExt;
    let write_task = tokio::spawn(async move {
        // Announce 1_000_000 bytes...
        let len = 1_000_000u32.to_le_bytes();
        client.write_all(&len).await.expect("write prefix");
        // ...but send only 4 bytes of "body" and close.
        client.write_all(b"garb").await.expect("write partial");
        client.shutdown().await.ok();
    });

    let sessions = Arc::new(Mutex::new(SessionManager::new()));
    let result = handle_connection(server, Arc::clone(&sessions)).await;
    write_task.await.expect("write task join");

    // The handler MUST error (ShortFrame), and the SessionManager MUST be
    // empty (nothing was forwarded).
    assert!(result.is_err(), "short frame must error");
    let err = result.expect_err("short frame must error");
    assert!(
        err.is_short_frame(),
        "error must be a ShortFrame variant, got: {err:?}"
    );
    let guard = sessions.lock().await;
    assert!(
        guard.is_empty(),
        "SessionManager must be empty after a short frame (no partial event forwarded)"
    );
}

#[tokio::test]
async fn duplicate_event_is_acknowledged_not_recorded_as_new() {
    // Two connections delivering the same event id for the same session: first
    // is New, second is Duplicate. Verifies the dedup path through the socket
    // handler (not just the pure unit test in session_dedup.rs).
    let sessions = Arc::new(Mutex::new(SessionManager::new()));

    for idx in 0..2 {
        let (mut client, server) = connected_pair(&format!("dup-{idx}"))
            .await
            .expect("connected pair");
        let event = WireEvent::new(WireEventKind::TurnComplete {
            session_id: SessionId::new("s-dup"),
            turn_id: TurnId::new("t-dup"),
            transcript_path: None,
        });
        let write_task = tokio::spawn(async move {
            write_frame(&mut client, &event).await.expect("write_frame");
        });
        let result = handle_connection(server, Arc::clone(&sessions)).await;
        write_task.await.expect("write task join");
        assert!(
            result.is_ok(),
            "iteration {idx} should not error: {result:?}"
        );
    }

    let guard = sessions.lock().await;
    let state = guard
        .get(&SessionId::new("s-dup"))
        .expect("session tracked");
    // Only the first delivery advanced the receipt_seq; the duplicate did not.
    assert_eq!(
        state.receipt_seq, 1,
        "duplicate must not advance receipt_seq"
    );
}

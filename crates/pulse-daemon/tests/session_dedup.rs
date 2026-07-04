//! Idempotent-dedup integration test for `SessionManager`.
//!
//! Spec §3 "Idempotent dedup is by event_id": an incoming `WireEvent` whose
//! event id matches the last-seen for its session is acknowledged (logged at
//! `debug!`) and NOT re-processed. Dedup is per-session, not global — two
//! different sessions can each see the same nominal event id without
//! collision.
//!
//! These tests hit `SessionManager::record` directly (the dedup logic is a
//! pure function of the manager state; no socket involved). The socket
//! integration is covered by `tests/socket_accept.rs`.

use pulse_core::wire::WireEventKind;
use pulse_core::{SessionId, TurnId, WireEvent};
use pulse_daemon::{DedupVerdict, SessionManager};

fn turn(session: &str, turn: &str) -> WireEvent {
    WireEvent::new(WireEventKind::TurnComplete {
        session_id: SessionId::new(session),
        turn_id: TurnId::new(turn),
    })
}

#[test]
fn same_event_id_twice_yields_new_then_duplicate() {
    let mut mgr = SessionManager::new();
    // First delivery of event id "t1" for session "s1".
    let v1 = mgr.record(&turn("s1", "t1"));
    assert_eq!(v1, DedupVerdict::New, "first delivery is New");
    // Retry / duplicate: same session + same event id.
    let v2 = mgr.record(&turn("s1", "t1"));
    assert_eq!(v2, DedupVerdict::Duplicate, "retry is Duplicate");
    // A second retry is still Duplicate.
    let v3 = mgr.record(&turn("s1", "t1"));
    assert_eq!(v3, DedupVerdict::Duplicate);
    // A genuinely new turn id in the same session is New again.
    let v4 = mgr.record(&turn("s1", "t2"));
    assert_eq!(v4, DedupVerdict::New);
}

#[test]
fn two_sessions_same_event_id_both_new() {
    // Dedup is per-session, NOT global. Two different sessions can each see
    // the same nominal event id ("t1") without colliding (spec §3).
    let mut mgr = SessionManager::new();
    let v1 = mgr.record(&turn("s1", "t1"));
    let v2 = mgr.record(&turn("s2", "t1"));
    assert_eq!(v1, DedupVerdict::New);
    assert_eq!(v2, DedupVerdict::New);
    // And a retry within s1 is still a Duplicate for s1 only.
    let v3 = mgr.record(&turn("s1", "t1"));
    assert_eq!(v3, DedupVerdict::Duplicate);
    // s2's retry is a Duplicate for s2.
    let v4 = mgr.record(&turn("s2", "t1"));
    assert_eq!(v4, DedupVerdict::Duplicate);
}

#[test]
fn receipt_seq_advances_only_on_new() {
    let mut mgr = SessionManager::new();
    mgr.record(&turn("s", "t1"));
    mgr.record(&turn("s", "t1")); // dup
    mgr.record(&turn("s", "t1")); // dup
    mgr.record(&turn("s", "t2"));
    mgr.record(&turn("s", "t3"));
    let state = mgr.get(&SessionId::new("s")).expect("session tracked");
    // Three NEW events (t1, t2, t3); the two dups do not advance the seq.
    assert_eq!(state.receipt_seq, 3);
    assert_eq!(state.last_event_id.as_deref(), Some("t3"));
}

#[test]
fn distinct_sessions_tracked_independently() {
    let mut mgr = SessionManager::new();
    mgr.record(&turn("alpha", "t1"));
    mgr.record(&turn("beta", "t9"));
    mgr.record(&turn("alpha", "t1")); // dup for alpha
    assert_eq!(mgr.len(), 2, "two distinct sessions tracked");
    let alpha = mgr.get(&SessionId::new("alpha")).expect("alpha tracked");
    let beta = mgr.get(&SessionId::new("beta")).expect("beta tracked");
    assert_eq!(alpha.receipt_seq, 1, "alpha: one new event, one dup");
    assert_eq!(beta.receipt_seq, 1, "beta: one new event");
    assert_eq!(alpha.last_event_id.as_deref(), Some("t1"));
    assert_eq!(beta.last_event_id.as_deref(), Some("t9"));
}

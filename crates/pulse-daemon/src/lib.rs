//! # pulse-daemon
//!
//! The PulseVoice resident daemon. Binds the Unix domain socket, accepts
//! framed [`WireEvent`](pulse_core::WireEvent)s from hook subprocesses
//! (work-1.02) and the transcript reader (work-1.04), correlates them by
//! [`SessionId`](pulse_core::SessionId), dedups idempotently by event id, and
//! logs receipt at `info!`.
//!
//! **Two of VS-1.1.1's integration hazards are owned here:**
//! - **Session-identity correlation** across terminal restarts (a Claude Code
//!   session resumed in a new terminal maps to the same `SessionId`) — via
//!   [`session::SessionManager`] keyed on `SessionId`.
//! - **Idempotent dedup** of duplicate/retried hooks (Claude Code retries
//!   hooks; the daemon speaks each logical event once) — via
//!   [`session::DedupVerdict`].
//!
//! ## No-panic discipline (NFR-12 / NFR-15, load-bearing)
//!
//! A malformed frame, a partial read, or a poisoned session degrades that one
//! connection via `?` + typed [`error::DaemonError`] — the daemon stays alive.
//! Enforced at compile time via `#![deny(clippy::panic)]` (in `main.rs`).
//!
//! ## R2 / R3 boundary
//!
//! The transcript-reader invocation is a stub ([`connection::read_turn_stub`])
//! for R2: it returns the happy-path contract values so the
//! `ProbeOutcome::Drift` / `ReadVerdict::Truncated` match arms compile and are
//! exercised against the real pulse-core types WITHOUT taking a
//! `pulse-source` dependency (which is work-1.04, round 3). R3 replaces the
//! stub with a real call.

pub mod connection;
pub mod degraded;
pub mod error;
pub mod listener;
pub mod session;
pub mod shutdown;

pub use connection::handle_connection;
pub use degraded::{clear_degraded, mark_degraded};
pub use error::DaemonError;
pub use listener::bind_socket;
pub use session::{DedupVerdict, SessionManager, SessionState};
pub use shutdown::wait_for_signal;

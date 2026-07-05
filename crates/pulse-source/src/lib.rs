//! # pulse-source
//!
//! The PulseVoice source adapter for Claude Code. This crate is the **only**
//! crate in the workspace that knows about Claude Code's JSONL transcript
//! shape (MASTER-SPEC §Phase 7.1, `02-system-patterns.md` boundary rule).
//! Downstream crates read only the surface-neutral
//! [`SourceEvent`](pulse_core::SourceEvent) stream this adapter emits.
//!
//! ## What 1.04 ships
//!
//! VS-1.1.1's fourth integration hazard is *reading only complete transcript
//! writes*. This crate owns that hazard end-to-end:
//!
//! - **Read-safety** ([`reader`]): adaptive settle window (size-scaled,
//!   3-consecutive-stable-polls, bursty-appender-aware) for end-of-write
//!   detection; size-regression truncation detection; bounded total wait
//!   (NFR-11: never blocks the daemon indefinitely).
//! - **Schema-presence probe** ([`probe`]): lightweight top-level-shape check
//!   producing [`ProbeOutcome`](pulse_core::source::ProbeOutcome). The full
//!   schema probe is VS-1.2.1+.
//! - **`ClaudeCodeAdapter`** ([`adapter`]): the first concrete
//!   [`SourceAdapter`](pulse_core::adapter::SourceAdapter) impl; for VS-1.1.1
//!   it emits only `SourceEvent::TurnComplete` after a successful read.
//!
//! ## No-panic discipline (NFR-7 / NFR-12)
//!
//! A missing file, a permission error, a partial line, or a malformed JSON
//! object degrades that read via [`SourceError`](error::SourceError) — the
//! daemon (caller) stays alive. Enforced at compile time via
//! `#![deny(clippy::panic)]`; no `unwrap` / `expect` / `panic!` outside
//! `#[cfg(test)]`.

// NFR-7: a malformed transcript, a probe drift, or a partial read MUST
// degrade the affected turn/session and return an `Err`. The daemon stays
// alive. Library code may not panic — enforced via deny(clippy::panic).
#![deny(clippy::panic)]
// Lint bar matches the workspace convention in 03-code-patterns.md:
// "cargo clippy -D warnings is the hard bar — zero warnings are permitted."
#![warn(clippy::all)]

pub mod adapter;
pub mod error;
pub mod probe;
pub mod reader;

pub use adapter::ClaudeCodeAdapter;
pub use error::SourceError;
pub use reader::{read_complete, DEFAULT_SETTLE_BOUND};

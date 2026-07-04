//! # pulse-core
//!
//! PulseVoice domain types and trait seams. This crate is the **dependency
//! root** of the workspace (MASTER-SPEC §Phase 7.1): every other pulse-* crate
//! may depend on it; it depends on no other workspace crate.
//!
//! ## Public surface
//!
//! - **Domain vocabulary** ([`event`]): `Segment`, `Turn`, `AttentionEvent`,
//!   `Role`, `Kind`, `ChunkDecision`, `Utterance`, `SessionId`, and the
//!   surface-agnostic `SourceEvent` enum (`Segment` / `AttentionEvent` /
//!   `TurnComplete`).
//! - **Source-neutral contract types** ([`source`]): `ProbeOutcome`,
//!   `ReadVerdict`, `TurnRead`, `TurnDigest` — the vocabulary shared across the
//!   daemon ↔ source seam, deliberately placed here (not in `pulse-source`)
//!   so both crates compile against them independently.
//! - **Trait seams** ([`adapter`], [`tts`]): the pinned `SourceAdapter` and
//!   `TtsProvider` trait definitions. Their shapes are pinned per
//!   MASTER-SPEC §Phase 5.1 — see each module's docs for the pin rationale.
//! - **IPC envelope** ([`wire`]): the versioned `WireEvent` envelope and the
//!   length-prefixed-JSON framing helpers used by the hook→daemon socket.
//! - **Typed error** ([`error`]): `CoreError`, the only error type this crate
//!   raises at a public seam.
//!
//! ## No-panic discipline
//!
//! Library code never `panic!`s or uses `unwrap()`/`expect()`. This is enforced
//! at compile time via `#![deny(clippy::panic)]` and the workspace lint bar
//! (`clippy::all` at `-D warnings`). A malformed input degrades to a typed
//! `Err` rather than aborting the daemon (NFR-12).

// NFR-12: a malformed transcript, a probe drift, or a poisoned segment MUST
// degrade the affected segment/session and return an `Err`. The daemon stays
// alive. Library code may not panic — enforced via deny(clippy::panic).
#![deny(clippy::panic)]
// Lint bar matches the workspace convention in 03-code-patterns.md:
// "cargo clippy -D warnings is the hard bar — zero warnings are permitted."
#![warn(clippy::all)]

pub mod adapter;
pub mod error;
pub mod event;
pub mod source;
pub mod tts;
pub mod wire;

pub use event::{
    AttentionEvent, AttentionKind, ChunkDecision, Kind, Role, Segment, SessionId, SourceEvent,
    Turn, TurnId, Utterance,
};
pub use source::{ProbeOutcome, ReadVerdict, TurnDigest, TurnRead};
pub use wire::{wire_version, WireEvent};

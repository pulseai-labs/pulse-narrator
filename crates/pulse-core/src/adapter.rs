//! # `SourceAdapter` — the input-source trait seam (PINNED)
//!
//! The shape of this trait is **pinned** per MASTER-SPEC §Phase 5.1 /
//! `03-code-patterns.md`: it must not change between v1 and Phase 2, because
//! adding a future poll/scrape-based adapter (Codex CLI, desktop) must conform
//! without reshaping anything downstream.
//!
//! ## Why the concrete `Stream` impl is `tokio_stream::wrappers::ReceiverStream`
//!
//! The spec's wording ("`async fn events(&self) -> impl Stream<Item =
//! SourceEvent>`, or equivalently an `mpsc` receiver") permits an
//! `mpsc::Receiver<SourceEvent>` as the concrete shape. We expose it as
//! `tokio_stream::wrappers::ReceiverStream<SourceEvent>`, the standard
//! `futures::Stream` adapter around `tokio::sync::mpsc::Receiver`, so the
//! daemon owns the sender side and adapters produce by forwarding into it.
//! This keeps the literal pinned signature (`async fn events(...) -> impl
//! Stream<Item = SourceEvent>`) intact while letting an mpsc receiver be the
//! concrete stream.
//!
//! ## Not validated in this slice
//!
//! Per work-1.01 §"Not in this work item", the pinned shape is
//! **staged-for-validation, not validated**. VS-1.1.1's only impl (work-1.04)
//! emits `TurnComplete` only; the `Segment` and `AttentionEvent` arms are
//! never exercised here. VS-1.2.1's first task validates the shape against a
//! real JSONL→`Segment` flow. **Do NOT add a toy Segment-emitting impl here —
//! defer that to VS-1.2.1.**

use tokio_stream::wrappers::ReceiverStream;

use crate::event::SourceEvent;

/// Concrete `Stream` type the adapter yields. A
/// `tokio_stream::wrappers::ReceiverStream<SourceEvent>` wrapping the
/// adapter-owned `tokio::sync::mpsc::Receiver` — see module docs for rationale.
pub type SourceStream = ReceiverStream<SourceEvent>;

/// Input-source trait seam. Implementors produce a stream of surface-neutral
/// [`SourceEvent`]s for the daemon to consume.
///
/// # Stability contract
///
/// This trait's signature is the workspace's pinned input seam. Breaking it
/// requires a workspace-wide atomic reshape touching `pulse-core`,
/// `pulse-source`, `pulse-pipeline`, and `pulse-daemon` in one commit
/// (`03-code-patterns.md` "Versioning").
pub trait SourceAdapter {
    /// Return the live event stream for this source.
    ///
    /// Each call yields a fresh `impl Stream<Item = SourceEvent>`; the adapter
    /// owns the corresponding sender internally. The stream MUST eventually
    /// terminate (close the channel) when the source is exhausted so the
    /// daemon's consumer loop ends cleanly.
    // `async fn` in a trait is the pinned shape per MASTER-SPEC §Phase 5.1 /
    // 03-code-patterns.md. The auto-trait-bounds caveat (no explicit `Send`
    // on the returned future) is acceptable here: the daemon is the only
    // consumer, and we deliberately do not constrain the future's auto traits
    // at this seam — adding `Send` would be a one-way API door. Suppressed
    // explicitly per the code-style rule "disable a lint only with a
    // #[allow(...)] that carries a comment explaining why."
    #[allow(async_fn_in_trait)]
    async fn events(&self) -> SourceStream;
}

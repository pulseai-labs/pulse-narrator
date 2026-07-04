//! # `TtsProvider` — the TTS-engine trait seam (PINNED)
//!
//! Like [`SourceAdapter`](crate::adapter::SourceAdapter), this trait's shape is
//! pinned per MASTER-SPEC §Phase 5.1 / `03-code-patterns.md`: cloud TTS
//! providers plug in by implementing this trait, not by touching
//! `pulse-pipeline`. So the signature here must not change without a
//! workspace-wide atomic reshape.
//!
//! ## Declaration only — no impl this slice
//!
//! Per work-1.01 spec, the Kokoro provider lands in VS-1.1.2. This module
//! declares the trait + its argument/return types so VS-1.1.1's pipeline
//! crates have a stable target; no `impl TtsProvider` ships here.

use serde::{Deserialize, Serialize};

use crate::event::Utterance;

/// PCM (or other raw) audio produced by a [`TtsProvider`]. Opaque byte bag the
/// playback queue hands to the audio sink. Encoding/shape details are a
/// runtime concern; here it is just owned bytes + a sample-rate tag so the
/// sink can resample if needed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AudioClip {
    /// Raw sample bytes (provider-defined encoding; sink is expected to know
    /// how the active provider frames its output).
    pub samples: Vec<u8>,
    /// Sample rate in Hz the provider produced `samples` at.
    pub sample_rate_hz: u32,
}

/// Typed error a [`TtsProvider`] raises at its public seam. Provider-specific
/// detail stays inside the implementing crate (`pulse-tts`); only a neutralized
/// reason crosses the seam (NFR-15: typed `Error` enums at every crate seam).
#[derive(Debug, thiserror::Error)]
pub enum TtsError {
    /// The provider could not synthesize within its latency budget and the
    /// daemon should fall back to a pre-synthesized phrase (NFR-4).
    #[error("synthesis failed: {reason}")]
    SynthesisFailed { reason: String },
    /// The provider was not in a usable state (model not loaded, etc.).
    #[error("provider unavailable: {reason}")]
    Unavailable { reason: String },
}

/// TTS-engine trait seam. Implementors synthesize an [`Utterance`] into an
/// [`AudioClip`] for the playback queue.
///
/// # Stability contract
///
/// This signature is the workspace's pinned output seam. Breaking it requires
/// a workspace-wide atomic reshape touching `pulse-core`, `pulse-tts`, and
/// `pulse-pipeline` in one commit (`03-code-patterns.md` "Versioning").
pub trait TtsProvider {
    /// Synthesize `utterance` into a playable [`AudioClip`].
    ///
    /// Errors are typed [`TtsError`]s; the daemon degrades the affected
    /// utterance rather than panicking (NFR-12: daemon stays alive).
    // `async fn` in a trait is the pinned shape per MASTER-SPEC §Phase 5.1 /
    // 03-code-patterns.md. Same auto-trait-bounds rationale as
    // `SourceAdapter::events`: the daemon is the only consumer, and we do not
    // constrain the future's auto traits at this seam. Suppressed explicitly.
    #[allow(async_fn_in_trait)]
    async fn synthesize(&self, utterance: &Utterance) -> Result<AudioClip, TtsError>;
}

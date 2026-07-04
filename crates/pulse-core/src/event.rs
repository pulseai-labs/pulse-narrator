//! Surface-agnostic domain vocabulary for the PulseVoice pipeline.
//!
//! These types are deliberately **source-neutral** (MASTER-SPEC §Phase 5.1,
//! `02-system-patterns.md` "boundary rule"). Only `pulse-source` knows about
//! Claude Code's JSONL shape; everything downstream reads only the neutral
//! `Segment`/`AttentionEvent`/`TurnComplete` stream emitted by the
//! [`SourceAdapter`](crate::adapter::SourceAdapter) trait.
//!
//! All public types implement `Debug`, `Clone`, `PartialEq`, `Eq` and
//! `serde::{Serialize, Deserialize}` so they can travel over IPC and through
//! fixture files (`03-code-patterns.md`).

use serde::{Deserialize, Serialize};

/// Identifier of a single agent session. Keyed by the session id carried in the
/// hook event; the daemon's `SessionManager` holds a `HashMap<SessionId, _>`
/// (BACKLOG-19 / FR-24 — `SessionManager` itself lands in work-1.03).
///
/// Carried here (not in `pulse-source`) so the daemon can key its session map
/// without depending on the source crate.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SessionId(String);

impl SessionId {
    /// Construct a `SessionId` from an opaque hook-supplied string.
    #[must_use]
    pub fn new(id: impl Into<String>) -> Self {
        Self(id.into())
    }

    /// Borrow the underlying id string.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl From<SessionId> for String {
    fn from(id: SessionId) -> Self {
        id.0
    }
}

impl std::fmt::Display for SessionId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// Monotonically-increasing turn id within a session. Emitted in
/// `SourceEvent::TurnComplete` to signal the v1 narration trigger boundary
/// (`02-system-patterns.md` async boundary 5: narration fires on
/// `TurnComplete`, not on individual tokens).
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct TurnId(String);

impl TurnId {
    #[must_use]
    pub fn new(id: impl Into<String>) -> Self {
        Self(id.into())
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for TurnId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// Speaker role for a [`Segment`] / [`Utterance`]. Mirrors the agent-vs-user
/// axis downstream chunking keys on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    /// The agent (assistant) produced this content.
    Assistant,
    /// The operator (user) produced this content.
    User,
    /// A system message (tool result, environment, etc.).
    System,
}

/// Structural kind of a [`Segment`]. Drives the chunker's role × size tiering
/// and the code-to-speakable normalization pass (both land in `pulse-pipeline`
/// — here we only carry the label).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    /// Prose the agent wrote (reasoning, narration, answers).
    Text,
    /// Source-code block.
    Code,
    /// Shell command the agent proposed to run.
    Command,
    /// Captured stdout/stderr from a tool invocation.
    ToolOutput,
    /// Unified diff / patch.
    Patch,
    /// Plan / checklist.
    Plan,
    /// Error message (test failure, panic, non-zero exit).
    Error,
    /// Permission prompt the agent is blocking on.
    PermissionPrompt,
}

/// A neutral content segment flowing through the pipeline. This is the
/// surface-agnostic metadata bag downstream crates read (`02-system-patterns.md`
/// "boundary rule (compile-time enforced)").
///
/// `text` carries the raw segment content; downstream normalization produces
/// the speakable form. Optional typed fields carry structure the chunker keys
/// on without exposing any agent-specific schema.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Segment {
    /// Speaker role for this segment.
    pub role: Role,
    /// Structural kind of this segment.
    pub kind: Kind,
    /// Raw segment content. Source-neutral — never contains agent-schema JSON.
    pub text: String,
}

impl Default for Segment {
    fn default() -> Self {
        Self {
            role: Role::Assistant,
            kind: Kind::Text,
            text: String::new(),
        }
    }
}

impl Segment {
    #[must_use]
    pub fn new(role: Role, kind: Kind, text: impl Into<String>) -> Self {
        Self {
            role,
            kind,
            text: text.into(),
        }
    }
}

/// Out-of-band attention signal. Does NOT enter the normal segment queue — the
/// `PlaybackQueue` preempts everything on receipt (`02-system-patterns.md`
/// async boundary 4). Carries a [`AttentionKind`] so the daemon can pick a
/// pre-synthesized fallback phrase when Kokoro cannot meet the <1s latency
/// target (NFR-4).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AttentionEvent {
    pub kind: AttentionKind,
}

impl Default for AttentionEvent {
    fn default() -> Self {
        Self {
            kind: AttentionKind::PermissionPrompt,
        }
    }
}

/// What kind of attention an [`AttentionEvent`] signals. The literal spoken
/// phrases for each kind live in `pulse-daemon` (the fallback-phrase table is
/// a runtime concern); the discriminator lives here so it can travel over IPC.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AttentionKind {
    /// Agent is blocked waiting for operator permission.
    PermissionPrompt,
    /// Agent finished its turn and is awaiting the next instruction.
    Idle,
}

/// A single completed turn, identified by its [`TurnId`]. Referred to by the
/// `SourceEvent::TurnComplete` arm to fire the v1 narration trigger.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Turn {
    pub id: TurnId,
}

/// The surface-agnostic event vocabulary emitted by a [`SourceAdapter`].
///
/// **Pinned shape** (MASTER-SPEC §Phase 5.1 / `03-code-patterns.md`): adding a
/// future poll/scrape-based adapter (Codex CLI, desktop) must conform without
/// reshaping anything downstream. The three variants are the complete
/// vocabulary downstream ever sees.
///
/// # Note on the `Segment` arm
///
/// Per work-1.01 §"Not in this work item", the `Segment` and `AttentionEvent`
/// arms are **not exercised** by anything in VS-1.1.1 — VS-1.1.1's only
/// `SourceAdapter` impl emits `TurnComplete` only. The arms are declared here
/// on faith; VS-1.2.1's first task validates that the pinned shape fits a real
/// JSONL→`Segment` flow.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SourceEvent {
    Segment(Segment),
    AttentionEvent(AttentionEvent),
    TurnComplete(TurnId),
}

/// A chunking decision produced by the pipeline chunker. Lives in `pulse-core`
/// (not `pulse-pipeline`) because it travels over IPC as part of the daemon's
/// per-segment bookkeeping and is referenced by the source-neutral
/// [`TurnRead`](crate::source::TurnRead) digest. The chunker's *logic* lives
/// in `pulse-pipeline`; the *vocabulary* lives here.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChunkDecision {
    /// Tier the chunker assigned this segment.
    pub tier: ChunkTier,
    /// Speakable text the normalizer produced (already code-normalized).
    pub speakable: String,
}

/// Chunking tier. The chunker assigns one of these per segment.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChunkTier {
    /// Read aloud in full.
    Read,
    /// Announced with a short label.
    Announce,
    /// Announced + "pull" cue (e.g. permission prompt).
    AnnouncePull,
    /// Suppressed entirely.
    Suppress,
}

/// A unit of speech the [`TtsProvider`](crate::tts::TtsProvider) synthesizes.
/// Produced by the chunker/normalizer; the provider's job is to turn it into
/// an [`AudioClip`](crate::tts::AudioClip).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Utterance {
    pub text: String,
}

impl Utterance {
    #[must_use]
    pub fn new(text: impl Into<String>) -> Self {
        Self { text: text.into() }
    }
}

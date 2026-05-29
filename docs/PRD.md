# PulseVoice — Product Requirements Document

> Derived from [MASTER-SPEC.md](../../pulse-narrator-ai/MASTER-SPEC.md) and the project Executive Summary.
> Use-case IDs minted here (`UC-*`) are the stable contract consumed downstream by the SRS and BACKLOG.

**Document version:** 1.0
**Date:** 2026-05-29
**Project class:** Other (personal developer tool — local-first macOS daemon)

---

## Vision

PulseVoice turns the structured output of CLI coding agents into intelligent, content-aware narration so a developer can run long agent sessions eyes-free — hearing prose read, operational edits announced, and inline code spoken in a human-speakable form, while verbatim code is never droned out. It is the output-side complement to voice input: instead of forcing the developer to triage walls of mixed prose-and-code for the load-bearing 10%, it lets them step away during a long run and trust being called back the moment the agent is blocked on a permission or input prompt.

## Problem

Developers running long agentic coding sessions face two intertwined frictions: triaging dense, mixed prose-and-code agent output to find the few sentences that actually matter, and the dead time while a long agent task grinds away. Reading every completed `Turn` on screen is wasteful — most output is `operational-code`, `structure`, or `noise` that does not need to be read aloud verbatim, while the `prose` and any `AttentionEvent` (a permission gate or a waiting-on-user signal) are what the operator must not miss. Today there is no faithful way to consume that stream by ear: naive text-to-speech reads code character by character and offers no priority path, so an operator who steps away can be silently stuck — hearing low-value chatter from one session while another session sits blocked, waiting for input. PulseVoice resolves this by tapping the agent's structured transcript, classifying each `Segment` by role and size into a `ChunkDecision` (read / announce / announce+pull / suppress), normalizing code into speakable `Utterance`s, and guaranteeing that attention events always speak and preempt across every session.

## Users

PulseVoice is a single-operator tool; both personas are the same individual in different modes of work, and they share one machine with no multi-user model.

| Persona | Description | Primary goal | Key pain point resolved |
| --- | --- | --- | --- |
| **P1 — Solo developer / homelab tinkerer** (primary) | Privacy-conscious developer running CLI coding agents for long tasks; local-first / Ollama-homelab ethos; frequently multitasking across several terminals or worktrees. | Step away during a long agent run and have each completed turn narrated content-aware, never missing a permission or attention prompt. | Cannot leave the screen during long runs without risking a silently-blocked session; cloud TTS would exfiltrate private source code, paths, and incidental secrets. |
| **P2 — Agentic-dev power user** | Already uses voice input (Wispr Flow) and wants to close the loop to a fully eyes-and-hands-free workflow. | Drive and consume agent sessions hands-and-eyes-free, with audio carrying the prose while eyes (when used) go to the code. | No output-side voice companion exists to pair with voice input; existing TTS is not content-aware and reads code verbatim. |

## Scope

### In scope (MVP / v1)

- **Single source — Claude Code only.** Ingest completed turns via Claude Code Stop/Notification hooks, delivering events to the resident daemon over a Unix domain socket; parse the JSONL transcript with a schema-version probe.
- **Content-aware chunker (the crown jewel).** Role × size tiering with a duration-based threshold producing per-`Segment` `ChunkDecision`s: read prose, announce operational edits, announce+pull for addressable blocks, suppress noise. Never reads verbatim code.
- **Code→speakable normalization pass** folded into the pipeline (e.g., `camelCase`→words, `auth.rs`→"auth, rust file", operator mapping).
- **Local Kokoro TTS**, warm-resident / on-device, behind the `TtsProvider` trait — a privacy/exfil-driven default, not only a cost decision.
- **Cancellable playback queue with barge-in** — skip cuts current TTS near-instantly at segment boundaries with no orphaned half-played state; read-segments are sub-chunked to ~sentence units.
- **Always-on attention events** that preempt across all sessions (permission gates / waiting-on-user), with engineered delivery guarantees: dedup by event id, fail-loud on audio-device unavailability, and "while you were away" replay on wake.
- **Mode toggle** (also the built-in kill switch), **trivial-turn filter** (suppress completed turns below the ~15–20 spoken-word one-breath threshold when mode is ON; attention events still always speak), and **pull-on-demand** as full read-out only.
- **Multi-session focus arbitration** — exactly one focused session narrates at a time (frontmost terminal by default, manual menubar pin override); attention events still preempt across all sessions.
- **Two minimal surfaces** — a macOS menubar app and global hotkeys; audio output itself is the primary surface.

### Out of scope (deferred to Phase 2 or later)

- **Additional sources / adapters.** The Codex CLI adapter and any desktop (thick) adapters are deferred until the `SourceAdapter` trait is proven on Claude Code.
- **LLM preprocessors.** The summarize and re-voice preprocessors — and therefore the summarize variant of Pull — are deferred; v1 ships no LLM, and Pull is full read-out only. The Pull seam is kept so Phase 2 can add summarize-on-demand without reshaping the pipeline.
- **Token-level streaming, voice control, and cloud premium voices** (ElevenLabs / Cartesia), plus **secret-redaction before speaking**, which is an explicit aware-but-deferred item under the v1 assumption of a private/trusted audio environment.

## Success metrics

Success is daily personal use, proven behaviorally — not external adoption. Each KPI below is specific and testable over a week of real Claude Code sessions on macOS.

1. **Narration adoption:** narration mode stays ON for ≥ 90% of active Claude-Code session time across a representative one-week window (measured from mode-toggle state in structured logs).
2. **Zero missed attention prompts:** 0 missed permission/attention prompts when stepped away — every `AttentionEvent` is spoken (or surfaced via the fail-loud menubar badge if audio is unavailable), with no silent drops.
3. **Attention latency (critical path):** time from hook fire to first spoken word of an attention event < ~1 s at the typical case; first audio uses a pre-synthesized phrase when Kokoro cannot synthesize in time.
4. **Turn-narration first-audio latency:** sub-second from `TurnComplete` to the first `Utterance` becoming audible (the load-bearing Kokoro-on-M1 latency is validated empirically against this target via logged TTS first-audio timing).
5. **Barge-in responsiveness:** skip → silence is near-instant (cancellation at the current sub-sentence segment boundary; no orphaned playback).
6. **Chunker quality bar:** the labeled ReplayAdapter fixture corpus — covering patches, command output, errors, plans, test failures, permission prompts, file edits, and terminal logs — passes its golden tier-label assertions, so a green run means genuinely useful narration (the crown-jewel regression net).
7. **Trust proof (behavioral):** the operator does not revert to staring at the screen during long runs across the one-week window — the single proof that "walk away and be called back" works.

## Use cases

Each use case names the actor(s), the trigger that initiates the interaction, and the observable outcome that satisfies the actor. UC IDs are stable and consumed downstream.

- **UC-1 — First-run setup and lifecycle.**
  - actor: P1, P2
  - trigger: operator installs/launches the daemon and menubar app for the first time after a fresh checkout.
  - outcome: the launchd user agent is registered (auto-start at login, restart on crash), the one-time macOS accessibility permission for global hotkeys is granted, Kokoro weights are present and warmed resident, and the menubar shows a ready/idle state.

- **UC-2 — Toggle narration mode for a session.**
  - actor: P1, P2
  - trigger: operator hits the toggle-mode hotkey or menubar control.
  - outcome: narration mode flips ON or OFF for the session; when ON, completed turns are narrated; when OFF, turn narration is suppressed but attention events still always speak. The toggle also acts as a kill switch if a build misbehaves.

- **UC-3 — Narrate a completed turn content-aware (core loop).**
  - actor: P1, P2
  - trigger: a Claude Code `Stop` hook fires `TurnComplete` while mode is ON for the focused session.
  - outcome: the turn's `Segment`s narrate in source order — prose read, operational edits announced, inline code normalized to speakable form, verbatim code never read — so the operator consumes the turn eyes-free without losing technical faithfulness.

- **UC-4 — Suppress a trivial turn.**
  - actor: P1, P2
  - trigger: a completed turn whose total speakable output (after normalization) falls below the one-breath threshold (~15–20 spoken words) while mode is ON.
  - outcome: the turn is silently suppressed (no `Utterance` enqueued), keeping low-value chatter out of the audio stream — while any attention event in scope still speaks regardless.

- **UC-5 — Hear and act on an always-on attention event.**
  - actor: P1, P2
  - trigger: a Claude Code `Notification` hook signals a permission gate or waiting-on-user state in any session (focused or not), even with mode OFF.
  - outcome: the attention event preempts the playback queue across all sessions and speaks which action is needed (e.g., "Claude needs permission to run git push"), so a blocked session is never silently waiting; the event is deduplicated by event id.

- **UC-6 — Barge-in / skip the current narration.**
  - actor: P1, P2
  - trigger: operator presses the skip hotkey during playback.
  - outcome: the current TTS clip is cancelled near-instantly at the nearest sub-sentence segment boundary and the queue advances, with no orphaned half-played audio.

- **UC-7 — Pause and resume narration.**
  - actor: P1, P2
  - trigger: operator presses the pause/resume hotkey.
  - outcome: playback halts and later resumes cleanly from the queued position, without dropping queued utterances or pending attention events.

- **UC-8 — Pull the full read-out of an announced block.**
  - actor: P1, P2
  - trigger: operator presses the pull hotkey after hearing an announced (announce / announce+pull) block on the focused session.
  - outcome: the full content of that addressable block is read aloud verbatim-as-speakable (full read-out only in v1), letting the operator drill into a block they were only told about.

- **UC-9 — Replay the last narration.**
  - actor: P1, P2
  - trigger: operator presses the replay-last hotkey on the focused session.
  - outcome: the most recent narrated utterance(s) are spoken again, so a missed or half-heard segment can be recovered without rerunning the agent.

- **UC-10 — Adjust playback speed.**
  - actor: P1, P2
  - trigger: operator presses the speed-up / speed-down hotkey.
  - outcome: TTS playback rate changes immediately and persists for subsequent utterances, letting the operator skim faster or slow down for dense content.

- **UC-11 — Arbitrate focus across concurrent sessions.**
  - actor: P1
  - trigger: the operator runs multiple Claude Code sessions in parallel (several terminals/worktrees) and switches the frontmost terminal or pins a session via the menubar.
  - outcome: exactly one focused session is narrated at a time (frontmost by default, manual pin override); non-focused turn narration is queued or muted per setting, while attention events from any session still preempt regardless of focus.

- **UC-12 — Recover queued attention events after sleep/wake.**
  - actor: P1, P2
  - trigger: the Mac sleeps with attention events queued, then wakes.
  - outcome: the queued attention events fire on wake with a "while you were away" marker, so an event raised during sleep is surfaced rather than lost.

- **UC-13 — Be warned when narration degrades (schema-probe / fail-loud).**
  - actor: P1, P2
  - trigger: the Claude Code JSONL structured parse fails (the schema-version probe detects a changed/unversioned shape) or the audio device becomes unavailable.
  - outcome: the daemon degrades loudly — it drops to flattened-text narration with a spoken "degraded mode" notice and a persistent menubar badge, and on audio-device loss raises a visible badge rather than silently dropping audio — so the operator knows narration quality fell below the intelligent bar and can investigate.

- **UC-14 — Configure voices, hotkeys, and thresholds.**
  - actor: P1, P2
  - trigger: operator edits settings via the menubar or the plain config file (voice/persona, hotkey bindings, trivial-turn and chunker thresholds).
  - outcome: settings persist to the config file and take effect for subsequent narration, letting the operator tune the experience without code changes.

- **UC-15 — Inspect daemon state and recent activity.**
  - actor: P1, P2
  - trigger: operator opens the menubar app.
  - outcome: the menubar shows current state (narrating / paused / current segment), mode status, any degraded-mode or audio-failure badge, and access to settings — the daemon's legible, VoiceOver-compatible face.

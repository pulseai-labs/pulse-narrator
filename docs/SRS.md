# PulseVoice — Software Requirements Specification

> Derived from [MASTER-SPEC.md](../../pulse-narrator-ai/MASTER-SPEC.md), the project Executive Summary, and [PRD.md](./PRD.md).
> Functional requirements (`FR-*`) are minted here and trace to the PRD use cases (`UC-*`).
> Non-functional requirements (`NFR-*`) are minted here and are phrased test-ably where they are an acceptance bar.

**Document version:** 1.0
**Date:** 2026-05-29
**Project class:** Other (personal developer tool — local-first macOS daemon)

---

## Functional Requirements

Each `FR-N` states a capability the system MUST do and traces to one or more PRD use cases. Requirements describe observable system behavior, not implementation choices.

### Setup, lifecycle, and configuration

- **FR-1 — Daemon lifecycle and warm start.** The system MUST install a launchd user agent that auto-starts the daemon at login and restarts it on crash, warm Kokoro TTS resident at daemon startup (never lazily per-utterance), verify Kokoro model weights are present, and report a ready/idle state once the pipeline is live. (traces_uc: UC-1)

- **FR-2 — One-time accessibility grant for global hotkeys.** The system MUST surface and use the one-time macOS accessibility permission required for system-wide global hotkeys, and MUST function as a daemon for narration even before the grant is given (hotkey control degrades, narration does not). (traces_uc: UC-1)

- **FR-3 — Persist and apply settings.** The system MUST let the operator configure voice/editorial persona, hotkey bindings, the trivial-turn threshold, and chunker thresholds via the menubar or a plain config file; settings MUST persist to the config file and take effect for subsequent narration without a code change. (traces_uc: UC-14)

- **FR-4 — Expose daemon state and recent activity.** The system MUST present a legible, VoiceOver-compatible menubar surface that shows current state (narrating / paused / current segment), mode status, any degraded-mode or audio-failure badge, and access to settings. (traces_uc: UC-15)

### Narration core loop

- **FR-5 — Ingest completed turns from Claude Code.** The system MUST receive Claude Code `Stop`/`Notification` hook events over a Unix domain socket, read the JSONL transcript on `TurnComplete`, and produce a normalized event stream (`Segment` / `AttentionEvent` / `TurnComplete`) that is surface-agnostic downstream of the source adapter. (traces_uc: UC-3, UC-5)

- **FR-6 — Classify each segment into a chunk decision (content-aware tiering).** The system MUST classify every `Segment` by role × estimated-spoken-duration into a `ChunkDecision` of read / announce / announce+pull / suppress, such that prose is read, operational edits are announced, addressable blocks are announce+pull, and noise is suppressed. (traces_uc: UC-3)

- **FR-7 — Normalize code to speakable form.** The system MUST run a code→speakable normalization pass on read/announce content (e.g., `camelCase`→words, `auth.rs`→"auth, rust file", operator mapping) and MUST never read verbatim code character-by-character; output MUST remain technically faithful. (traces_uc: UC-3, UC-8)

- **FR-8 — Narrate a completed turn in source order.** When narration mode is ON for the focused session, the system MUST narrate a turn only after `TurnComplete` (not token-by-token), emitting its `Utterance`s in source `Segment` order so the operator consumes the turn eyes-free. (traces_uc: UC-3)

- **FR-9 — Toggle narration mode.** The system MUST let the operator flip narration mode ON/OFF per session via a hotkey or menubar control; with mode ON, completed turns are narrated; with mode OFF, turn narration is suppressed while attention events still always speak. The toggle MUST also act as a built-in kill switch. (traces_uc: UC-2)

- **FR-10 — Suppress trivial turns.** When mode is ON, the system MUST suppress (enqueue no `Utterance`) a completed turn whose total speakable output after normalization falls below the one-breath threshold (~15–20 spoken words), while still always speaking any attention event in scope. (traces_uc: UC-4)

### Always-on attention path

- **FR-11 — Always-speak preemptive attention events.** The system MUST treat a permission-gate / waiting-on-user signal from any session — focused or not, and even with mode OFF — as an out-of-band `AttentionEvent` that preempts the playback queue across all sessions, conveying which action is needed (e.g., "Claude needs permission to run git push"). (traces_uc: UC-5, UC-11)

- **FR-12 — Deduplicate attention events.** The system MUST deduplicate `AttentionEvent`s by event id so a re-signalled event does not speak twice. (traces_uc: UC-5)

- **FR-13 — Fail loud on audio-device unavailability.** When the audio device is unavailable, the system MUST raise a visible menubar badge for any pending attention event rather than silently dropping it. (traces_uc: UC-5, UC-13)

- **FR-14 — Pre-synthesized attention fallback.** When Kokoro cannot synthesize the attention phrase in time, the system MUST fall back to a pre-synthesized spoken phrase that names the action class (e.g., "Claude needs permission", "agent is waiting for input"); a bare earcon/beep is insufficient. (traces_uc: UC-5)

- **FR-15 — Recover queued attention events after sleep/wake.** When the Mac wakes with attention events queued from sleep, the system MUST fire those events with a "while you were away" marker rather than losing them. (traces_uc: UC-12)

### Playback control

- **FR-16 — Barge-in / skip.** On the skip hotkey, the system MUST cancel the current TTS clip near-instantly at the nearest sub-sentence segment boundary and advance the queue, leaving no orphaned half-played audio. (traces_uc: UC-6)

- **FR-17 — Pause and resume.** On the pause/resume hotkey, the system MUST halt playback and later resume cleanly from the queued position without dropping queued utterances or pending attention events. (traces_uc: UC-7)

- **FR-18 — Pull full read-out of an announced block.** On the pull hotkey after an announce / announce+pull block on the focused session, the system MUST read that addressable block's full content aloud as normalized-speakable text (full read-out only in v1). (traces_uc: UC-8)

- **FR-19 — Replay last narration.** On the replay-last hotkey for the focused session, the system MUST speak the most recent narrated utterance(s) again so a missed or half-heard segment is recoverable without rerunning the agent. (traces_uc: UC-9)

- **FR-20 — Adjust playback speed.** On the speed-up / speed-down hotkey, the system MUST change the TTS playback rate immediately and persist that rate for subsequent utterances. (traces_uc: UC-10)

### Multi-session arbitration

- **FR-21 — Single focused narration with manual pin.** The system MUST narrate exactly one focused session at a time (frontmost terminal by default, with a manual menubar pin override) and MUST queue or mute non-focused turn narration per setting, while keeping the attention path preemptive across all sessions. (traces_uc: UC-11)

- **FR-22 — Replay/pull act on the focused session.** The system MUST scope the replay-last and pull hotkeys to the currently focused (or pinned) session. (traces_uc: UC-9, UC-8, UC-11)

### Resilience and loud degradation

- **FR-23 — Schema-probe with loud flattened-text degradation.** When the Claude Code JSONL structured parse fails (the schema-version probe detects a changed/unversioned shape), the system MUST degrade to flattened-text narration AND announce it loudly — a spoken "degraded mode" notice plus a persistent menubar badge — so the operator knows narration quality fell below the intelligent bar. (traces_uc: UC-13)

- **FR-24 — Hook fails fast when no daemon listens.** When a Claude Code hook fires and no daemon is listening on the socket, the hook MUST log and exit non-zero rather than blocking the agent. (traces_uc: UC-1, UC-5)

---

## Non-Functional Requirements

Each `NFR-N` is derived from a quality attribute — latency/throughput budgets, determinism invariants, security/privacy invariants, or the test/coverage quality bar — and is phrased test-ably where it is an acceptance bar.

### Latency and throughput

- **NFR-1 — Attention-event latency (critical path).** Time from attention hook fire to first spoken word (or pre-synthesized fallback) MUST be < ~1 s at the typical case. (traces_uc: UC-5)

- **NFR-2 — Turn first-audio latency.** Time from `TurnComplete` to the first `Utterance` becoming audible MUST be sub-second at the typical case; the Kokoro-on-M1 first-audio latency MUST be validated empirically against this target via logged TTS first-audio timing. (traces_uc: UC-3)

- **NFR-3 — Barge-in responsiveness.** Skip → silence MUST be near-instant, cancelling at the current sub-sentence segment boundary with no orphaned playback. (traces_uc: UC-6)

- **NFR-4 — Steady-state throughput.** Under sustained narration, TTS synthesis MUST proceed faster than real-time speech so the playback queue drains and does not back up at human-paced agent turn rates. (traces_uc: UC-3)

- **NFR-5 — Warm-resident TTS.** Kokoro MUST stay warm-resident in the daemon (warmed at startup) and MUST NOT be lazily loaded per-utterance, so first-audio latency is not paid on each turn. (traces_uc: UC-1, UC-3)

### Determinism and faithfulness invariants

- **NFR-6 — Faithfulness invariant.** Narration MUST never alter technical content; the lossy/expensive op (summarize) MUST run only on explicit Pull and never automatically — and in v1, Pull is full read-out only with no summarize path active. (traces_uc: UC-3, UC-8)

- **NFR-7 — Turn-as-aggregate ordering.** A turn MUST narrate only after `TurnComplete` and its `Segment`s MUST narrate in source order; the system MUST NOT emit token-by-token in v1. (traces_uc: UC-3)

- **NFR-8 — Attention events never suppressed.** `AttentionEvent`s MUST always speak (or fail loud visibly) and MUST never be suppressed or dropped — even when mode is OFF or the trivial-turn filter is active. (traces_uc: UC-4, UC-5, UC-9)

- **NFR-9 — Clean barge-in state invariant.** A skip MUST cut current TTS with no orphaned half-played state; the playback queue MUST be cancellable cleanly at segment boundaries. (traces_uc: UC-6)

- **NFR-10 — Normalizer output invariants (property-tested).** The normalizer MUST satisfy, under property tests, that it never emits a raw underscore or scope-colons, always produces non-empty speakable text, and is idempotent on already-normalized input. (traces_uc: UC-3, UC-7 via FR-7)

- **NFR-11 — Surface-agnostic invariant.** Everything downstream of the source adapter MUST NOT know which agent produced the text; source-specific transcript shapes MUST be mapped by the adapter into source-neutral `Segment` metadata (role, kind, typed fields) only. (traces_uc: UC-3)

- **NFR-12 — No-panic resilience.** The pipeline MUST NOT panic on bad input; a malformed transcript, a Kokoro failure, or a poisoned segment MUST degrade the affected segment/session while keeping the always-on daemon alive. (traces_uc: UC-5, UC-13)

### Security and privacy invariants

- **NFR-13 — Local-only default path.** In the v1 default path the system MUST NOT send agent output (source code, paths, reasoning, incidental secrets) to any cloud service; TTS MUST run on-device (local Kokoro) so no exfiltration occurs. (traces_uc: UC-3, UC-5)

- **NFR-14 — Loopback-only IPC.** Daemon ↔ menubar / hook IPC MUST be local-only (loopback / Unix domain socket) with no public internet exposure; if any daemon↔node traffic crosses the LAN it MUST be trusted-LAN-only / authenticated, never public. (traces_uc: UC-1, UC-5, UC-15)

- **NFR-15 — No auth, single-tenant.** The system MUST operate as a single local user on their own machine with no user accounts and no data collection; the only OS-level grant is the one-time macOS accessibility permission for global hotkeys. (traces_uc: UC-1, UC-2)

- **NFR-16 — Private-audio-environment assumption (recorded).** v1 MUST operate under the explicit assumption of a private/trusted audio environment; secret-redaction before speaking is a deliberate aware-but-deferred item and MUST be recorded as such (not silently assumed safe). (traces_uc: UC-3)

- **NFR-17 — Local-only observability.** Logs MUST be written to a rotating local file at `~/Library/Logs/PulseVoice/` (plus stderr in dev) with no remote telemetry; the system MUST log hook-event receipt, transcript parse outcomes (incl. schema-probe degradations), chunk decisions, TTS first-audio latency, and barge-in events. (traces_uc: UC-13, UC-15)

### Quality gates and test bars

- **NFR-18 — Coverage floor on pure-logic crates.** Test coverage MUST be ≥ ~80% on the pure-logic crates (`pulse-core`, `pulse-pipeline` = chunker + normalizer); coverage on I/O / audio / OS-integration crates (`pulse-daemon`, `pulse-tts`, `pulse-menubar`) is pragmatic and validated manually rather than gated. (traces_uc: UC-3)

- **NFR-19 — Chunker golden tier-label bar.** The labeled ReplayAdapter fixture corpus MUST cover the hard cases (patches, command output, errors, plans, test failures, permission prompts, file edits, terminal logs) and the golden tests MUST assert each segment's `ChunkDecision` against its expected tier label; a green run MUST mean genuinely useful narration. (traces_uc: UC-3, UC-4, UC-5)

- **NFR-20 — Pre-merge gates clean.** Before merge the system MUST pass `cargo fmt --check`, `cargo clippy -D warnings`, and `cargo test` green — including the ReplayAdapter golden tests and the normalizer proptest suite — with coverage not below the ~80% floor on the pure-logic crates. (traces_uc: UC-3)

### Accessibility and degradation

- **NFR-21 — Legible, VoiceOver-compatible menubar.** The menubar UI MUST follow native macOS conventions with system font sizing and VoiceOver-compatible labels on menu items. (traces_uc: UC-15, UC-14)

- **NFR-22 — Degradation is never silent.** Any drop below the intelligent narration bar — flattened-text degradation or audio-device loss — MUST be conveyed both audibly (a spoken notice or pre-synthesized phrase) and visibly (a persistent menubar badge); it MUST NOT masquerade as normal operation. (traces_uc: UC-13)

---

## Traceability

Every functional and non-functional requirement maps to at least one PRD use case. The reverse mapping (use case → requirements) confirms every `UC` in the ledger is covered.

### Requirement → Use case

| Requirement | Traces to | Quality attribute (NFR) |
| --- | --- | --- |
| FR-1 | UC-1 | — |
| FR-2 | UC-1 | — |
| FR-3 | UC-14 | — |
| FR-4 | UC-15 | — |
| FR-5 | UC-3, UC-5 | — |
| FR-6 | UC-3 | — |
| FR-7 | UC-3, UC-8 | — |
| FR-8 | UC-3 | — |
| FR-9 | UC-2 | — |
| FR-10 | UC-4 | — |
| FR-11 | UC-5, UC-11 | — |
| FR-12 | UC-5 | — |
| FR-13 | UC-5, UC-13 | — |
| FR-14 | UC-5 | — |
| FR-15 | UC-12 | — |
| FR-16 | UC-6 | — |
| FR-17 | UC-7 | — |
| FR-18 | UC-8 | — |
| FR-19 | UC-9 | — |
| FR-20 | UC-10 | — |
| FR-21 | UC-11 | — |
| FR-22 | UC-9, UC-8, UC-11 | — |
| FR-23 | UC-13 | — |
| FR-24 | UC-1, UC-5 | — |
| NFR-1 | UC-5 | Latency (critical path) |
| NFR-2 | UC-3 | Latency (turn first-audio) |
| NFR-3 | UC-6 | Latency (barge-in) |
| NFR-4 | UC-3 | Throughput (steady-state) |
| NFR-5 | UC-1, UC-3 | Latency (warm-resident) |
| NFR-6 | UC-3, UC-8 | Faithfulness invariant |
| NFR-7 | UC-3 | Ordering invariant |
| NFR-8 | UC-4, UC-5, UC-9 | Attention invariant |
| NFR-9 | UC-6 | Barge-in state invariant |
| NFR-10 | UC-3 | Normalizer property invariants |
| NFR-11 | UC-3 | Surface-agnostic invariant |
| NFR-12 | UC-5, UC-13 | No-panic resilience |
| NFR-13 | UC-3, UC-5 | Privacy / exfil |
| NFR-14 | UC-1, UC-5, UC-15 | Security (IPC) |
| NFR-15 | UC-1, UC-2 | Security (auth/tenancy) |
| NFR-16 | UC-3 | Security (recorded assumption) |
| NFR-17 | UC-13, UC-15 | Observability (local-only) |
| NFR-18 | UC-3 | Quality gate (coverage) |
| NFR-19 | UC-3, UC-4, UC-5 | Quality gate (chunker eval) |
| NFR-20 | UC-3 | Quality gate (pre-merge) |
| NFR-21 | UC-15, UC-14 | Accessibility |
| NFR-22 | UC-13 | Degradation visibility |

### Use case → Requirement (coverage check)

| Use case | Covered by |
| --- | --- |
| UC-1 — First-run setup and lifecycle | FR-1, FR-2, FR-24, NFR-5, NFR-14, NFR-15 |
| UC-2 — Toggle narration mode | FR-9, NFR-15 |
| UC-3 — Narrate a completed turn content-aware | FR-5, FR-6, FR-7, FR-8, NFR-2, NFR-4, NFR-5, NFR-6, NFR-7, NFR-10, NFR-11, NFR-13, NFR-16, NFR-18, NFR-19, NFR-20 |
| UC-4 — Suppress a trivial turn | FR-10, NFR-8, NFR-19 |
| UC-5 — Hear and act on an attention event | FR-5, FR-11, FR-12, FR-13, FR-14, FR-24, NFR-1, NFR-8, NFR-12, NFR-13, NFR-14, NFR-19 |
| UC-6 — Barge-in / skip | FR-16, NFR-3, NFR-9 |
| UC-7 — Pause and resume | FR-17 |
| UC-8 — Pull full read-out | FR-7, FR-18, FR-22, NFR-6 |
| UC-9 — Replay last narration | FR-19, FR-22, NFR-8 |
| UC-10 — Adjust playback speed | FR-20 |
| UC-11 — Arbitrate focus across sessions | FR-11, FR-21, FR-22 |
| UC-12 — Recover queued attention events after sleep/wake | FR-15 |
| UC-13 — Be warned when narration degrades | FR-13, FR-23, NFR-12, NFR-17, NFR-22 |
| UC-14 — Configure voices, hotkeys, thresholds | FR-3, NFR-21 |
| UC-15 — Inspect daemon state and recent activity | FR-4, NFR-14, NFR-17, NFR-21 |

# PulseVoice — Roadmap

> Derived from MASTER-SPEC.md by `/plan-roadmap` on 2026-05-29.
> Co-edited by user + scaffold-dev orchestrator over time.

## Roadmap overview

A macOS daemon + menubar app (PulseVoice) that taps the structured output of CLI coding agents (Claude Code, Codex CLI) and narrates it aloud, content-aware, so agent responses can be consumed eyes-free. It is the output-side complement to voice input (Wispr Flow).

**Visionary horizon (Phases).** Over a multi-year arc PulseVoice grows from a faithful narrator into a full eyes-and-hands-free loop. **Phase 1 — Faithful Narration (v1)** delivers the daily-usable core: tap Claude Code's structured output, narrate completed turns content-aware via local Kokoro TTS, and never miss an attention prompt. **Phase 2 — Eyes-and-hands-free loop** adds voice control (closing the Wispr-Flow loop), the Codex CLI adapter, desktop adapters, and token-streaming. **Phase 3 — Interpretation layer** turns on the deliberately-deferred lossy capabilities: summarize/re-voice LLM preprocessors and premium cloud voices.

**Value-building windows (Sprints).** Only Phase 1 is decomposed for build now (Phases 2–3 stay sprint-level placeholders until v1 ships). Phase 1's four sprints compound deliberately: **1.1 Tap & Attention Path** front-loads the two hardest unknowns (the JSONL hook tap and Kokoro-on-M1 latency) and proves the end-to-end thread with a throwaway spike; **1.2 Content-Aware Narration** builds the crown-jewel chunker + normalizer; **1.3 Playback Control & Multi-Session** lays the control-plane foundation then the full hotkey surface and session arbitration; **1.4 Hardening & Daily-Use Confidence** brings menubar, settings, loud degradation, observability, and the install/lifecycle work that makes "leave it on for a week" real.

**Visibility cycles (Vertical Slices).** Phase 1's 17 slices each ship something demoable and trace to the requirements baseline (PRD use cases → SRS `FR`/`NFR` → `BACKLOG`), covering all 24 FRs, 22 NFRs, and 20 backlog items. Each slice carries `auto:`/`user:` demo criteria — with quantified gates on the risk-bearing ones (p95 first-audio < 1 s, p95 cancellation < 150 ms, golden-corpus chunk accuracy ≥ 90%) — so "done" is observable, not asserted. scaffold-dev's orchestrator consumes this hierarchy as its R1 input contract.

## Phase 1: Faithful Narration (v1) — now to ~4-6 weeks (no deadline)

The MVP working in daily use: Claude Code Stop/Notification hook tap over a Unix socket -> content-aware chunker (role x size tiering) -> code->speakable normalization -> warm-resident local Kokoro TTS -> cancellable playback queue with barge-in. Mode toggle, trivial-turn filter, pull (full read-out only), and always-on attention events that preempt across sessions. At the end of this phase the operator can step away during a long agent run and trust being called back when the agent needs input. Nearly all FR/NFR/BACKLOG IDs land here.

### Sprint 1.1: Tap & Attention Path

Stand up the Unix-socket hook ingestion, the launchd daemon lifecycle (auto-start, restart, warm-resident Kokoro), and the always-on attention-event path. Demoable at close: a Claude Code permission prompt speaks itself when the operator has stepped away. Front-loads the two hardest unknowns (the JSONL hook tap and Kokoro-on-M1 first-audio latency).

#### VS-1.1.1: Hook ingestion + daemon skeleton

Unix-socket hook receiver, the resident daemon, and the launchd user-agent lifecycle (auto-start, restart on crash); hook fails fast + exits non-zero when no daemon listens. Resolves the REAL Claude Code integration hazards up front: transcript-path discovery from the hook payload, reading only complete transcript writes, session-identity correlation across terminal restarts, and idempotent handling of duplicate/retried hooks.

##### Traceability

- FR: FR-1, FR-5, FR-24
- NFR: NFR-12, NFR-14, NFR-15
- Backlog: BACKLOG-3, BACKLOG-19

##### Demo criteria

- [ ] auto: deliver a recorded hook event over the Unix socket → daemon receives and logs the TurnComplete / Notification event
- [ ] user: kill the daemon then fire a Claude Code hook → the hook logs and exits non-zero without blocking the agent
- [ ] auto: against real Claude Code hooks → the adapter discovers the correct transcript path, reads only complete writes, correlates session identity across a terminal restart, and treats duplicate/retried hooks idempotently

#### VS-1.1.2: Warm Kokoro + minimal playback

Warm-resident Kokoro behind the TtsProvider trait, warmed at daemon startup (never lazily per-utterance), able to synthesize and play a fixed self-test phrase on-device (no cloud).

##### Traceability

- FR: FR-1
- NFR: NFR-2, NFR-5, NFR-13
- Backlog: BACKLOG-3

##### Demo criteria

- [ ] auto: start the daemon → Kokoro is warmed resident and a self-test phrase synthesizes within the first-audio latency target
- [ ] user: trigger the self-test → hear the spoken test phrase within about one second
- [ ] auto: measure first-audio latency across 20 self-test runs on the M1 baseline → p95 from trigger to first audible sample < 1 s

#### VS-1.1.3: Always-on attention events

Notification-hook -> AttentionEvent, out-of-band and preemptive across sessions, conveying which action is needed; dedup by event id; pre-synthesized phrase fallback; fail-loud to menubar on audio loss; sleep/wake recovery.

##### Traceability

- FR: FR-11, FR-12, FR-13, FR-14, FR-15
- NFR: NFR-1, NFR-8
- Backlog: BACKLOG-2, BACKLOG-12

##### Demo criteria

- [ ] auto: feed a permission Notification event plus a duplicate event id → a 'Claude needs permission' utterance is spoken under ~1s and the duplicate speaks only once
- [ ] user: step away and trigger a real permission prompt → hear which action is needed spoken aloud
- [ ] auto: measure attention-event latency across 20 permission events → p95 from hook fire to first spoken word < 1 s, and the hook never blocks the agent beyond a small bounded time

#### VS-1.1.4: End-to-end narration spike (throwaway)

A throwaway end-to-end thread that de-risks the #1 project risk (chunker) early: real recorded transcript fixture -> rough chunk decisions -> normalization -> Kokoro synthesis -> minimal play+cancel. Validates chunker plausibility, Kokoro-on-M1 first-audio timing, and cancellation behavior IN Sprint 1.1 rather than discovering them in Sprint 1.2/1.3. Discarded/absorbed once the real chunker (VS-1.2.2) lands.

##### Traceability

- FR: None
- NFR: NFR-2
- Backlog: BACKLOG-18

##### Demo criteria

- [ ] auto: run the spike over a recorded transcript fixture → measured Kokoro first-audio latency on the M1 baseline is logged and the rough chunk tiers are plausible
- [ ] user: hear a real (rough) turn narrated end-to-end → confirm the latency and intelligibility feel acceptable enough to commit to the approach

### Sprint 1.2: Content-Aware Narration

Build the crown-jewel chunker (role x size tiering, duration threshold), the code->speakable normalization pass, and full turn narration on TurnComplete. Demoable at close: a completed Claude Code turn is narrated intelligently - operational edits announced, prose read, inline code normalized, noise suppressed.

#### VS-1.2.1: JSONL parse + Segment model

Read the Claude Code JSONL transcript on TurnComplete with a schema-version probe; map agent-specific shapes into a source-neutral ordered Segment stream (role/kind/typed fields); degrade to flattened-text on parse failure.

##### Traceability

- FR: FR-5, FR-23
- NFR: NFR-7, NFR-11
- Backlog: BACKLOG-1, BACKLOG-13, BACKLOG-19

##### Demo criteria

- [ ] auto: parse a recorded JSONL transcript fixture → the expected ordered Segment stream with role and kind tags is produced
- [ ] auto: feed a changed-schema / malformed fixture → the pipeline degrades to flattened-text narration without crashing

#### VS-1.2.2: Content-aware chunker

The crown-jewel chunker: classify each Segment by role x estimated-spoken-duration into a ChunkDecision (read / announce / announce+pull / suppress), validated against a labeled golden corpus covering the hard cases.

##### Traceability

- FR: FR-6
- NFR: NFR-19
- Backlog: BACKLOG-1, BACKLOG-18

##### Demo criteria

- [ ] auto: run the golden ReplayAdapter corpus through the chunker → every segment ChunkDecision matches its expected tier label across the hard cases
- [ ] auto: run the labeled golden corpus → chunk-tier classification accuracy ≥ 90% with a bounded false-suppress rate on operational/attention segments

#### VS-1.2.3: Normalization + full turn narration

The code->speakable normalization pass (camelCase/snake_case split, file basenames, operator mapping, never verbatim code) plus full turn narration emitting Utterances in source Segment order on TurnComplete; the pure-logic crates land here under their coverage floor, and the private-audio-environment assumption becomes operative once substantive content is spoken.

##### Traceability

- FR: FR-7, FR-8
- NFR: NFR-6, NFR-10, NFR-16, NFR-18
- Backlog: BACKLOG-1, BACKLOG-17

##### Demo criteria

- [ ] auto: run the normalizer property suite → no raw underscores or scope-colons emitted, output always non-empty, idempotent on normalized input
- [ ] user: narrate a real completed turn → operational edits are announced, prose is read, inline code is spoken normalized

### Sprint 1.3: Playback Control & Multi-Session

Add the full playback control surface (barge-in/skip, pause/resume, replay-last, speed +/-, pull full-read-out), the mode toggle + trivial-turn filter, and focused-session arbitration across parallel sessions. Demoable at close: complete hotkey-driven control with attention events preempting across all sessions.

#### VS-1.3.0: Control-plane foundation

The control primitives the rest of Sprint 1.3 depends on: the menubar shell process skeleton, the versioned daemon<->menubar IPC envelope over the Unix socket, and global-hotkey registration. Sequenced first in Sprint 1.3 so mode toggle, pull, speed-persist, and focus arbitration have a real control surface to attach to (fixes the build-order inversion where 1.3 features assumed primitives that 1.4 built later).

##### Traceability

- FR: FR-2
- NFR: NFR-14
- Backlog: BACKLOG-16

##### Demo criteria

- [ ] auto: the menubar process connects to the daemon over the IPC socket and a registered global hotkey round-trips a command to the daemon
- [ ] user: press a registered global hotkey with the menubar running → the daemon receives and acts on the command

#### VS-1.3.1: Playback queue + barge-in

The cancellable PlaybackQueue with barge-in: skip cancels the current TTS clip near-instantly at the nearest sub-sentence boundary with no orphaned half-played audio; steady-state synthesis drains faster than real-time speech.

##### Traceability

- FR: FR-16
- NFR: NFR-3, NFR-9, NFR-4
- Backlog: BACKLOG-6

##### Demo criteria

- [ ] auto: skip mid-clip in a playback-queue test → current TTS cancels at the sub-sentence boundary with no orphaned audio
- [ ] user: press skip mid-utterance → narration cuts to silence near-instantly
- [ ] auto: measure skip→silence across 20 barge-ins → p95 cancellation latency < 150 ms with zero orphaned audio

#### VS-1.3.2: Pause/resume, replay, speed

The non-destructive playback controls: pause/resume from queued position, replay-last, and speed up/down that persists for subsequent utterances.

##### Traceability

- FR: FR-17, FR-19, FR-20
- NFR: None
- Backlog: BACKLOG-7, BACKLOG-9, BACKLOG-11

##### Demo criteria

- [ ] auto: exercise pause/resume/replay/speed in a queue test → pause halts and resume continues from position, replay re-speaks the last utterance, speed change persists
- [ ] user: use each playback hotkey during narration → each behaves as expected

#### VS-1.3.3: Mode toggle + trivial filter + pull

Per-session narration mode ON/OFF (kill switch), the trivial-turn filter (suppress sub-one-breath turns while attention events still speak), and pull = full read-out of an announced block (v1: no summarize).

##### Traceability

- FR: FR-9, FR-10, FR-18
- NFR: NFR-8
- Backlog: BACKLOG-4, BACKLOG-5, BACKLOG-8

##### Demo criteria

- [ ] auto: toggle mode OFF then feed a turn plus an attention event → turn narration is suppressed but the attention event still speaks; a sub-15-word turn is suppressed
- [ ] user: pull an announced block → its full content is read aloud

#### VS-1.3.4: Focused-session arbitration

Exactly one focused session narrated at a time (frontmost terminal by default, manual menubar pin override); non-focused turn narration queued/muted; attention events preempt across ALL sessions regardless of focus.

##### Traceability

- FR: FR-21, FR-22
- NFR: None
- Backlog: BACKLOG-10

##### Demo criteria

- [ ] auto: run two concurrent sessions with one focused → only the focused session turns narrate, and an unfocused session permission event still preempts and speaks
- [ ] user: pin a session via the menubar → narration follows the pinned session

### Sprint 1.4: Hardening & Daily-Use Confidence

Menubar UI + TOML settings persistence + loud degradation (schema-probe, audio-loss badge) + structured logging/latency instrumentation + golden (ReplayAdapter) and normalizer proptest suites + the one-time accessibility-grant flow. Demoable at close: install it and leave narration on for a full week of real sessions.

#### VS-1.4.1: Menubar UI + accessibility grant

The legible, VoiceOver-compatible menubar surface (state, mode, degraded/audio badges, settings access) and the one-time macOS accessibility-permission grant flow for global hotkeys.

##### Traceability

- FR: FR-2, FR-4
- NFR: NFR-21
- Backlog: BACKLOG-14, BACKLOG-16

##### Demo criteria

- [ ] auto: render the menubar across daemon states → state, mode, and degraded/audio badges display correctly
- [ ] user: grant macOS accessibility once → global hotkeys work and the menubar reflects ready state

#### VS-1.4.2: TOML settings persistence

Plain TOML config via serde; the daemon holds runtime state as source-of-truth and the menubar mutates via IPC (never writes TOML directly); config loaded at startup and on explicit reload.

##### Traceability

- FR: FR-3
- NFR: None
- Backlog: BACKLOG-15

##### Demo criteria

- [ ] auto: change a setting via IPC then reload config → the setting round-trips through TOML and takes effect
- [ ] user: change a setting then restart the daemon → the setting persists

#### VS-1.4.3: Loud degradation surfacing

Degradation is never silent: schema-probe failure -> spoken 'degraded mode' notice once + persistent menubar badge; audio-device loss -> visible badge. It must not masquerade as normal operation.

##### Traceability

- FR: FR-13, FR-23
- NFR: NFR-22
- Backlog: BACKLOG-13

##### Demo criteria

- [ ] auto: force a schema-probe parse failure → a 'degraded mode' notice is spoken once and a persistent menubar badge appears
- [ ] user: disconnect the audio device during a pending attention event → a visible menubar badge is raised rather than silent loss

#### VS-1.4.4: Observability + latency instrumentation

Structured local logging via tracing to a rotating file at ~/Library/Logs/PulseVoice/ (no remote telemetry): hook-event receipt, transcript parse outcomes, chunk decisions, TTS first-audio latency, barge-in events. Includes a minimal v1 redaction rule: likely-secret tokens (API keys, bearer tokens, obvious credentials) are omitted/redacted in logs rather than written verbatim - logs persist on disk, so this is cheaper and more durable than speak-redaction (which stays deferred per NFR-16).

##### Traceability

- FR: None
- NFR: NFR-2, NFR-17, NFR-20
- Backlog: BACKLOG-20

##### Demo criteria

- [ ] auto: run a narration session → the rotating log at ~/Library/Logs/PulseVoice records hook receipt, chunk decisions, TTS first-audio latency, and barge-in events
- [ ] user: inspect a session log → first-audio latency metrics are present and within the validated target

#### VS-1.4.5: Install / lifecycle / uninstall

Execution-critical macOS daemon lifecycle: launchd registration + app-bundle placement, socket-path + log-dir creation with correct permissions, Kokoro model-asset location/fetch, accessibility (TCC) reset/recovery, crash-restart visibility, and a clean uninstall. Makes 'install it and leave narration on for a week' actually reachable rather than implied.

##### Traceability

- FR: None
- NFR: NFR-12
- Backlog: BACKLOG-3

##### Demo criteria

- [ ] auto: a fresh install registers the launchd agent, creates the socket + log dir with correct perms, locates Kokoro assets, and the daemon auto-starts; uninstall removes all of it cleanly
- [ ] user: install on a clean profile and reboot → narration auto-starts; then uninstall → nothing is left behind

## Phase 2: Eyes-and-hands-free loop — post-v1, ~6-12 months out

Closes the Wispr Flow loop: voice control for narration commands (skip, read that diff, pause), the Codex CLI adapter (proving the SourceAdapter trait against a second source), desktop adapters (Claude Desktop / Codex Desktop via accessibility scraping), and token-streaming narration. At the end the operator drives narration fully hands-free across both CLI agents.

### Sprint 2.1: Voice Control

Voice commands for narration control (skip, read that diff, pause), closing the Wispr Flow voice-in/voice-out loop. Slice-level detail deferred until Phase 1 ships.

### Sprint 2.2: Second Source & Desktop

Codex CLI adapter (proving the SourceAdapter trait against a second source), desktop adapters (Claude Desktop / Codex Desktop via accessibility scraping), and token-streaming narration. Slice-level detail deferred.

## Phase 3: Interpretation layer — year 2+

The deliberately-deferred lossy/interpretive capabilities: the summarize and re-voice LLM preprocessors (trust-gated, opt-in, never default for correctness-sensitive output), pull-to-summarize going live, and cloud premium voices / wider accents behind the TtsProvider trait. Also the point at which a minimal secret-redaction suppressor is revisited (Phase 4 aware-but-deferred item).

### Sprint 3.1: Interpretation Preprocessors

Summarize and re-voice LLM preprocessors (trust-gated, opt-in, never default for correctness-sensitive output); pull-to-summarize goes live; revisit a minimal secret-redaction suppressor. Slice-level detail deferred.

### Sprint 3.2: Premium Voices

Cloud TTS opt-in (ElevenLabs / Cartesia) and wider accents behind the TtsProvider trait, for when a specific voice/persona matters. Slice-level detail deferred.


# PulseVoice — Product Backlog

> Derived from [MASTER-SPEC.md](../../pulse-narrator-ai/MASTER-SPEC.md), [PRD.md](./PRD.md), and [SRS.md](./SRS.md).
> Story IDs (`BACKLOG-N`) are minted here. Each story traces to functional requirements (`FR-*`) and/or use cases (`UC-*`) in the provided ledger.

**Document version:** 1.0
**Date:** 2026-05-29

---

## Story format

Every backlog item follows the standard user story template:

> **As a** \<persona\>, **I want to** \<capability\>, **so that** \<outcome\>.

Personas are drawn from the PRD Users section:
- **P1** — Solo developer / homelab tinkerer (primary persona): privacy-conscious, local-first, frequently multitasking across agent sessions.
- **P2** — Agentic-dev power user: already uses voice input, wants a fully hands-and-eyes-free loop.

Each story includes:
- A **slug** that names the feature at a glance.
- An **Acceptance** criterion that is observable and independently testable.
- A **Traces** annotation listing the FR and/or UC IDs from the ledger that ground the story.

Stories are ordered highest user value first, anchoring on the MVP core use case (UC-3) and working outward.

---

## Initial stories (seeded from MASTER-SPEC.md)

### BACKLOG-1 — Content-aware turn narration (core loop)

**As a** P1 solo developer, **I want** each completed Claude Code turn narrated content-aware — prose read aloud, operational edits announced by name, inline code spoken in normalized speakable form, and verbatim code never droned out — **so that** I can consume the agent's work eyes-free without losing technical faithfulness.

**Acceptance:**
- A completed turn triggers audio only after `TurnComplete` fires (not token-by-token).
- Segments narrate in source order: prose segments produce read utterances; operational-edit segments produce announcement utterances (e.g., "edited auth.rs, 12 lines"); code-only segments are never read character-by-character.
- The normalizer converts `camelCase` identifiers to words, `auth.rs` to "auth, rust file", and scope-colons and raw underscores never appear in spoken output.
- A ReplayAdapter golden test over at least one recorded fixture turn passes with the correct `ChunkDecision` tier label (read / announce / announce+pull / suppress) asserted per segment.
- The entire pipeline produces technically faithful utterances: no content is silently dropped or reworded.

(traces_fr: FR-5, FR-6, FR-7, FR-8, traces_uc: UC-3)

---

### BACKLOG-2 — Always-on attention event preemption

**As a** P1 solo developer who steps away from the screen, **I want** any permission gate or waiting-on-user signal from any agent session spoken immediately and preemptively — even when narration mode is OFF and even from a non-focused session — **so that** a blocked session is never silently waiting while I am doing something else.

**Acceptance:**
- A synthetic `AttentionEvent` injected into the daemon while mode is OFF still produces spoken audio naming the action required (e.g., "Claude needs permission to run git push").
- The event preempts any in-progress turn narration: the current utterance is cancelled and the attention phrase plays first.
- If the same `event_id` is signalled twice, only one spoken event is produced (deduplication).
- When Kokoro cannot synthesize the phrase in time, a pre-synthesized fallback phrase from the approved set plays; a bare earcon does not satisfy this criterion.
- A test using a fake `TtsProvider` confirms the attention utterance is the first item enqueued after preemption, regardless of what was already in the queue.

(traces_fr: FR-11, FR-12, FR-14, traces_uc: UC-5)

---

### BACKLOG-3 — Daemon lifecycle and warm-resident Kokoro startup

**As a** P1 solo developer, **I want** the PulseVoice daemon to start automatically at login, warm Kokoro TTS resident at startup, and report a ready/idle state in the menubar **so that** narration is available the moment I open a terminal without any manual startup step.

**Acceptance:**
- A `launchd` user agent plist is installed that auto-starts `pulse-daemon` at login and restarts it on crash.
- Kokoro model weights are verified present at startup; if absent the daemon reports an error in the menubar rather than starting in a broken state.
- Kokoro is warmed at daemon startup: the time from `TurnComplete` to first audio does not include a model-load cost after the initial warm.
- The menubar shows a ready/idle state indicator once the pipeline is live.
- `cargo test` on `pulse-core` and `pulse-pipeline` passes; the daemon lifecycle is validated manually.

(traces_fr: FR-1, FR-24, traces_uc: UC-1)

---

### BACKLOG-4 — Narration mode toggle and kill switch

**As a** P1 solo developer, **I want** to flip narration mode ON or OFF with a hotkey or menubar control **so that** I can silence turn narration instantly when I need quiet, knowing attention events will still always speak and I have a reliable kill switch if a build misbehaves.

**Acceptance:**
- Pressing the toggle-mode hotkey (or using the menubar control) switches narration mode for the current session; the menubar reflects the new state immediately.
- With mode OFF: no new turn `Utterance`s are enqueued; any in-progress turn narration is stopped.
- With mode OFF: a synthesized `AttentionEvent` still produces spoken audio (the always-on invariant is not broken by the toggle).
- The toggle is reversible: toggling ON resumes normal narration for the next completed turn.

(traces_fr: FR-9, traces_uc: UC-2)

---

### BACKLOG-5 — Trivial-turn filter

**As a** P1 solo developer, **I want** turns whose total speakable content falls below a one-breath threshold to be silently suppressed **so that** low-value chatter (short acknowledgements, status echoes) does not interrupt my flow while real content still comes through.

**Acceptance:**
- A completed turn whose normalized speakable output is below the configured one-breath threshold (~15–20 spoken words) produces no `Utterance`s and no audio.
- A turn above the threshold narrates normally.
- An `AttentionEvent` embedded in a trivially short turn still fires and speaks, regardless of threshold.
- The threshold is configurable via the settings config file and takes effect for the next completed turn without a daemon restart.
- A unit test drives three fixture turns (below threshold, above threshold, and a trivial turn carrying an attention event) and asserts the correct suppression and speak outcomes.

(traces_fr: FR-10, traces_uc: UC-4)

---

### BACKLOG-6 — Barge-in / skip playback

**As a** P1 solo developer listening to narration, **I want** to press a skip hotkey and have the current TTS clip cancelled near-instantly **so that** I can move past a segment I have already understood without waiting for it to finish.

**Acceptance:**
- Pressing the skip hotkey cancels the currently playing utterance at the nearest sub-sentence segment boundary; silence follows within a perceptibly instant response (no orphaned audio continues after the key press).
- The playback queue advances to the next utterance and plays it normally.
- A test using a fake `TtsProvider` that records cancellation events confirms the cancel call is issued and the next utterance begins without a gap.
- The barge-in does not disturb pending `AttentionEvent`s: they remain in the queue at their preemptive priority.

(traces_fr: FR-16, traces_uc: UC-6)

---

### BACKLOG-7 — Pause and resume playback

**As a** P1 solo developer, **I want** to pause the narration queue and resume it cleanly from where I left off **so that** I can take a call or answer a message without losing the queued content for the current turn.

**Acceptance:**
- Pressing the pause/resume hotkey halts audio output immediately; the menubar reflects a paused state.
- Pressing again resumes from the next queued utterance without replaying already-spoken content or dropping any queued utterances.
- `AttentionEvent`s queued during pause speak immediately on resume (or preempt at pause-time if so configured).
- A test confirms no utterances are dropped or reordered after a pause/resume cycle.

(traces_fr: FR-17, traces_uc: UC-7)

---

### BACKLOG-8 — Pull full read-out of an announced block

**As a** P2 agentic-dev power user, **I want** to press a pull hotkey after hearing an announced block and have its full content read aloud as normalized speakable text **so that** I can drill into any block I was only told about without having to look at the screen.

**Acceptance:**
- After an announce / announce+pull `ChunkDecision`, pressing the pull hotkey on the focused session enqueues and speaks the full normalized content of that addressable block.
- The read-out uses the same normalization pass (camelCase, file extensions, operators) as the main narration — verbatim code is not droned out character-by-character.
- If the pull hotkey is pressed when no announce+pull block is addressable, no action is taken (or a brief "nothing to pull" phrase plays).
- Pull acts only on the currently focused (or pinned) session, not all sessions.

(traces_fr: FR-18, FR-22, traces_uc: UC-8, UC-11)

---

### BACKLOG-9 — Replay last narration

**As a** P1 solo developer, **I want** to replay the most recently narrated utterances with a hotkey **so that** I can recover a phrase I missed or only half-heard without rerunning the agent or staring at the screen.

**Acceptance:**
- Pressing the replay-last hotkey on the focused session re-enqueues and speaks the most recent narrated utterance(s) from that session.
- The replay uses the same normalized text as the original; it does not re-parse the transcript.
- Replay is scoped to the focused (or pinned) session; it does not replay utterances from other sessions.
- A test confirms the correct utterance(s) are produced a second time after a replay request.

(traces_fr: FR-19, FR-22, traces_uc: UC-9, UC-11)

---

### BACKLOG-10 — Multi-session focus arbitration

**As a** P1 solo developer running three parallel Claude Code sessions in separate terminals, **I want** the daemon to narrate exactly one focused session at a time (the frontmost terminal by default, with a manual menubar pin option) **so that** I am not bombarded by interleaved narration from every concurrent session, yet I still hear every attention event no matter which session raises it.

**Acceptance:**
- With two sessions active, only the frontmost terminal's completed turns are narrated; the other session's turns are queued or muted per the configured setting.
- Switching the frontmost terminal changes the focused session immediately; the menubar reflects which session is focused.
- Pinning a session via the menubar overrides frontmost-terminal detection and keeps that session focused until unpinned.
- An `AttentionEvent` from the non-focused session still speaks and preempts any in-progress narration from the focused session.
- The replay-last and pull hotkeys operate on the currently focused session, not the session that last spoke an utterance.

(traces_fr: FR-21, FR-22, FR-11, traces_uc: UC-11)

---

### BACKLOG-11 — Adjust playback speed

**As a** P2 agentic-dev power user who wants to skim long turns quickly, **I want** to bump TTS playback speed up or down with hotkeys **so that** I can move at the right pace for the current content density.

**Acceptance:**
- Pressing speed-up / speed-down hotkeys changes the TTS playback rate by a perceptible increment (e.g., ±0.25×) and takes effect on the next utterance or the current one if the TTS engine supports mid-clip rate changes.
- The new rate persists for all subsequent utterances until changed again; it also survives a pause/resume cycle.
- The configured speed is written to the settings config file so it persists across daemon restarts.
- Speed adjustment does not disrupt the barge-in or attention-preemption behavior.

(traces_fr: FR-20, traces_uc: UC-10)

---

### BACKLOG-12 — Sleep/wake attention event recovery

**As a** P1 solo developer whose Mac sleeps during a long agent run, **I want** any attention events that were raised during sleep to fire on wake with a "while you were away" marker **so that** a permission prompt raised while the Mac was asleep is not silently lost.

**Acceptance:**
- Simulating a sleep/wake cycle (or equivalent daemon-pause/resume) with a queued `AttentionEvent` confirms the event speaks on wake with a distinct "while you were away" prefix.
- The event's content (which action is needed) is preserved verbatim alongside the "while you were away" marker.
- Events that were already spoken before sleep are not replayed on wake.
- A test injects an `AttentionEvent` into the queue after a simulated sleep, triggers wake, and asserts the correct spoken sequence.

(traces_fr: FR-15, traces_uc: UC-12)

---

### BACKLOG-13 — Loud degradation on schema-probe failure or audio loss

**As a** P1 solo developer, **I want** the daemon to announce loudly when narration quality drops — whether from a Claude Code JSONL schema change or an audio-device failure — **so that** I am never deceived into thinking I am getting intelligent content-aware narration when I am actually getting a degraded or silent experience.

**Acceptance:**
- When the JSONL structured parse fails (schema-probe detects a changed shape), the daemon: (a) speaks a "degraded mode" notice once, (b) continues narrating using flattened-text, and (c) shows a persistent menubar badge; it does NOT silently masquerade as normal intelligent operation.
- When the audio device becomes unavailable while an `AttentionEvent` is pending, the daemon raises a visible menubar badge rather than dropping the event silently.
- Recovering the audio device or restoring a valid JSONL schema clears the degraded badge after the next successful parse.
- A unit test drives a fixture with a deliberately malformed JSONL record and asserts the degraded-mode spoken notice and badge are triggered, and that the pipeline continues rather than panics.

(traces_fr: FR-23, FR-13, traces_uc: UC-13)

---

### BACKLOG-14 — One-time macOS accessibility grant for global hotkeys

**As a** P1 solo developer, **I want** the menubar app to guide me through granting the one-time macOS accessibility permission for system-wide hotkeys **so that** skip, pause, pull, and replay all work globally — even when my terminal is not the frontmost window.

**Acceptance:**
- On first launch, if the accessibility permission has not been granted, the menubar app surfaces a prompt or System Settings deep-link that directs the operator to grant the TCC accessibility grant.
- The daemon continues to run and narrate via the launchd socket even before the grant is given; only hotkey control degrades (narration is not blocked).
- After the grant is given, global hotkeys are active without a daemon restart.
- The menubar reflects the accessibility status (granted vs pending) so the operator knows why hotkeys are not yet working.

(traces_fr: FR-2, traces_uc: UC-1)

---

### BACKLOG-15 — Persist and apply voice, hotkey, and threshold settings

**As a** P1 solo developer, **I want** to tune voice persona, hotkey bindings, and chunker/trivial-turn thresholds via the menubar or a plain config file — and have them stick across restarts — **so that** I can dial in the narration experience to my workflow without touching source code.

**Acceptance:**
- Changing a setting (e.g., trivial-turn word-count threshold) via the menubar or by editing the config file takes effect for the next completed turn without a code change or full daemon rebuild.
- Settings persist to the config file on change and are read from it on daemon startup.
- Invalid or missing config values produce a logged warning and fall back to documented defaults rather than crashing the daemon.
- The config file format is plain JSON (or TOML) and is human-editable without a special tool.

(traces_fr: FR-3, traces_uc: UC-14)

---

### BACKLOG-16 — Menubar state display and VoiceOver-compatible UI

**As a** P1 solo developer, **I want** the menubar app to show current narration state, mode status, and any active degradation or failure badges — with VoiceOver-compatible labels — **so that** I can tell at a glance whether the daemon is alive and healthy, and assistive technology can read the same status.

**Acceptance:**
- The menubar item updates in real time to show at least: narrating / paused / idle / degraded states.
- A degraded-mode badge (triggered by schema-probe failure or audio loss) is visible and distinct from the normal idle state.
- All menu items carry VoiceOver-compatible accessibility labels (verified by enabling VoiceOver and navigating the menu).
- System font sizing is used throughout; no hard-coded pixel font sizes.

(traces_fr: FR-4, traces_uc: UC-15)

---

### BACKLOG-17 — Normalizer property test suite

**As a** P1 solo developer, **I want** the code-to-speakable normalizer to be covered by an automated property test suite **so that** regressions in the crown-jewel normalization logic surface immediately in CI rather than silently degrading narration quality.

**Acceptance:**
- A `proptest`-based property test suite on `pulse-pipeline` asserts, for all generated identifier-like inputs, that: (a) output never contains a raw underscore `_`, (b) output never contains scope-colons `::`, (c) output is always non-empty speakable text, and (d) applying the normalizer twice produces the same result as applying it once (idempotence).
- The suite runs as part of `cargo test` with no special flags.
- A deliberate regression (re-introducing raw underscore output) causes at least one property test to fail within a few seconds.

(traces_fr: FR-7, traces_uc: UC-3)

---

### BACKLOG-18 — Chunker golden test corpus (ReplayAdapter)

**As a** P1 solo developer, **I want** the content-aware chunker's tiering decisions to be pinned against a labeled corpus of recorded Claude Code turns **so that** I can refactor the chunker safely and know that a green test run still means genuinely intelligent narration.

**Acceptance:**
- The `pulse-source` crate ships a `ReplayAdapter` that feeds recorded JSONL transcript fixtures without a live agent session.
- The fixture corpus covers at minimum: a patch/diff turn, a command-output turn, an error-output turn, a planning/prose turn, a test-failure turn, a permission-prompt turn, a file-edit turn, and a terminal-log turn.
- Each fixture turn has segment-level expected `ChunkDecision` tier labels (read / announce / announce+pull / suppress) checked in alongside the fixture data.
- Golden tests in `pulse-pipeline` drive each fixture through the full chunker + normalizer and assert the expected tier labels; any label mismatch is a test failure.
- The suite runs in `cargo test` with no network or audio device.

(traces_fr: FR-6, FR-7, traces_uc: UC-3, UC-4, UC-5)

---

### BACKLOG-19 — Unix socket hook ingestion and hook fail-fast

**As a** P1 solo developer, **I want** Claude Code Stop/Notification hooks to deliver events to the resident daemon over a Unix domain socket, and to fail fast with a non-zero exit when no daemon is listening **so that** the hook never blocks my agent run even if the daemon is down.

**Acceptance:**
- A Stop hook script connects to the daemon Unix socket and delivers the JSONL transcript path; the daemon reads, parses, and ingests the turn.
- If no daemon is listening on the socket when the hook fires, the hook logs an error message and exits non-zero within a short timeout (< 2 s) rather than hanging.
- The hook does not buffer or retry indefinitely; one delivery attempt with a fast-fail is sufficient.
- An integration test drives the full path: `ReplayAdapter` → chunker → normalizer → fake `TtsProvider` (records utterances, no real audio) and asserts the correct `Utterance` stream is produced.

(traces_fr: FR-5, FR-24, traces_uc: UC-3, UC-1)

---

### BACKLOG-20 — Local structured logging and latency instrumentation

**As a** P1 solo developer, **I want** the daemon to write structured logs to `~/Library/Logs/PulseVoice/` covering hook receipt, transcript parse outcomes, chunk decisions, TTS first-audio latency, and barge-in events **so that** I can empirically validate the Kokoro-on-M1 latency target and debug degradation events from the log file alone, with no remote telemetry.

**Acceptance:**
- The daemon writes structured (key-value or JSON-lines) log entries to a rotating file at `~/Library/Logs/PulseVoice/` on all production paths.
- Log entries are present for: each hook-event receipt, each transcript parse outcome (including schema-probe degradations), each `ChunkDecision` per segment, TTS first-audio latency in milliseconds per turn, and each barge-in / skip event.
- No log data is sent to any remote endpoint; a network monitor confirms zero outbound connections from the daemon.
- The log file is human-readable without a special tool.

(traces_fr: FR-23, FR-13, traces_uc: UC-13, UC-15)

---

## Backlog conventions

- **IDs** are `BACKLOG-N`, assigned in continuous ascending order. Once assigned, an ID is never reused or renumbered.
- **Ledger slices** cite FR and UC IDs from the SRS and PRD ledger; no IDs are invented. `traces_fr` lists functional requirements; `traces_uc` lists use cases.
- **A single sprint slice may cover multiple stories.** Stories are ordered by user value, not by implementation dependency; the team (or, for a solo project, the author) may reorder within a sprint to match build sequencing.
- **Done stories** are moved to the bottom of the document with strikethrough formatting and a completion date appended, for example: ~~BACKLOG-1 — Content-aware turn narration (core loop)~~ (completed 2026-06-14).
- **No placeholder items.** Every `BACKLOG-N` entry represents a real, scoped piece of work with an observable acceptance criterion. Generic, aspirational, or out-of-scope items are not added.
- **Infrastructure work** is tracked in the backlog only when it has an explicit FR or NFR that requires it (e.g., BACKLOG-19 is grounded in FR-5 and FR-24).
- **Phase 2 items** (Codex CLI adapter, LLM summarize/re-voice preprocessors, cloud premium voices, secret-redaction, token-level streaming, voice control) are deliberately excluded from this backlog until the `SourceAdapter` and `TtsProvider` traits are proven in v1 and a new planning cycle begins.

# PulseVoice — Project Plan

**Document version:** 1.0
**Date:** 2026-05-29
**Project:** PulseVoice (pulse-narrator v1)

---

## Timeline

**Target weeks to MVP:** No hard deadline. Ship when the product is good enough for reliable daily use. The rough internal target is approximately 4–6 weeks of part-time solo work — this is a personal-pace rough estimate, not a commitment or a deadline. Progress is gated on quality of the attention-event path and the content-aware chunker, not on calendar date.

**Team size:** Solo (the author only).

The milestone sequence is:

1. Bootstrap and governance (Sprint 0) — governance docs, workspace tooling, build scaffold in place.
2. Attention-event path live end-to-end (Sprint 1) — the safety-critical path de-risked: hook fires, daemon ingests, attention event speaks; Kokoro warm-resident latency validated on M1.
3. Content-aware chunker and normalizer shippable (Sprint 2) — the crown-jewel logic locked behind golden tests; full turn narration working.
4. Playback controls and session management solid (Sprint 3) — skip, pause/resume, pull, replay, multi-session focus arbitration all working.
5. Polish, observability, and daily-use confidence (Sprint 4) — settings persistence, menubar state, logging, sleep/wake recovery, degraded-mode handling; product is ready for sustained daily use.

---

## Risks

The risks below are drawn from MASTER-SPEC.md Phase 2.2.2. All are technical; market risk is effectively zero for a personal-use tool.

### Risk 1 — Chunker quality (highest severity)

The content-aware chunker (role x size tiering + normalization pass) is the defining product claim. If the tiering decisions are wrong — if it drones verbatim code, swallows prose, or mislabels operational edits — the narration feels like a toy and the product fails its core promise (UC-3).

**Mitigation:** Build the ReplayAdapter golden-test corpus early (BACKLOG-18). Label every hard-case fixture (patches, command output, errors, plans, test failures, permission prompts) with expected ChunkDecision tier labels before writing the full chunker logic, so the acceptance bar is concrete and regression is caught automatically. Do not declare Sprint 2 done until the golden suite is green.

### Risk 2 — JSONL transcript contract instability

Claude Code's Stop/Notification hook output is an unversioned internal format that can change without notice. A schema change silently breaks the tap and, worse, could make the daemon appear to narrate normally while actually producing garbage (FR-23).

**Mitigation:** Implement a schema-version probe in the Claude Code adapter at the outset (BACKLOG-13, BACKLOG-19). When the structured parse fails, the daemon must degrade loudly — spoken notice once plus persistent menubar badge — so the operator knows they are getting flattened-text fallback, not intelligent content-aware narration. The probe is part of Sprint 1 acceptance criteria.

### Risk 3 — Kokoro on M1 first-audio latency

Local Kokoro TTS is a neural synthesizer; unlike a cloud streaming TTS, it synthesizes a clip per chunk without incremental streaming. The real first-audio latency on Mac Mini M1 is the load-bearing unknown. If the attention-event path (target: < ~1 s from hook fire to first spoken word) cannot be met by a warm-resident Kokoro, the "step away and trust being called back" promise is broken (FR-14).

**Mitigation:** Validate Kokoro latency empirically in Sprint 1 before building the rest of the pipeline on top of it. Keep Kokoro warm at daemon startup (never lazily loaded per utterance). Pre-synthesize the small set of approved attention fallback phrases so a slow Kokoro synthesis never delays a permission-prompt event.

### Risk 4 — macOS accessibility TCC grant friction

Global hotkeys require the one-time macOS accessibility (TCC) grant. An unsigned binary that is rebuilt can lose this grant, forcing a manual re-grant every time. If hotkeys break silently or frequently, the barge-in and skip experience degrades and the eyes-free loop is broken (FR-2).

**Mitigation:** Guide the operator through the TCC grant on first launch (BACKLOG-14). Narration continues to function even without the grant; only hotkey control degrades. Track TCC status in the menubar so the state is never ambiguous. For v1 the annoyance is accepted; revisit signing if it becomes painful enough to address.

### Risk 5 — Solo bandwidth

The full v1 scope — hook integration, JSONL parsing, schema probing, role classification, normalization, local neural TTS, streaming playback, global hotkeys, launchd lifecycle, menubar, and session multiplexing — is substantial for spare-time solo work. Any one area taking significantly longer than expected delays the whole.

**Mitigation:** The sprint structure is sequenced so the highest-risk, highest-value slices ship first (attention-event path in Sprint 1, chunker in Sprint 2). If bandwidth tightens, the scope within Sprint 3 and Sprint 4 can be reordered without compromising the core use case.

---

## Success metric

**Primary success criterion:** Behavioral proof across a week of real Claude Code sessions — narration mode stays ON, there is no reverting to staring at the screen, and not a single permission or attention prompt is missed when stepped away from the machine. The trust of being able to walk away and be called back is the single proof.

**Concrete acceptance bar:** At the end of the first full working week with the daemon running in daily use:
- Every completed Claude Code turn triggers content-aware narration (prose read, operational edits announced, inline code spoken in normalized speakable form, verbatim code never droned) without manual intervention (UC-3, FR-5, FR-6, FR-7, FR-8).
- Every permission or attention event from every active Claude Code session is spoken aloud regardless of narration mode state, regardless of which session is focused, and regardless of whether the Mac was sleeping when the event was raised (UC-5, FR-11, FR-12, FR-14, FR-15).
- No instance occurs where the operator has to look at the screen because a blocking attention event was missed.

---

## Budget

**Monthly cost cap (v1):** $0/month in the default path.

| Category | v1 Cost | Notes |
|---|---|---|
| Infrastructure / hosting | $0 | Self-hosted; runs on already-owned Mac Mini M1. No cloud hosting. |
| TTS API | $0 | Local Kokoro on-device; no cloud TTS in v1 default path. |
| LLM / summarize preprocessor | $0 | Summarize/re-voice is Phase 2; off in v1. |
| Tooling / CI runners | $0 | GitHub Actions free tier; macOS runners for cargo test. |
| Cloud premium voices (ElevenLabs / Cartesia) | Phase 2 opt-in only | Small budget only if cloud TTS is added later. |

No cloud API costs are expected in v1. The only one-time cost is model weight download for Kokoro (done once at setup, no recurring charge). If a Phase 2 opt-in cloud TTS provider is enabled, a small monthly API budget would apply at that time.

---

## Rollout plan

PulseVoice follows a direct personal-deployment rollout: there is no staged canary, no feature flags, and no external users to notify. A new version is released by running `git pull`, `cargo build --release`, and restarting the launchd daemon. The mode toggle serves as the built-in kill switch — if a build misbehaves, narration is silenced instantly and the operator falls back to reading the screen (the pre-tool status quo) until the issue is resolved. A last-good binary can optionally be kept on hand for immediate rollback by hand.

The daemon is distributed as a local build artifact (not a signed .app in v1); a GitHub Release with a tagged binary is the optional distribution form. The unsigned binary is acceptable for personal use; the known TCC re-grant annoyance on rebuild is noted and accepted for v1.

Observability in production relies on structured local logs written by the `tracing` crate to a rotating file at `~/Library/Logs/PulseVoice/` (BACKLOG-20). The daemon logs hook-event receipt, transcript parse outcomes including schema-probe degradations, chunk decisions per segment, TTS first-audio latency in milliseconds per turn, and barge-in events. There is no remote telemetry; the log file is human-readable without a special tool. The daemon is also its own alerting channel: it speaks failures aloud (degraded-mode spoken notice on schema-probe failure, pre-synthesized fallback on Kokoro timeout) and shows a persistent menubar badge for any degraded state. No email, Slack, or pager alerting is needed or intended. For latency specifically, the logged TTS first-audio latency values are the empirical instrument for validating that the attention-event target (< ~1 s from hook fire to first spoken word) is being met in real use (BACKLOG-20, FR-13).

---

## Sprint structure

### Sprint 0 — Bootstrap and governance (2026-05-29)

**Goal:** Workspace tooling and Rust crate scaffold in place; governance docs derived; first vertical slice ready for development.

Derivation timestamp: 2026-05-29.

Work in scope:
- Initialize the Rust workspace (`pulse-core`, `pulse-source`, `pulse-pipeline`, `pulse-tts`, `pulse-daemon`, `pulse-menubar` crates) with correct dependency boundaries and compile-time surface-agnostic enforcement.
- Author the launchd user agent plist for `pulse-daemon` auto-start at login (BACKLOG-3 groundwork).
- Establish `cargo fmt`, `cargo clippy -D warnings`, and `cargo test` as the local pre-merge gate.
- Set up the `~/Library/Logs/PulseVoice/` log directory and the rotating-file tracing subscriber skeleton (BACKLOG-20 groundwork).
- Derive MASTER-SPEC, PRD, SRS, BACKLOG, and this PROJECT_PLAN from the onboard session; docs are checked in and cross-linked.

Deliverable: `cargo build` succeeds on the full workspace; the daemon process starts, logs a startup event, and exits cleanly.

---

### Sprint 1 — Attention-event path and daemon lifecycle

**Goal:** The safety-critical path is live end-to-end: Claude Code hook fires, daemon ingests the event over the Unix socket, Kokoro is warm-resident, and an attention event speaks within the latency target. Kokoro on M1 first-audio latency validated empirically.

Backlog items:
- BACKLOG-2 — Always-on attention event preemption (FR-11, FR-12, FR-14; UC-5)
- BACKLOG-3 — Daemon lifecycle and warm-resident Kokoro startup (FR-1, FR-24; UC-1)
- BACKLOG-19 — Unix socket hook ingestion and hook fail-fast (FR-5, FR-24; UC-3, UC-1)
- BACKLOG-13 — Loud degradation on schema-probe failure or audio loss (FR-23, FR-13; UC-13)

This sprint de-risks the highest-stakes technical unknown (Kokoro latency on M1) and delivers the "never miss a permission prompt" invariant before any other narration logic is built. The schema-probe degraded-mode path ships in the same sprint because it protects the reliability of the attention path when the JSONL contract shifts.

---

### Sprint 2 — Content-aware chunker and full turn narration

**Goal:** The crown-jewel chunker and normalizer are working and locked behind golden and property tests. A completed Claude Code turn is narrated content-aware from end to end.

Backlog items:
- BACKLOG-1 — Content-aware turn narration (core loop) (FR-5, FR-6, FR-7, FR-8; UC-3)
- BACKLOG-18 — Chunker golden test corpus / ReplayAdapter (FR-6, FR-7; UC-3, UC-4, UC-5)
- BACKLOG-17 — Normalizer property test suite (FR-7; UC-3)
- BACKLOG-5 — Trivial-turn filter (FR-10; UC-4)

The ReplayAdapter fixture corpus and golden tests ship in this sprint alongside the chunker logic, not after. The acceptance bar for Sprint 2 is a green golden-test suite across all labeled hard-case fixture turns (patches, command output, errors, plans, test failures, permission prompts, file edits, terminal logs).

---

### Sprint 3 — Playback controls and session management

**Goal:** All interactive playback controls are working (skip, pause/resume, pull, replay, speed) and the daemon correctly arbitrates focus across multiple concurrent Claude Code sessions.

Backlog items:
- BACKLOG-4 — Narration mode toggle and kill switch (FR-9; UC-2)
- BACKLOG-6 — Barge-in / skip playback (FR-16; UC-6)
- BACKLOG-7 — Pause and resume playback (FR-17; UC-7)
- BACKLOG-8 — Pull full read-out of an announced block (FR-18, FR-22; UC-8, UC-11)
- BACKLOG-9 — Replay last narration (FR-19, FR-22; UC-9, UC-11)
- BACKLOG-10 — Multi-session focus arbitration (FR-21, FR-22, FR-11; UC-11)
- BACKLOG-11 — Adjust playback speed (FR-20; UC-10)

This sprint is the largest in scope. Given solo bandwidth, BACKLOG-4 (mode toggle) and BACKLOG-6 (skip) are the highest-priority items within the sprint; BACKLOG-11 (speed adjustment) is lowest and can slip to Sprint 4 if needed without breaking core daily use.

---

### Sprint 4 — Polish, observability, and daily-use confidence

**Goal:** Settings persist, menubar state is complete, logging is instrumented, sleep/wake recovery works, and the one-time accessibility grant flow is guided. Product is ready for sustained daily-use validation against the success metric.

Backlog items:
- BACKLOG-14 — One-time macOS accessibility grant for global hotkeys (FR-2; UC-1)
- BACKLOG-15 — Persist and apply voice, hotkey, and threshold settings (FR-3; UC-14)
- BACKLOG-16 — Menubar state display and VoiceOver-compatible UI (FR-4; UC-15)
- BACKLOG-12 — Sleep/wake attention event recovery (FR-15; UC-12)
- BACKLOG-20 — Local structured logging and latency instrumentation (FR-23, FR-13; UC-13, UC-15)

Sprint 4 concludes when the product has been running in real daily use for at least one full working week and the behavioral success metric (no missed attention events, narration mode stays ON, no reverting to the screen) has been observed in practice.

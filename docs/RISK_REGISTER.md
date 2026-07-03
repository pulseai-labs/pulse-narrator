# PulseVoice — Risk Register

> Derived from [MASTER-SPEC.md](../../pulse-narrator-ai/MASTER-SPEC.md).
> Seeds the project risk register from MASTER-SPEC Phase 2.2.2 (the onboarding risk record) plus
> technical signals visible in Phases 3–5. Risk IDs (`R-NNN`) are minted here; functional and
> non-functional requirements (`FR-*` / `NFR-*`) are cited from [SRS.md](./SRS.md) and backlog items
> (`BACKLOG-*`) from [BACKLOG.md](./BACKLOG.md). Use cases (`UC-*`) are defined in [PRD.md](./PRD.md).

**Document version:** 1.0
**Date:** 2026-07-03
**Project class:** Other (personal developer tool — local-first macOS daemon)

---

## Risk register

The seeded register below expands Phase 2.2.2's Top 3 (plus the noted resource risk) with the
durable technical, integration, and security risks the rest of MASTER-SPEC exposes. No market-risk
row is seeded: Phase 2.2.2 records market risk as ~ none (personal use). Owner and Status are left
as `TBD` / `open` for initial seeding.

| ID | Risk | Category | Likelihood | Impact | Mitigation | Owner | Status |
| --- | --- | --- | --- | --- | --- | --- | --- |
| R-001 | The content-aware chunker (role × size tiering + normalization — the v1 defining claim, Phase 2.2.2 Top 3 #1) fails to feel intelligent on real Claude Code turns, making the product feel like a toy and undercutting the only reason it exists. | tech | Medium | High | Iterate the chunker against a labeled golden corpus of real turns via the `ReplayAdapter` test seam (Phase 7.1 / 8.2), asserting per-segment tier labels on hard cases before merge. | TBD | open |
| R-002 | The Claude Code Stop/Notification hook → Unix-socket → JSONL-parse tap (Phase 2.2.2 Top 3 #2) silently drops or mis-parses structured events (paths, `tool_use`, line counts), so narration degrades without the operator noticing. | tech | Medium | High | Make degradation loud, never silent: announce "degraded mode" + persistent menubar badge on parse failure, fail-fast the hook when no daemon is listening, and dedupe attention events by id (NFR-22). | TBD | open |
| R-003 | Kokoro on the Mac Mini M1 (a local neural, effectively non-streaming synthesizer) cannot synthesize first audio fast enough for the attention and `TurnComplete` latency targets, and/or the playback queue does not cancel cleanly at segment boundaries (Phase 2.2.2 Top 3 #3, Phase 5.2.1 / 5.3). | tech | High | High | Validate Kokoro first-audio latency empirically as the first vertical slice, keep Kokoro warm-resident, sub-chunk read segments to ~sentence units, and pre-synthesize a small set of attention fallback phrases (NFR-2, NFR-5). | TBD | open |
| R-004 | Solo spare-time bandwidth is insufficient to carry the full v1 surface (hook integration, transcript parsing, schema probing, role classification, normalization, local neural TTS, streaming playback, global hotkeys, launchd lifecycle, menubar, session multiplexing, golden + property tests), starving the crown-jewel chunker of iteration time (Phase 2.1, Phase 2.2.2). | resource | Medium | Medium | Sequence the roadmap so the attention-event path is the first vertical slice (de-risks early) and protect chunker-iteration time explicitly; the mode toggle is a built-in kill switch if a build is rough. | TBD | open |
| R-005 | The Claude Code JSONL transcript is an unversioned third-party internal format that ships changes often (Phase 5.2.2); a Claude Code update shifts the structured shape and breaks the content-aware parse. | integration | High | Medium | Run a schema-version probe in the `claude-code` adapter on each parse; on mismatch, fall back to flattened-text narration announced loudly with a persistent menubar badge and a log line (FR-23, NFR-22). | TBD | open |
| R-006 | Secrets that appear in agent output (env vars, tokens in a Bash command, API keys in a diff) are spoken aloud because secret-redaction is deliberately deferred for v1 under the private-audio-environment assumption (Phase 4.1, Close-audit C2); the assumption breaks in any shared/public space (shoulder-surfing, smart speakers, meeting mics). | security | Low | High | Keep the private-audio-environment assumption explicit in docs and revisit a minimal secret-pattern suppressor in Phase 2 — unconditionally before any shared-space use (relates to NFR-16). | TBD | open |
| R-007 | System-wide global hotkeys via Rust crates (`tray-icon` / `muda` / `global-hotkey`) hit macOS accessibility / `.app`-bundling friction and become unreliable, blocking the primary interaction surface (Phase 5.2.1, Arch-review C7). | integration | Medium | Medium | Treat the Swift-fallback menubar shell as an explicit decision trigger: if Rust global-hotkey reliability blocks v1, switch the menubar shell to a thin Swift app over the Rust daemon via IPC, keeping all systems work in Rust. | TBD | open |
| R-008 | Queued attention events (permission prompts) are lost or never fire after a macOS sleep/wake cycle, breaking the "always spoken, never missed" safety-critical guarantee (Phase 5.3, Close-audit C7, FR-15). | tech | Medium | High | On wake, fire queued attention events with a "while you were away" marker and dedupe by event id; if the audio device is unavailable, fail loud to the menubar badge rather than dropping the event. | TBD | open |
| R-009 | Because v1 ships an unsigned/rebuilt binary, macOS TCC keys the accessibility grant on binary identity and the global-hotkey grant is lost on each rebuild, repeatedly breaking the primary interaction surface (Phase 10.1). | tech | High | Low | Accept the re-grant as a v1 annoyance and document it; revisit codesigning / notarization only if it slows the inner loop enough to become painful. | TBD | open |
| R-010 | The labeled chunker golden corpus (Phase 7.1 / 8.2 — the eval bar) does not cover the hard cases (patches, command output, errors, plans, test failures, permission prompts, file edits, terminal logs), so green golden tests give false confidence in the crown jewel. | tech | Medium | High | Build the labeled corpus to explicitly include each hard-case class before declaring the chunker done, and assert tier labels per segment against that corpus (NFR-19, BACKLOG-18). | TBD | open |

---

## Phase 2.2.2 source text

Reproduced verbatim from MASTER-SPEC Phase 2.2.2 (Constraints). This is the historical onboarding
risk record; the table above operationalizes it. Do not edit this block — amend risks in the table.

> **Top 3 risks:**
> 1. Chunker quality (tech, highest): role x size tiering + normalization is the crown jewel; if it does not feel intelligent the product feels like a toy. 2. Transcript-tap reliability (tech): depends on Claude Code Stop/Notification hooks + parsing the structured JSONL event sequence (paths, tool_use, line counts); silent degradation if the transcript shape shifts or events are missed. 3. Local TTS latency + barge-in (tech): Kokoro on M1 must stream fast enough and the playback queue must cancel cleanly at segment boundaries; plus the one-time macOS accessibility grant for global hotkeys. Market risk ~ none (personal use); resource risk = solo bandwidth.

---

## Conventions

- **IDs are `R-NNN`**, zero-padded, minted in ascending order, and **never reused**. A risk that is
  closed or accepted keeps its ID; a risk that supersedes another gets a fresh `R-NNN` and references
  the one it replaces. Never renumber existing rows.
- **Status** is exactly one of: `open · mitigated · accepted · closed`.
  - `open` — identified; no mitigation in place yet.
  - `mitigated` — an actionable mitigation is implemented and tracked (the risk still exists; its
    likelihood and/or impact are reduced).
  - `accepted` — consciously retained (e.g., R-006's v1 private-audio assumption) with the decision
    recorded, typically in an ADR.
  - `closed` — the underlying cause is gone (e.g., a dependency removed, an assumption retired);
    the ID is retired with it.
- **Hand-edit instructions**:
  - To add a risk, append the next free `R-NNN` (do not renumber earlier rows) and fill every column;
    state the risk as a concrete, project-specific sentence and reference the phase / FR / NFR that
    exposes it.
  - As a risk evolves, update Likelihood, Impact, and Status in place; when it retires, set Status
    to `closed` or `accepted` and leave the row for history.
  - Cross-link any mitigation to the ADR (in [`docs/adr/`](./adr/)) or backlog item that records the
    decision.
  - The verbatim Phase 2.2.2 block above is the historical onboarding record — amend risks in the
    table, never by rewriting that block.

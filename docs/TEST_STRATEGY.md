# TEST_STRATEGY

> **Derived from [MASTER-SPEC.md](../MASTER-SPEC.md)** · v1.0 · 2026-07-03

PulseVoice is a local-first macOS daemon: it ingests agent turns, chunks them by role/significance, normalizes identifiers into speakable text, and narrates via Kokoro TTS with sub-sentence barge-in. The crown jewels are deterministic, silent-failure pure logic (`pulse-core`, `pulse-pipeline` = chunker + normalizer); everything that touches audio, the OS, or a live agent is validated by hand. This strategy floors the logic that matters and refuses to pretend that testing audio/UI glue earns its keep.

## Coverage floor

Targeted floor, not blanket: **~80% on the pure-logic crates** (`pulse-core`, `pulse-pipeline` = chunker + normalizer, the crown jewel) where bugs are silent and behavior is deterministic. Lower/pragmatic on `pulse-daemon`, `pulse-tts`, `pulse-menubar` (I/O, audio, OS integration — validated manually). A blanket repo-wide percentage would force low-value tests on audio/UI glue; floor the logic that matters.

This numeric bar is encoded as acceptance criterion **NFR-18** (Coverage floor ≥ ~80% on `pulse-core`, `pulse-pipeline`). Coverage is measured per-crate via `cargo-tarpaulin`/`cargo-llvm-cov` over `pulse-core` and `pulse-pipeline` only; the daemon/TTS/menubar crates are excluded from the numeric gate. A merge that drops either crown-jewel crate below ~80% fails the pre-merge gate (see [Pre-merge gates](#pre-merge-gates)).

## Test types in scope

**Unit** — the deterministic leaves: the normalizer (`camelCase`→words, `auth.rs`→"auth, rust file", operator mapping), and the duration-threshold classifier. These are the cheapest, fastest, highest-signal tests in the project; nearly every behavior assertion in FR-6/FR-7 lives here. Grounded in `pulse-pipeline::normalizer` (Phase 3/5).

**Golden / snapshot** — chunker tiering decisions over the recorded-JSONL fixture corpus via `ReplayAdapter` (the crown-jewel regression net). This is where [Close-audit C6] lands: the chunker needs an explicit quality bar, not just "role × size". The `ReplayAdapter` fixture corpus doubles as the **labeled chunker eval set** — each recorded turn is annotated with the expected tier per segment, and the golden tests assert `ChunkDecision` per segment against those labels. The corpus must include the hard cases (patches, command output, errors, plans, test failures, permission prompts, file edits, terminal logs). Full classification rule catalog authored during the build against this corpus. **Verified by NFR-19** (Chunker golden tier-label bar: ReplayAdapter labeled corpus asserts `ChunkDecision` per segment); acceptance = every labeled segment in the corpus matches the chunker's emitted tier.

**Property** — normalization invariants via `proptest`, **INCLUDED IN v1** for the normalizer. **Verifies NFR-10** with these acceptance thresholds, all asserted over generated inputs: never emits raw underscore or scope-colons, output is always non-empty speakable text, normalization is idempotent. Also exercises **NFR-12** (no-panic resilience on bad input) — property cases feed hostile/malformed strings and assert the normalizer returns rather than panics. Tracked as **BACKLOG-17**.

**Integration** — `ReplayAdapter` → chunker → normalizer → a fake `TtsProvider` that records utterances (no real audio), asserting the full `Utterance` stream + barge-in cancellation. This is the tier where **NFR-3** (barge-in responsiveness — skip→silence near-instant at sub-sentence boundary) and **NFR-9** (clean barge-in state invariant — no orphaned half-played state) are verified: the fake provider records the exact cancel/cull behavior on skip without ever synthesizing audio. Hook-ingestion integration is covered separately as **BACKLOG-19**.

**Not e2e / not contract** — real audio + live agent validated manually (see [What we deliberately don't test](#what-we-deliberately-dont-test)).

Measurable-bar NFR → test-type map (acceptance thresholds stated):

| NFR | Bar | Test type | Acceptance threshold |
|---|---|---|---|
| NFR-10 | Normalizer property invariants | Property (`proptest`) | no raw `_`/scope-colons, non-empty, idempotent, on all generated inputs |
| NFR-19 | Chunker golden tier-label | Golden/snapshot | every labeled segment in the corpus matches emitted `ChunkDecision` tier |
| NFR-3 | Barge-in skip→silence | Integration (fake `TtsProvider`) | in-flight `Utterance` cancelled at sub-sentence boundary |
| NFR-9 | Clean barge-in state | Integration (fake `TtsProvider`) | no orphaned half-played state after cancel |
| NFR-12 | No-panic on bad input | Property (`proptest`) | returns/doesn't panic on hostile input |
| NFR-18 | ~80% coverage | Coverage gate | `pulse-core` & `pulse-pipeline` ≥ ~80% |
| NFR-2 / NFR-4 | First-audio / steady-state latency | **Empirical, not CI** | logged timing, validated by hand (**BACKLOG-20**) |

## Pyramid

Three tiers — this project is small and mostly deterministic, so the pyramid is fat at the base and deliberately thin at the top. No fourth tier: Phase 9.3 is null (Kokoro is a neural TTS model, **not** an LLM doing content generation), so there is **no model-eval tier** and `EVALS_PLAN.md` is gated out for this project — do not add one.

| Tier | Share target | Runtime budget | What it covers |
|---|---|---|---|
| **Unit** | ~70% | < 5s wall-clock | normalizer leaves, duration-threshold classifier, operator/symbol mapping. Pure functions, no I/O. |
| **Integration** | ~25% | < 30s wall-clock | `ReplayAdapter` → chunker → normalizer → fake `TtsProvider`; barge-in cancel/cull (NFR-3, NFR-9); hook-ingestion (**BACKLOG-19**). Golden tier-label corpus (NFR-19) runs here. |
| **Property** | ~5% | < 15s wall-clock | normalizer invariants via `proptest` (NFR-10), no-panic on bad input (NFR-12). |
| **E2E** | 0% (manual) | n/a | real Kokoro audio + live agent — validated by a human at a desk, never in CI. |

Targets are grounded in the Rust stack (Phase 5.2.1): `cargo test` is single-process, parallelizes cheaply, and the pure-logic crates have no build of audio/OS deps, so the unit tier is essentially free and the budget collapses onto the integration tier. Percentage targets are guidelines for where new tests *belong*, not a CI-enforced ratio — the only CI-enforced number is the ~80% coverage floor on the two crown-jewel crates (NFR-18).

## Pre-merge gates

Reproduced verbatim from Phase 9.2.1 (enforced once CI is added per Phase 8; run locally before merge until then):

> Pre-merge gates (enforced once CI is added per Phase 8; run locally before merge until then): `cargo fmt --check` clean, `cargo clippy -D warnings` clean, `cargo test` green INCLUDING the `ReplayAdapter` golden tests and the normalizer `proptest` suite, and coverage not below the ~80% floor on the pure-logic crates (`pulse-core`, `pulse-pipeline`). Audio/TTS paths are validated manually (not gated in CI).

Each gate annotated with the NFR it enforces:

- **`cargo fmt --check` clean** — enforces **NFR-20** (gates clean) on style.
- **`cargo clippy -D warnings` clean** — enforces **NFR-20**; treats lints as errors, which is the cheap proxy for **NFR-12** (no-panic resilience) on the obvious foot-guns.
- **`cargo test` green INCLUDING `ReplayAdapter` golden tests and normalizer `proptest` suite** — the golden tier-label asserts enforce **NFR-19**; the `proptest` suite enforces **NFR-10** (invariants) and **NFR-12** (no-panic); the integration cases in the same run enforce **NFR-3** and **NFR-9** (barge-in); the whole bundle is what **NFR-20** names as "test green".
- **coverage not below the ~80% floor on `pulse-core`, `pulse-pipeline`** — enforces **NFR-18** (and is the coverage clause of **NFR-20**).
- **Audio/TTS paths validated manually (not gated in CI)** — by definition not a gate; their measurable bars (**NFR-2** first-audio sub-second, **NFR-4** steady-state faster than real-time) are checked via logged timing instrumentation (**BACKLOG-20**), reviewed by a human, not asserted in CI.

## Framework

Canonical framework: **`cargo test`** — the Rust-native test runner (Phase 5.2.1 stack: Rust core for chunker, normalizer, playback queue, adapters, daemon; menubar shell Rust-first via `tray-icon`/`muda`/`global-hotkey`). Property tests use **`proptest`** as a dev-dependency in `pulse-pipeline`. Coverage is collected with **`cargo-llvm-cov`** (or `cargo-tarpaulin`) scoped to `--package pulse-core --package pulse-pipeline`.

**Canonical command:**

```
cargo test
```

This single command is what the vertical-slice `auto:` demo criteria run and what the scaffold-dev `/impl-check` verification gate runs. It must exit green. Coverage is a separate invocation (`cargo llvm-cov --package pulse-core --package pulse-pipeline`) consulted for the floor, not part of the default `cargo test` run.

There is **no LLM-eval framework** to name. Phase 9.3 is null for this project: Kokoro TTS is a neural TTS model evaluated by ear, not an LLM whose outputs need scored evals, so no eval harness (`EVALS_PLAN.md`, promptfoo, etc.) applies.

## What we deliberately don't test

So future contributors don't re-add coverage that was consciously excluded:

1. **Real Kokoro audio output / synthesis quality.** Synthesized speech is judged auditorily by a human at a desk; a unit test cannot hear intelligibility, prosody, or mispronunciation. NFR-2 (first-audio latency) and NFR-4 (faster than real-time) are checked via logged timing (**BACKLOG-20**), not asserted in CI. A test that "plays audio and checks it played" earns nothing.
2. **Live-agent / end-to-end hook ingestion with a real terminal session.** The boundary between PulseVoice and a real running agent is environment-dependent and flaky; the integration test (`ReplayAdapter`-fed, **BACKLOG-19**) covers the ingestion *logic* against recorded fixtures, and the real-agent path is exercised manually before release. Don't add a test that shells out to a live agent.
3. **macOS OS integration: menubar, global-hotkey, tray-icon.** These are OS-surface glue (`tray-icon`/`muda`/`global-hotkey` crates, with the Swift fallback trigger if hotkey reliability blocks v1). Their failures are environmental and visual; they're validated by hand on the target macOS version. A headless test of "did the menu appear" is theater.
4. **Non-macOS / cross-platform targets.** v1 is macOS-only. Tests asserting Linux/Windows behavior are out of scope until a port is on the roadmap; do not gate on them.
5. **Latency/performance regressions under CI.** NFR-1 (attention-event < ~1s), NFR-2, and NFR-4 are real bars but are validated empirically via logged timing reviewed by a human, not by a flaky wall-clock assertion in CI. CI runtime budgets (see [Pyramid](#pyramid)) exist to keep the suite fast, not to police latency.

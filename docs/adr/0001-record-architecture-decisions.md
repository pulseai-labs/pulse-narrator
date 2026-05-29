# ADR 0001 — Record Architecture Decisions

## Status

Accepted

## Context

PulseVoice is a macOS desktop daemon and menubar app (project class: Other) that taps
Claude Code's structured JSONL transcript output and narrates it aloud through a
content-aware pipeline. The project is small and solo-staffed, but it carries a set
of early architectural choices that are already constraining the design space and will
continue to do so as the codebase grows.

Phase 4 of the master specification fixed several decisions that are not obvious from
the code alone and that would be expensive to reverse later:

- Auth model: none. The daemon binds to localhost over a Unix domain socket with no
  authentication layer, reflecting a deliberate single-user, local-only stance. Any
  future multi-user or networked extension would require revisiting this foundation.
- Tenancy: single-user, single-tenant. One operator, one machine. Session
  multiplexing (multiple concurrent Claude Code terminals) is handled inside the
  daemon but the security model is not multi-user.
- Local-only / no-cloud default. Local Kokoro TTS is the security-driven default, not
  merely a cost optimization. Cloud TTS paths exist only behind an opt-in trait seam.
  This shapes every future decision about data-in-flight.
- Secret-redaction: explicitly deferred to Phase 2 under the assumption of a
  private/trusted audio environment. This is a recorded aware-but-deferred item, not
  an oversight. The assumption must be revisited before any shared or public-space use.
- Trusted-LAN-only homelab stance. If daemon-to-node traffic ever crosses a LAN (a
  two-node Mac Mini / ROG laptop setup), it is trusted-LAN-only and must never be
  exposed to a public network.

Phase 5 added further constraints: the architecture shape is a modular-monolith Rust
daemon (a single process, not microservices); the primary language is Rust throughout;
and the primary data store is none (file-only TOML/JSON config, no database). The
pipeline is built around two explicit trait seams (SourceAdapter and TtsProvider) that
enforce surface-agnosticism downstream. The Claude Code JSONL transcript is an
unversioned third-party format and is therefore a recognised durability risk, mitigated
by a schema-version probe with loud degradation to flattened-text narration.

Without a record of these decisions, a future contributor (or the author returning
after a break) would have no way to distinguish intentional constraints from accidental
ones, and could unknowingly undo tradeoffs that were made carefully.

## Decision

The project will use Architecture Decision Records (ADRs) as described by Michael
Nygard (https://cognitect.com/blog/2011/11/15/documenting-architecture-decisions).

ADRs are filed under `docs/adr/` in the canonical repository. Each file follows the
naming convention `NNNN-<slug>.md` where NNNN is a zero-padded four-digit sequence
number starting at 0001. Sequence numbers are never reused; a superseded ADR is marked
as such and a new ADR is opened for the replacement decision.

Each ADR contains exactly four required sections, in this order:

1. Status — one of: Proposed, Accepted, Deprecated, Superseded by NNNN
2. Context — the forces and constraints that made the decision necessary
3. Decision — what was decided and why
4. Consequences — what becomes easier or harder as a result

The `scaffold-dev` plugin supports fast ADR authoring and can stub new ADR files in
this format.

## Consequences

Positive consequences:

- Every significant architectural constraint is traceable to an explicit decision
  record with its rationale, so future contributors understand why the codebase looks
  the way it does rather than having to reverse-engineer intent from code.
- Deferred decisions (such as secret-redaction and the Swift-fallback trigger for the
  menubar shell) are documented with their assumptions explicitly stated, making it
  safe to defer without losing the context needed to revisit them.
- The discipline of writing a short Context + Decision + Consequences entry provides a
  forcing function to articulate tradeoffs at decision time, when the reasoning is
  fresh, rather than reconstructing it later.
- The ADR index provides a natural checklist of constraints to review when evaluating
  any proposed change.

Negative constraints:

- ADRs capture decisions that constrain future work, not every implementation detail.
  Low-level choices (function signatures, internal module layout, error type naming)
  belong in code comments and doc-strings, not ADRs. The distinction requires
  judgment; erring too fine-grained dilutes the record.

Project-specific consequences visible from the master specification:

- The two trait seams (SourceAdapter and TtsProvider) are architectural decisions
  with significant downstream implications: they enforce surface-agnosticism, pin the
  async stream contract, and determine how future adapters (Codex CLI, ElevenLabs)
  plug in. Each seam warrants its own ADR as the trait shape stabilises and is
  validated against real adapters.
- The Claude Code JSONL contract durability risk (an unversioned third-party format
  that ships changes often, mitigated by a schema-version probe and loud degraded-mode
  announcement) is a standing architectural concern that will warrant an ADR capturing
  the probe strategy, the degradation behaviour, and the conditions under which the
  mitigation is no longer sufficient.
- The Swift-fallback trigger for the menubar shell (switch from Rust-first tray-icon
  to a thin Swift app if global-hotkey / accessibility / .app-bundling friction blocks
  v1) is a conditional decision with a defined trigger condition; it should be recorded
  in its own ADR when the trigger is evaluated so the outcome is not lost.

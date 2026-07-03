# PulseVoice — Cutover Plan

> Derived from [MASTER-SPEC.md](../../pulse-narrator-ai/MASTER-SPEC.md).
> Honest framing: PulseVoice is a single-user, local-first macOS daemon + menubar app. There is no staging environment, no operations team, no external users to notify. This document is the operator's personal upgrade/install runbook — every enterprise-flavored section heading below is restated for that reality.

**Document version:** 1.0
**Date:** 2026-07-03
**Project class:** Other (personal developer tool — local-first macOS daemon)
**Operator:** solo, Mac Mini M1

---

## Environments

There is **one environment**: the operator's Mac. Per the spec (phase_8.2.2), PulseVoice is *dev-only* — there is no staging and no prod, because the program runs on the operator's machine. The concept of "environments" therefore reduces to **build profiles**, not deployed targets:

- **Debug build** (`cargo build` / `cargo run -p pulse-daemon`): the working profile during development. Runs out of `target/debug/`, emits `tracing` output to stderr in addition to the rotating log file, and is what the operator edits against.
- **Release build** (`cargo build --release`): the profile used for the running daemon that stays resident across the day. Optimized, runs out of `target/release/`, and is what a cutover produces.

There is no separate deploy target to promote *to*. A "new version" is a new release binary on the same Mac it was built on. A tagged GitHub Release may additionally publish the built artifact for archival / re-install on a fresh checkout, but v1 distribution is an unsigned local build (phase_8.3.1) — no notarized signed `.app` is required.

---

## Hosting

Self-hosted / none (phase_8.3.1). PulseVoice is a local macOS daemon (`pulse-daemon`) plus a menubar app (`pulse-menubar`), running as a launchd user agent on the operator's Mac Mini M1. There is **no cloud hosting**.

- The daemon binds to localhost / a Unix domain socket (NFR-14). The only network surface in the system is the Phase-2 *opt-in* cloud TTS / LLM, which is not hosting and is not part of the v1 default path (NFR-13).
- Distribution = a GitHub Release with a built artifact, or simply a locally-built binary. v1 ships unsigned; `cargo run` / an unsigned binary is acceptable.
- Known gotcha, recorded in phase_8.3.1: an unsigned/rebuilt binary can lose its macOS accessibility (TCC) grant on each rebuild, because TCC keys on binary identity. This is an accepted v1 annoyance; revisit signing only if it becomes painful. A post-cutover re-grant may be required (see Post-cutover step 4).

---

## Rollout strategy

Direct (phase_10.1.1). A new version is:

```
git pull && cargo build --release && restart the launchd agent
```

No staged rollout, no canary, no feature flags — none of those make sense for a single-user app. Two structural mitigations cover the risks that staged rollout would otherwise address:

- **Mode toggle as kill switch (FR-9).** If a freshly-built daemon misbehaves, the operator flips narration mode OFF. Turn narration is suppressed, but the always-on attention path (FR-11, NFR-8) still speaks across all sessions, so the safety-critical behavior is not lost while debugging.
- **Last-good binary kept by hand.** The pre-cutover snapshot (step 2) preserves the previously-running release binary, so a rollback is a single `cp` + launchd restart (see Rollback).

Support posture (phase_10.3): solo, best-effort, no SLA. If it breaks, narration mode goes off and the operator reads the screen — the status quo — until it is fixed. The launchd auto-restart handles transient crashes (FR-1, BACKLOG-3), and the no-panic discipline (NFR-12) keeps the daemon alive through bad input.

---

## Cutover script

This is the operator's personal upgrade procedure. It assumes the working tree is at `/Users/draco/projects/pulse-narrator/pulse-narrator`, the daemon runs as a launchd user agent, and the release binary lives at `target/release/pulse-daemon`.

### Pre-cutover

Run these in order. Each step states its expected outcome.

1. **Confirm the daemon is the thing you think it is.** Note the currently-running commit and binary path: `git rev-parse --short HEAD` and `launchctl list | grep pulse` (or whichever label the agent is registered under).
   - *Expected outcome:* a known "current" commit hash and a visible launchd entry; this is the baseline you are rolling *from*.
2. **Snapshot the last-good binary and current config.** Copy the running release binary and the config file aside so a rollback target exists:
   ```
   cp target/release/pulse-daemon target/release/pulse-daemon.lastgood
   cp config.json config.json.lastgood          # adjust path to the real config file
   ```
   (If `.lastgood` already exists, overwrite it only after confirming the currently-running binary is healthy — otherwise keep the older known-good copy.)
   - *Expected outcome:* `pulse-daemon.lastgood` and `config.json.lastgood` exist on disk, matching the running version. This is the rollback artifact used in *Rollback*.
3. **Confirm clean working tree before pulling.** `git status` should show no uncommitted local edits. If there are local tweaks (config experiments, a patched hook script), stash or commit them so `git pull` does not conflict.
   - *Expected outcome:* `git status` is clean (or stashed); `git pull` will fast-forward without merge conflicts.
4. **Verify the kill switch is reachable.** Confirm the menubar app is responsive and the mode-toggle hotkey works *before* you disturb anything — this is the fallback if the new build misbehaves mid-cutover (FR-9).
   - *Expected outcome:* toggling mode OFF changes the menubar state; you have a working manual fallback.
5. **Toggle narration mode OFF.** Suppress turn narration so the cutover does not interrupt an in-flight turn and so attention-event behavior during the rebuild is predictable.
   - *Expected outcome:* menubar shows narration paused; no new turn narration begins.
6. **Fetch the new source.** `git pull` (or `git fetch && git checkout <tag>` for a tagged release).
   - *Expected outcome:* working tree at the target commit; `git log -1` shows the expected new hash.
7. **Build the new release binary.** `cargo build --release -p pulse-daemon` (and `-p pulse-menubar` if shipped separately). The first build after dependency changes is the long pole (phase_8.1.1).
   - *Expected outcome:* `cargo build` exits 0; `target/release/pulse-daemon` has a fresh mtime.

### During cutover

The live upgrade. Each step is one action with one observable outcome.

1. **Run the test suite against the new build.** Before swapping the running daemon, run `cargo test` — this MUST include the ReplayAdapter golden tests (NFR-19) and the normalizer proptest suite (NFR-10, NFR-20). A red test suite is a no-go; abort here and stay on the last-good binary.
   - *Expected outcome:* `cargo test` exits green; all chunker golden-tier assertions and normalizer property tests pass.
2. **Stop the running daemon.** Unload the launchd agent so the socket is free and no in-flight narration is cut mid-clip:
   ```
   launchctl bootout gui/$(id -u) ~/Library/LaunchAgents/com.pulsevoice.daemon.plist
   ```
   (Use `launchctl unload` on older macOS if `bootout` is unavailable.)
   - *Expected outcome:* `launchctl list | grep pulse` returns nothing; the menubar app shows the daemon as not-running / disconnected; the Unix socket is free.
3. **Confirm the new binary is in place.** The release build from Pre-cutover step 7 already wrote `target/release/pulse-daemon`. If the launchd plist points elsewhere (e.g. an install path under `~/.local/bin` or `~/Applications`), copy the freshly-built binary there now.
   - *Expected outcome:* the path the launchd plist's `ProgramArguments` points at is the new binary; `ls -l` shows the expected mtime.
4. **Restart the daemon via launchd.** Load the agent again, which also re-applies the auto-start-on-login and restart-on-crash policy (FR-1, BACKLOG-3):
   ```
   launchctl bootstrap gui/$(id -u) ~/Library/LaunchAgents/com.pulsevoice.daemon.plist
   ```
   - *Expected outcome:* `launchctl list | grep pulse` shows the agent loaded with a PID; the daemon process is running.
5. **Wait for the ready/idle state.** The daemon warms Kokoro resident at startup (NFR-5) and reports ready/idle once the pipeline is live (FR-1). Give it a few seconds rather than driving traffic immediately.
   - *Expected outcome:* the menubar app reconnects and shows the daemon in a ready / idle state (no degraded badge yet — see Post-cutover).

### Post-cutover

Verification and self sign-off. Each step is a concrete check; the sequence ends with an explicit go/no-go.

1. **Menubar health signal.** Confirm the menubar shows ready / idle with **no degraded badge** and **no audio-failure badge** (FR-4, NFR-22). A persistent badge here is the rollback trigger.
   - *Pass:* menubar green, no badge.
2. **Observability check — local logs.** Tail the rotating log at `~/Library/Logs/PulseVoice/` (NFR-17, BACKLOG-20) and confirm: the daemon logged startup, Kokoro warmup completed, the schema-version probe ran clean (no degradation line), and there are no panic traces (NFR-12).
   - *Pass:* startup + warmup lines present; no `degraded` line; no panic/error stack.
3. **Smoke test — automated.** Re-confirm on the installed path what the build already proved: `cargo test` green, specifically the ReplayAdapter golden corpus (NFR-19) and the normalizer proptest (NFR-10). This is the same gate as During-cutover step 1; re-running it here catches any environment drift introduced by the install copy.
   - *Pass:* `cargo test` green.
4. **Accessibility (TCC) re-grant check.** Because the release binary may have changed identity (phase_8.3.1 gotcha), confirm the global hotkeys still work — toggle mode, hit skip, hit replay-last. If macOS has revoked the grant, re-grant it in System Settings → Privacy & Security → Accessibility (FR-2).
   - *Pass:* hotkeys fire; or, if re-grant was needed, it is now done and hotkeys fire.
5. **End-to-end smoke — narrate one real turn (UC-3).** With narration mode ON, run one real Claude Code turn in a terminal and listen. Confirm: prose is read, operational edits are announced, inline code is normalized (not read verbatim), and the turn completes narration in source order. This is the load-bearing proof — if this is missing, the product fails (UC-3).
   - *Pass:* one turn narrated content-aware, audibly and correctly.
6. **Attention-path spot check (UC-5).** Trigger or await one permission prompt from a Claude Code session and confirm the attention phrase speaks (pre-synthesized fallback acceptable per FR-14) and preempts any in-progress turn narration.
   - *Pass:* attention event speaks and preempts.
7. **Go / no-go sign-off (self).** The cutover is GO if and only if all of the above pass. Record the verdict in the local changelog (see Communication):
   - **GO:** "daemon live, menubar green, one turn narrated (commit `<new-hash>`)."
   - **NO-GO:** any step failed → execute *Rollback*.

### Rollback

**Trigger condition.** Roll back if **any** of the following holds after the cutover (grounded in NFR-22 — degradation is never silent — and the menubar badge):

- the daemon will not start, or crashes repeatedly despite launchd restart (FR-1 / NFR-12 violated in practice);
- the menubar shows a **persistent** degraded or audio-failure badge that is not cleared by a restart (FR-4, FR-23, NFR-22);
- narration is broken or inaudible after one real turn — i.e. Post-cutover step 5 fails (UC-3 not met);
- the schema probe is in flattened-text degradation when it should not be (FR-23), and a restart does not clear it.

A single transient glitch (one missed clip, a momentary badge that clears) is **not** a rollback trigger — flip mode OFF (FR-9, the kill switch) and investigate first.

**Revert steps, in order:**

1. **Toggle narration mode OFF.** Suppress turn narration immediately so a broken build cannot keep talking (FR-9 kill switch). Attention events still speak per NFR-8 while you work.
   - *Expected outcome:* menubar shows narration paused.
2. **Stop the new daemon.** `launchctl bootout gui/$(id -u) ~/Library/LaunchAgents/com.pulsevoice.daemon.plist`.
   - *Expected outcome:* launchd entry gone; process stopped; socket free.
3. **Restore the last-good binary and config** saved in Pre-cutover step 2:
   ```
   cp target/release/pulse-daemon.lastgood target/release/pulse-daemon
   cp config.json.lastgood config.json            # only if config also regressed
   ```
   If the new build's commit is the problem rather than the binary, also `git checkout <previous-good-hash>` so the source tree matches the binary you just restored.
   - *Expected outcome:* `target/release/pulse-daemon` is byte-identical to the last-known-good build; source tree at the prior commit.
4. **Restart the daemon via launchd.** `launchctl bootstrap gui/$(id -u) ~/Library/LaunchAgents/com.pulsevoice.daemon.plist`.
   - *Expected outcome:* daemon loaded with a PID; menubar reconnects.
5. **Verify the rollback.** Re-run Post-cutover steps 1, 2, and 5: menubar green with no badge, logs show clean startup with no degradation, and one real turn narrates correctly (UC-3).
   - *Expected outcome:* the prior known-good behavior is restored.
6. **Notify stakeholders (honest, single-user version).** The sole stakeholder is the operator. "Notification" = leave a note in the local changelog and a `git commit` describing what happened: which commit was rolled back, which check failed, and that the last-good binary is again live. This is also the input to the next attempt at the upgrade.
   - *Expected outcome:* a CHANGELOG entry and a commit exist documenting the rollback; no external parties to contact.

---

## Communication

PulseVoice is a solo project. There is exactly **one stakeholder**: the operator/author. There is no release-committee email, no stakeholder notification list, no customer-facing announcement channel — and fabricating one would be dishonest to the project class.

Communication for a cutover therefore reduces to two local artifacts:

- **A `git commit` message** on the upgrade (or, in the rollback case, on the revert) recording the new commit hash, the build profile, and the verdict (GO / rolled back). This is the audit trail.
- **A `CHANGELOG` entry** appended per the scaffold-dev `appending-changelog-entry` skill: version, date, the cutover outcome, and — on a rollback — the failed commit and the reason. This is what the operator reads next time before upgrading.

The product itself is its own operational alerting channel (phase_10.2.2): it speaks failures aloud and shows a persistent menubar badge (FR-4, NFR-22). No email, Slack, or pager is in scope, by design.

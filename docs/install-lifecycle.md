# PulseVoice daemon — install lifecycle (v1 / VS-1.1.1)

This document is the operator-facing reference for installing, verifying, and
restarting the PulseVoice daemon as a macOS launchd **user agent**. It covers
what VS-1.1.1 ships and explicitly calls out what is deferred to later slices
(codesigning, `.pkg`, TCC recovery, Notification silent-drop fix).

> **Scope.** This is the *light* install glue. The full install lifecycle
> (codesigning/notarization, `.app` bundling, Kokoro model-asset fetch, TCC
> accessibility grant + recovery, `.pkg` installer, clean uninstall) is
> **VS-1.4.5**. This document ships only what makes VS-1.1.1's always-on
> resident posture and its "kill the daemon → it comes back" demo (slice AC2)
> real on the operator's machine.

---

## 1. What gets installed

| Path | Mode | Purpose |
|------|------|---------|
| `~/Library/LaunchAgents/com.pulsevoice.daemon.plist` | 0644 | The launchd **user-agent** plist (auto-start at login, restart on crash). |
| `~/Library/Application Support/PulseVoice/` | 0700 | Socket dir; the daemon's Unix socket lives here as `daemon.sock`. |
| `~/Library/Application Support/PulseVoice/DEGRADED` | — | Marker file the daemon writes on `HookDegraded`; cleared on every clean startup. |
| `~/Library/Logs/PulseVoice/` | 0755 | Log dir. |
| `~/Library/Logs/PulseVoice/daemon.launchd.log` | — | launchd-level stdout+stderr capture (catches pre-tracing crashes). |

The daemon binary itself is **not** copied anywhere by these scripts — the plist
points at `<repo-root>/target/debug/pulse-daemon` by default. Build it first:

```bash
cargo build -p pulse-daemon
```

The daemon is the user domain (`LaunchAgent`), **not** a system `LaunchDaemon`.
PulseVoice is single-user and runs as the operator; a system daemon would need
root and is wrong for the security model.

---

## 2. Install

```bash
scripts/install-agent.sh
```

What it does, in order:

1. Renders `resources/launchd/com.pulsevoice.daemon.plist.template` into
   `~/Library/LaunchAgents/com.pulsevoice.daemon.plist`, substituting the
   resolved socket path, binary path, and launchd-log path.
2. Creates the socket dir at `0700` and the log dir; touches the launchd log
   so `StandardOutPath`/`StandardErrorPath` always exist.
3. `launchctl bootstrap gui/$(id -u) <plist>` — the **modern** verb (replaces
   the deprecated `load`).
4. `launchctl enable` + `launchctl kickstart -k` — ensures the agent is enabled
   and started now (not just at next login).
5. Prints status.

**Idempotent.** Re-running must not fail. `launchctl bootstrap` of an
already-loaded service returns a non-zero the script tolerates; this matters
because the slice's AC2 demo and the rebuild-restart loop both re-install.

### Flags

```
--socket-path PATH   override the daemon socket path
--binary-path  PATH  override the daemon binary path
--plist-path   PATH  override the installed plist path
--log-dir      PATH  override the log dir
--dry-run            render + mkdir + status, do NOT call launchctl
                     (CI/non-macOS-safe; also used by the work-1.05 gate)
-h, --help
```

### Dry-run (the safe preview)

```bash
scripts/install-agent.sh --dry-run \
  --socket-path /tmp/pv-test.sock \
  --binary-path /usr/bin/true \
  --plist-path  /tmp/pv-test.plist
```

`--dry-run` templates the plist, creates the dirs, prints status, and exits 0
**without calling `launchctl`**. It is what lets the install path run on any
host (CI, non-macOS dev) — the real bootstrap is operator-run on macOS only.

---

## 3. Verify

```bash
# the plist is in place
test -f ~/Library/LaunchAgents/com.pulsevoice.daemon.plist

# the agent is loaded and running
launchctl print gui/$(id -u)/com.pulsevoice.daemon | grep -E 'state =|pid ='

# the socket is bound
ls -la ~/Library/Application\ Support/PulseVoice/daemon.sock
```

### The slice's AC2 demo: kill → it comes back

```bash
pid=$(launchctl print gui/$(id -u)/com.pulsevoice.daemon | awk '/pid =/ {print $3}')
kill "$pid"            # SIGTERM → daemon drains, removes socket, exits 0
# ...within ThrottleInterval (default 10s) launchd restarts it
launchctl print gui/$(id -u)/com.pulsevoice.daemon | grep -E 'pid ='
```

**Wait — `kill` here is a clean shutdown, doesn't launchd restart on exit?**
No, and this is load-bearing. The plist uses:

```xml
<key>KeepAlive</key>
<dict>
    <key>SuccessfulExit</key>
    <false/>
</dict>
```

`SuccessfulExit=false` means launchd restarts the daemon **only on a non-zero
exit (crash/panic)** — a *clean* exit 0 (the daemon's SIGTERM/SIGINT graceful
shutdown, built in work-1.03) **stays down**. Unconditional `KeepAlive=true`
would silently undo every clean shutdown. To *fully* stop the auto-restart
agent (not just kill one process), use `uninstall-agent.sh` — that `bootout`s
the agent and removes the plist.

So the AC2 demo as written above actually demonstrates the **graceful-shutdown
stays down** behavior. To demonstrate **crash-restart**, force a non-zero exit:

```bash
kill -9 "$pid"         # SIGKILL → non-zero exit → launchd restarts
```

---

## 4. Restart (the rebuild-restart loop)

During Phase 1 the operator's loop is `cargo build` → test → rebuild →
restart. `restart-agent.sh` is that verb:

```bash
cargo build -p pulse-daemon
scripts/restart-agent.sh
```

What it does:

1. If the agent is currently loaded: `launchctl bootout gui/$(id -u)/<label>`.
2. **Bounded wait (5s default)** for the socket file to disappear — so a stale
   `daemon.sock` from a prior build's crash is not carried into the new agent.
   Prints a warning if the timeout elapses but does **not** fail the restart
   (the daemon clears the `DEGRADED` marker on startup via
   `degraded.rs::clear_degraded`).
3. `launchctl bootstrap gui/$(id -u) <plist>` — **re-reading the existing
   plist**, so the binary path is whatever is currently there (i.e. your
   freshly rebuilt binary).
4. `launchctl enable` + `launchctl kickstart -k`.
5. Prints status.

This is **distinct from**:

- `install-agent.sh` — templates the plist from scratch.
- `uninstall-agent.sh` — `bootout` + removes the plist (and optionally the
  socket/log dirs with `--purge`).

`restart-agent.sh` assumes the plist is already installed; if it isn't, the
script exits non-zero with a "run install-agent.sh first" message.

### Flags

```
--plist-path   PATH   override the installed plist path
--socket-path  PATH   override the socket path (for the removal-wait)
--wait-seconds N      override the socket-removal wait (default 5)
--dry-run             show plan, do NOT call launchctl
-h, --help
```

---

## 5. Uninstall

```bash
scripts/uninstall-agent.sh            # bootout + remove plist
scripts/uninstall-agent.sh --purge    # also remove socket + socket dir + log dir
```

Uses `launchctl bootout gui/$(id -u)/<label>` — the modern verb (replaces the
deprecated `unload`). Idempotent: `bootout` of an already-unloaded service
returns a non-zero the script tolerates, so re-running must not fail.

`--purge` also removes the socket file, the socket dir, and the log dir. The
`DEGRADED` marker lives in the socket dir, so `--purge` removes it too;
without `--purge` it is left in place (a fresh install's daemon clears it on
startup anyway).

---

## 6. Known v1 limitation: the ~10s restart-window silent-drop

**What.** Between a daemon crash and launchd's restart (bounded by
`ThrottleInterval`, default 10s), every Claude Code hook that fires hits the
hook script's fail-fast path (the daemon isn't there to receive it).

**Impact.**

- `Stop` / turn-end events: **fine** — best-effort narration, the next turn
  recovers. The hook logs the miss and exits non-zero without blocking the
  agent.
- `Notification` (attention / permission-prompt) events arriving in that
  window: **silently dropped**. This violates the always-on attention
  guarantee (MASTER-SPEC §5.1, §2.3 success metric: *"trust being called back
  when the agent needs input"*).

**Why deferred.** Fixing it well is its own design problem; VS-1.1.1's README
AC2 was softened to "demonstrates hook non-blocking for Stop/turn events"
precisely because the attention case is deferred. The candidate fixes
 VS-1.1.3 will choose among:

- (a) `Notification`-only small-bound block-and-retry in the hook script;
- (b) a tightened `ThrottleInterval` on the plist (sub-second restart);
- (c) crash-log scan + "while you were away" replay on daemon restart.

**Tracked in:** VS-1.1.3.

---

## 7. What is NOT here (deferred)

| Concern | Slice |
|---------|-------|
| Codesigning / notarization / `.app` bundling | VS-1.4.5 |
| Kokoro model-asset fetch + location logic | VS-1.1.2 (warm Kokoro) + VS-1.4.5 |
| TCC accessibility grant + recovery flow | VS-1.4.1 / VS-1.4.5 |
| `.pkg` installer / GitHub-Release artifacts | VS-1.4.5 |
| Notification silent-drop fix (§6 above) | VS-1.1.3 |
| Mode toggle / narration on-off (the scripts only bring the daemon up; they do NOT enable narration) | VS-1.3.3 |

A known macOS annoyance the install script tolerates: rebuilding the binary can
reset TCC accessibility grants (the binary's signature changes). The script's
idempotent re-bootstrap is precisely what makes "rebuild → re-grant → restart"
survivable as a dev loop. The full TCC-recovery flow is VS-1.4.5.

# pulse-hook

The **ephemeral hook subprocess** for PulseVoice. Claude Code / launchd spawn
it; it reads the Claude Code hook payload from stdin, frames a `WireEvent`,
opens the daemon's Unix socket, writes the frame, and exits.

This is the **producer** half of the hook→daemon IPC pair. The consumer is
`pulse-daemon` (work-1.03).

## What it does

On each invocation:

1. Reads the Claude Code hook payload JSON from **stdin** (verbatrim — the
   bytes drive the content-hash in the synthesized `event_id`).
2. Parses it tolerantly (unknown/extra fields ignored; every expected field
   is best-effort/optional).
3. Derives an `event_id` per the pinned precedence (see below).
4. Maps the hook kind to a `WireEventKind`:
   - `Stop` → `TurnComplete { session_id, turn_id }`
   - `Notification` → `AttentionHint { session_id, event_id, raw_kind, transcript_path }`
   - Degenerate payload → `HookDegraded { reason, session_id }`
5. Opens the daemon socket, writes one length-prefixed-JSON frame, exits.

## Exit codes (fail-fast contract)

The hook **never** blocks the agent beyond two bounded timeouts. Every
failure mode is a distinct non-zero exit so the daemon's logs can interpret
why a delivery didn't land:

| Code | Meaning | Default bound |
|------|---------|---------------|
| `0`  | Delivered | — |
| `2`  | Socket file absent — daemon not running | immediate |
| `3`  | Connect refused or timed out | 200 ms |
| `4`  | Frame write did not complete | 500 ms |

There is **no retry loop** in the hook. Retried/duplicate hooks are deduped
by the daemon (work-1.03), not by re-sending. The hook fires once, delivers
once-or-fails-fast, exits.

## `event_id` derivation (load-bearing for dedup)

The hook derives an identity for each delivery with this precedence:

1. **Prefer Claude Code's `message_id`** when present (the strongest signal).
2. **Else synthesize** from `(session_id, hook_kind, payload_content_hash, payload_size)` —
   a content-aware key so two *distinct* rapid turns with the same session +
   transcript + kind cannot collide.
3. **Else emit `WireEvent::HookDegraded`** — no stable fields derivable at
   all. Loud, never silent (NFR-15).

The content hash is the critical part: it prevents two distinct rapid turns
from colliding on a synthesized key. The naive `(session_id, transcript_path,
hook_kind)` triple would collide between genuinely-distinct rapid turns and
silently drop the second — the exact silent-drop failure mode this slice
retires.

## Usage

```bash
pulse-hook --socket-path /path/to/daemon.sock --hook-kind stop \
  <<< '{"transcript_path":"/path/to/transcript.jsonl","session_id":"abc123"}'
echo "exit=$?"   # 0 = delivered; 2/3/4 = fail-fast (see table above)
```

### Claude Code hook-config snippet

Wire `pulse-hook` as both the `Stop` and `Notification` hook in your Claude
Code settings, pointing at the daemon socket path:

```jsonc
{
  "hooks": {
    "Stop": [{
      "type": "command",
      "command": "pulse-hook --socket-path ~/Library/Application Support/PulseVoice/daemon.sock --hook-kind stop"
    }],
    "Notification": [{
      "type": "command",
      "command": "pulse-hook --socket-path ~/Library/Application Support/PulseVoice/daemon.sock --hook-kind notification"
    }]
  }
}
```

The payload JSON is delivered on stdin by Claude Code; the hook reads it
itself.

## Crate shape

- `src/payload.rs` — Claude-Code-specific payload parse + `event_id`
  derivation (the only place that knows the hook JSON shape).
- `src/dispatch.rs` — `deliver(event, socket_path, connect_t, write_t)`
  with bounded timeouts and the typed `DeliverOutcome` (0/2/3/4).
- `src/cli.rs` — clap-derived argv.
- `src/main.rs` — binary entry; reads stdin, builds the event, delivers,
  exits with the typed `ExitCode`.
- `src/lib.rs` — re-exports + `build_event` (the kind-mapping + degraded
  fallback).

## Test coverage

- `src/payload.rs` unit tests — parse tolerance, `event_id` precedence,
  content-hash divergence.
- `src/dispatch.rs` unit tests — exit-code table.
- `tests/dispatch_failfast.rs` — the full delivery matrix: absent socket → 2,
  refused connect → 3, write timeout → 4, happy path → 0 (with wire
  round-trip decode).

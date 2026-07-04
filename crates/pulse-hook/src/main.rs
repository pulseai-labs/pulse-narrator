//! `pulse-hook` binary entry point.
//!
//! Reads the Claude Code hook payload from stdin, builds a [`WireEvent`],
//! delivers it to the daemon socket with bounded timeouts, and exits with a
//! typed [`ExitCode`](std::process::ExitCode) per the fail-fast table (0/2/3/4).
//!
//! Exit codes (also in `--help` and the crate README):
//!
//! | Code | Meaning |
//! |---|---|
//! | 0 | Delivered |
//! | 2 | Socket absent — daemon not running |
//! | 3 | Connect refused / timed out (>200 ms) |
//! | 4 | Frame write did not complete (>500 ms) |
//!
//! The hook never blocks the agent beyond those two bounded timeouts.

use std::process::ExitCode;

use clap::Parser;
use pulse_core::wire::WireEvent;
use pulse_hook::{
    build_event, deliver, read_payload_stdin, ClaudeHookPayload, Cli, DeliverOutcome, HookKind,
};
use tokio::runtime::Runtime;

fn main() -> ExitCode {
    // Initialize a minimal tracing subscriber so the bounded-timeout warn
    // lines land somewhere visible in dev. In production launchd captures
    // stderr; the daemon's own log config is its concern, not the hook's.
    let _ = tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .with_writer(std::io::stderr)
        .try_init();

    let cli = Cli::parse();
    let runtime = match Runtime::new() {
        Ok(rt) => rt,
        Err(e) => {
            eprintln!("pulse-hook: failed to start tokio runtime: {e}");
            return ExitCode::from(4);
        }
    };

    runtime.block_on(run(cli))
}

async fn run(cli: Cli) -> ExitCode {
    // Read the payload verbatim from stdin. The bytes drive the content-hash
    // in the synthesized event_id; do not re-serialize.
    let raw_payload = match read_payload_stdin() {
        Ok(b) => b,
        Err(e) => {
            eprintln!("pulse-hook: failed to read payload from stdin: {e}");
            return ExitCode::from(4);
        }
    };

    // Empty stdin is a degenerate invocation (no payload at all). Forward as
    // HookDegraded rather than crashing — the daemon decides what to do.
    let payload: ClaudeHookPayload = if raw_payload.is_empty() {
        ClaudeHookPayload::default()
    } else {
        match ClaudeHookPayload::from_bytes(&raw_payload) {
            Ok(p) => p,
            Err(e) => {
                eprintln!("pulse-hook: payload JSON parse failed: {e}");
                // Build a degraded envelope directly so the daemon still
                // hears about the hook fire.
                let event = WireEvent::new(pulse_core::wire::WireEventKind::HookDegraded {
                    reason: format!("payload JSON parse failed: {e}"),
                    session_id: None,
                });
                return deliver_and_exit(&cli, &event).await;
            }
        }
    };

    let hook_kind: HookKind = cli.hook_kind.into();
    let event = build_event(&payload, &raw_payload, hook_kind);
    deliver_and_exit(&cli, &event).await
}

async fn deliver_and_exit(cli: &Cli, event: &WireEvent) -> ExitCode {
    let (connect_t, write_t) = resolve_timeouts(cli.timeout_ms);
    let outcome = deliver(event, &cli.socket_path, connect_t, write_t).await;
    match outcome {
        DeliverOutcome::Delivered => ExitCode::SUCCESS,
        DeliverOutcome::SocketAbsent => {
            eprintln!(
                "pulse-hook: daemon not running (no socket at {})",
                cli.socket_path.display()
            );
            ExitCode::from(2)
        }
        DeliverOutcome::ConnectFailed => {
            eprintln!(
                "pulse-hook: connect to {} refused or timed out (>{:?})",
                cli.socket_path.display(),
                connect_t
            );
            ExitCode::from(3)
        }
        DeliverOutcome::WriteTimedOut => {
            eprintln!(
                "pulse-hook: frame write to {} did not complete (>{:?})",
                cli.socket_path.display(),
                write_t
            );
            ExitCode::from(4)
        }
    }
}

/// Resolve the (connect, write) timeout pair from the CLI override.
///
/// `0` (default) → use the spec-pinned defaults (200 ms connect, 500 ms
/// write). A positive value overrides *both* — kept simple for v1.
fn resolve_timeouts(timeout_ms: u64) -> (std::time::Duration, std::time::Duration) {
    if timeout_ms == 0 {
        (
            pulse_hook::DEFAULT_CONNECT_TIMEOUT,
            pulse_hook::DEFAULT_WRITE_TIMEOUT,
        )
    } else {
        let t = std::time::Duration::from_millis(timeout_ms);
        (t, t)
    }
}

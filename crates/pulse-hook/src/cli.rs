//! Command-line interface for the `pulse-hook` binary.
//!
//! Claude Code / launchd spawn the hook with a fixed argv shape:
//!
//! ```text
//! pulse-hook --socket-path <path> --hook-kind {stop|notification} [--timeout-ms <n>]
//! ```
//!
//! The hook payload itself arrives on **stdin** as a JSON blob (Claude Code's
//! documented contract), not on argv — argv is reserved for the daemon
//! location + the hook kind, which the daemon's own logs need to interpret
//! delivery failures.

use std::path::PathBuf;

use clap::Parser;

use crate::payload::HookKind;

/// Ephemeral hook subprocess. Reads the Claude Code hook payload from stdin,
/// delivers one framed event to the daemon socket, exits.
#[derive(Debug, Clone, Parser)]
#[command(
    name = "pulse-hook",
    about = "PulseVoice hook subprocess — deliver one event to the daemon, fail fast.",
    long_about = "PulseVoice hook subprocess.\n\n\
                  Reads the Claude Code hook payload JSON from stdin, frames a WireEvent, \
                  opens the daemon's Unix socket, writes the frame, and exits.\n\n\
                  Exit codes:\n  \
                  0  delivered successfully\n  \
                  2  socket file absent (daemon not running)\n  \
                  3  connect refused or timed out (bounded connect, default 200 ms)\n  \
                  4  frame write did not complete (bounded write, default 500 ms)\n\n\
                  The hook never blocks the agent beyond those two bounded timeouts."
)]
pub struct Cli {
    /// Path to the daemon's Unix domain socket.
    #[arg(long)]
    pub socket_path: PathBuf,

    /// Which Claude Code hook fired.
    #[arg(long, value_enum)]
    pub hook_kind: HookKindArg,

    /// Bounded connect+write timeout in milliseconds (applies to each phase).
    ///
    /// Defaults split the budget: 200 ms connect, 500 ms write. If supplied,
    /// this single value overrides *both* (kept simple for v1 — the daemon's
    /// absence is the common case, and one knob is enough to tune it).
    #[arg(long, default_value_t = 0)]
    pub timeout_ms: u64,
}

/// clap-friendly mirror of [`HookKind`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
#[value(rename_all = "lower")]
pub enum HookKindArg {
    Stop,
    Notification,
}

impl From<HookKindArg> for HookKind {
    fn from(arg: HookKindArg) -> Self {
        match arg {
            HookKindArg::Stop => HookKind::Stop,
            HookKindArg::Notification => HookKind::Notification,
        }
    }
}

/// Read the hook payload JSON verbatim from stdin.
///
/// The bytes are returned as-is (no trimming, no re-serialization) so the
/// content-hash in [`crate::payload::payload_content_hash`] is computed over
/// exactly what Claude Code sent — keeping the synthesized `event_id` stable
/// across hook invocations for the same payload.
pub fn read_payload_stdin() -> Result<Vec<u8>, std::io::Error> {
    use std::io::Read;
    let mut buf = Vec::new();
    std::io::stdin().read_to_end(&mut buf)?;
    Ok(buf)
}

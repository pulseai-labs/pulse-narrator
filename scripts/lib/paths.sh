# shellcheck shell=bash
# scripts/lib/paths.sh — shared path-resolution module for the PulseVoice
# install/uninstall/restart script trio.
#
# Single source of truth for the four paths every operator-facing script needs:
#
#   * the daemon's Unix socket           ($PV_SOCKET_PATH)
#   * the launchd StandardOut/Error logs ($PV_LOG_DIR)
#   * the installed launchd plist        ($PV_PLIST_PATH)
#   * the DEGRADED marker file           ($PV_MARKER_PATH)
#
# These defaults mirror the daemon's own resolution (crates/pulse-daemon/src
# `default_socket_path()` + `degraded.rs::marker_path()`) so the scripts and the
# Rust binary agree without a config file. Every path is overridable via an
# env var so tests (and the `--dry-run` AC-7 gate) can point at /tmp without
# touching $HOME. Source this file; do NOT execute it directly.
#
# Env-var overrides:
#   PV_HOME          — base home dir (defaults to $HOME; for tests)
#   PV_SOCKET_PATH   — full socket path (default below)
#   PV_LOG_DIR       — log dir       (default below)
#   PV_PLIST_PATH    — installed plist path (default below)
#   PV_MARKER_PATH   — DEGRADED marker path (default below)

# Guard against double-source.
if [[ -n "${_PV_PATHS_SH_SOURCED:-}" ]]; then
  return 0 2>/dev/null || exit 0
fi
_PV_PATHS_SH_SOURCED=1

# Resolve the base home directory. Tests override PV_HOME to dodge $HOME.
_pv_home() {
  printf '%s' "${PV_HOME:-${HOME:-}}"
}

# socket: $HOME/Library/Application Support/PulseVoice/daemon.sock
pv_socket_path() {
  printf '%s' "${PV_SOCKET_PATH:-$(_pv_home)/Library/Application Support/PulseVoice/daemon.sock}"
}

# socket parent dir (mode 0700): the daemon also creates this defensively.
pv_socket_dir() {
  dirname "$(pv_socket_path)"
}

# log dir: $HOME/Library/Logs/PulseVoice
# (launchd's StandardOutPath/StandardErrorPath live here; the daemon's own
# tracing rotating file is separate and lives under the same dir in 1.04+.)
pv_log_dir() {
  printf '%s' "${PV_LOG_DIR:-$(_pv_home)/Library/Logs/PulseVoice}"
}

# launchd-level capture log (stderr+stdout launchd owns, for pre-tracing crashes).
pv_launchd_log() {
  printf '%s' "$(pv_log_dir)/daemon.launchd.log"
}

# installed plist: ~/Library/LaunchAgents/com.pulsevoice.daemon.plist
# (User domain — LaunchAgent, NOT a system LaunchDaemon.)
pv_plist_path() {
  printf '%s' "${PV_PLIST_PATH:-$(_pv_home)/Library/LaunchAgents/com.pulsevoice.daemon.plist}"
}

# DEGRADED marker: $HOME/Library/Application Support/PulseVoice/DEGRADED
# Matches degraded.rs::marker_path() exactly.
pv_marker_path() {
  printf '%s' "${PV_MARKER_PATH:-$(_pv_home)/Library/Application Support/PulseVoice/DEGRADED}"
}

# Default daemon binary path: <repo-root>/target/debug/pulse-daemon.
# Resolved relative to this lib file so the scripts work from any cwd.
# Operators override with --binary-path (install-agent.sh) or PV_BINARY_PATH.
pv_default_binary_path() {
  local script_dir lib_dir repo_root
  script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
  lib_dir="$(cd "$script_dir" && pwd)"            # .../scripts/lib
  repo_root="$(cd "$lib_dir/../.." && pwd)"       # repo root
  printf '%s/target/debug/pulse-daemon' "$repo_root"
}

# The launchd label the plist registers under (also its service target id).
pv_label() {
  printf '%s' 'com.pulsevoice.daemon'
}

# The user-domain target string modern launchctl wants: gui/<uid>.
pv_domain_target() {
  printf 'gui/%s' "$(id -u)"
}

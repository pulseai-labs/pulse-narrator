#!/usr/bin/env bash
# scripts/uninstall-agent.sh — uninstall the PulseVoice daemon launchd user agent.
#
# Uses the MODERN launchctl verb: `launchctl bootout gui/$(id -u)/<label>` (the
# replacement for the deprecated `unload`). Removes the installed plist.
# Optionally removes the socket + log dirs with --purge.
#
# Usage:
#   scripts/uninstall-agent.sh               # bootout + remove plist
#   scripts/uninstall-agent.sh --purge       # also remove socket + log dirs
#   scripts/uninstall-agent.sh --plist-path P
#   scripts/uninstall-agent.sh --dry-run     # show what would happen, no launchctl
#   scripts/uninstall-agent.sh -h | --help
#
# Flags:
#   --plist-path PATH   installed plist path (default: paths.sh)
#   --socket-path PATH  socket path (default: paths.sh; for --purge)
#   --log-dir     PATH  log dir (default: paths.sh; for --purge)
#   --purge             also remove the socket file + socket dir + log dir
#   --dry-run           print actions, do NOT call launchctl or remove anything
#   -h, --help          print this help and exit 0
#
# Exit codes:
#   0  success (bootout tolerates "not loaded" — idempotent)
#   1  mis-use / bad flags
#
# Idempotency: bootout of an already-unloaded service returns an error the
# script tolerates, so re-running uninstall must not fail.
set -euo pipefail

_script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=lib/paths.sh
. "$_script_dir/lib/paths.sh"

dry_run=0
purge=0
opt_plist=""
opt_socket=""
opt_log_dir=""

usage() {
  sed -n '2,/^set -euo/p' "$0" | sed 's/^# \?//' | sed '/^set -euo/d'
}

while [[ $# -gt 0 ]]; do
  case "$1" in
    --plist-path)  opt_plist="${2:?--plist-path requires a value}";   shift 2 ;;
    --socket-path) opt_socket="${2:?--socket-path requires a value}"; shift 2 ;;
    --log-dir)     opt_log_dir="${2:?--log-dir requires a value}";    shift 2 ;;
    --purge)       purge=1; shift ;;
    --dry-run)     dry_run=1; shift ;;
    -h|--help)     usage; exit 0 ;;
    *) echo "uninstall-agent.sh: unknown argument: $1" >&2; usage >&2; exit 1 ;;
  esac
done

[[ -n "$opt_plist"   ]] && PV_PLIST_PATH="$opt_plist"   && export PV_PLIST_PATH
[[ -n "$opt_socket"  ]] && PV_SOCKET_PATH="$opt_socket" && export PV_SOCKET_PATH
[[ -n "$opt_log_dir" ]] && PV_LOG_DIR="$opt_log_dir"    && export PV_LOG_DIR

plist_path="$(pv_plist_path)"
socket_path="$(pv_socket_path)"
socket_dir="$(dirname "$socket_path")"
log_dir="$(pv_log_dir)"
target="$(pv_label)"
domain="$(pv_domain_target)"

log() { printf '[uninstall-agent] %s\n' "$*"; }

# bootout the agent. Tolerates "not loaded" (idempotent re-uninstall).
_bootout() {
  if launchctl bootout "$domain/$target" 2>/dev/null; then
    log "booted out $domain/$target"
  else
    # bootout fails if not loaded; tolerate so re-uninstall is a no-op.
    log "bootout returned non-zero (service may not be loaded) — tolerating"
  fi
}

print_status() {
  log "target:    $domain/$target"
  log "plist:     $plist_path"
  if [[ "$purge" -eq 1 ]]; then
    log "socket:    $socket_path (will be removed)"
    log "socket dir: $socket_dir (will be removed)"
    log "log dir:   $log_dir (will be removed)"
  fi
  if [[ "$dry_run" -eq 1 ]]; then
    log "mode:      --dry-run (launchctl NOT called, nothing removed)"
  else
    log "mode:      live (launchctl bootout)"
  fi
}

print_status

if [[ "$dry_run" -eq 1 ]]; then
  log "--dry-run: skipping launchctl and removals"
  exit 0
fi

log "booting out agent"
_bootout

log "removing plist ($plist_path)"
rm -f "$plist_path" || true

if [[ "$purge" -eq 1 ]]; then
  log "--purge: removing socket + socket dir + log dir"
  rm -f "$socket_path" 2>/dev/null || true
  rmdir "$socket_dir" 2>/dev/null || true
  rm -rf "$log_dir" 2>/dev/null || true
fi

log "done."
exit 0

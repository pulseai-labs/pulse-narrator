#!/usr/bin/env bash
# scripts/restart-agent.sh — the operator's rebuild-restart loop verb.
#
# Distinct from install-agent.sh (which templates the plist from scratch) and
# uninstall-agent.sh (which removes it): restart ASSUMES the plist is already
# installed at ~/Library/LaunchAgents/com.pulsevoice.daemon.plist and just
# cycles the running agent. This is the verb the operator lives in during
# Phase 1's cargo-build → test → rebuild → restart loop.
#
# Flow:
#   1. bootout gui/<uid>/<label>   IF the agent is currently loaded
#      (tolerates "not loaded" — idempotent)
#   2. bounded wait (default 5s, --wait-seconds N) for the socket file to
#      disappear, so a stale daemon.sock from the prior build's crash is not
#      carried into the new agent. Prints a warning if the timeout elapses
#      but does NOT fail the restart — the daemon clears the DEGRADED marker
#      on startup anyway (degraded.rs::clear_degraded, 1.03).
#   3. launchctl bootstrap gui/<uid> <plist>  re-reading the EXISTING plist
#      (so the binary path is whatever is currently there — freshly rebuilt)
#   4. print status
#
# Usage:
#   scripts/restart-agent.sh                  # full restart
#   scripts/restart-agent.sh --wait-seconds 5 # override socket-removal wait
#   scripts/restart-agent.sh --plist-path P
#   scripts/restart-agent.sh --dry-run        # show plan, no launchctl
#   scripts/restart-agent.sh -h | --help
#
# Exit codes:
#   0  success
#   1  mis-use / bad flags, OR plist missing (run install-agent.sh first)
set -euo pipefail

_script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=lib/paths.sh
. "$_script_dir/lib/paths.sh"

dry_run=0
wait_seconds=5
opt_plist=""
opt_socket=""

usage() {
  sed -n '2,/^set -euo/p' "$0" | sed 's/^# \?//' | sed '/^set -euo/d'
}

while [[ $# -gt 0 ]]; do
  case "$1" in
    --plist-path)   opt_plist="${2:?--plist-path requires a value}";   shift 2 ;;
    --socket-path)  opt_socket="${2:?--socket-path requires a value}"; shift 2 ;;
    --wait-seconds) wait_seconds="${2:?--wait-seconds requires a value}"; shift 2 ;;
    --dry-run)      dry_run=1; shift ;;
    -h|--help)      usage; exit 0 ;;
    *) echo "restart-agent.sh: unknown argument: $1" >&2; usage >&2; exit 1 ;;
  esac
done

[[ -n "$opt_plist"  ]] && PV_PLIST_PATH="$opt_plist"  && export PV_PLIST_PATH
[[ -n "$opt_socket" ]] && PV_SOCKET_PATH="$opt_socket" && export PV_SOCKET_PATH

plist_path="$(pv_plist_path)"
socket_path="$(pv_socket_path)"
target="$(pv_label)"
domain="$(pv_domain_target)"

log()  { printf '[restart-agent] %s\n' "$*"; }
warn() { printf '[restart-agent] WARN: %s\n' "$*" >&2; }

if [[ ! -f "$plist_path" ]]; then
  echo "restart-agent.sh: installed plist not found at $plist_path" >&2
  echo "  run scripts/install-agent.sh first (restart re-uses the existing plist)" >&2
  exit 1
fi

print_status() {
  log "target:  $domain/$target"
  log "plist:   $plist_path (re-read as-is)"
  log "socket:  $socket_path"
  log "wait:    ${wait_seconds}s for socket removal after bootout"
  if [[ "$dry_run" -eq 1 ]]; then
    log "mode:    --dry-run (launchctl NOT called)"
  else
    log "mode:    live (launchctl bootout + bootstrap)"
  fi
}

# Is the agent currently loaded? launchctl print exits 0 iff loaded.
_loaded() {
  launchctl print "$domain/$target" >/dev/null 2>&1
}

_wait_socket_gone() {
  local elapsed=0
  while [[ -e "$socket_path" ]] && [[ "$elapsed" -lt "$wait_seconds" ]]; do
    sleep 1
    elapsed=$((elapsed + 1))
  done
  if [[ -e "$socket_path" ]]; then
    warn "socket $socket_path still present after ${wait_seconds}s — continuing anyway" \
         "(the daemon clears the DEGRADED marker on startup; a stale socket will be" \
         "overwritten when the new agent binds)"
  else
    log "socket removed after ${elapsed}s"
  fi
}

print_status

if [[ "$dry_run" -eq 1 ]]; then
  log "--dry-run: skipping launchctl"
  exit 0
fi

if _loaded; then
  log "agent loaded — booting out"
  launchctl bootout "$domain/$target" 2>/dev/null || \
    warn "bootout returned non-zero — continuing (race with concurrent unload?)"
  log "waiting up to ${wait_seconds}s for socket removal"
  _wait_socket_gone
else
  log "agent not loaded — skipping bootout"
fi

log "bootstrapping (re-reading existing plist)"
if launchctl bootstrap "$domain" "$plist_path" 2>/dev/null; then
  log "bootstrapped $domain/$target"
else
  # Tolerate already-loaded (e.g. concurrent restart race).
  warn "bootstrap returned non-zero (service may already be loaded) — kickstarting"
fi
launchctl enable "$domain/$target" 2>/dev/null || true
launchctl kickstart -k "$domain/$target" 2>/dev/null || true

log "done."
exit 0

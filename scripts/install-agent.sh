#!/usr/bin/env bash
# scripts/install-agent.sh — install the PulseVoice daemon as a launchd user agent.
#
# Templated from resources/launchd/com.pulsevoice.daemon.plist.template, with
# the socket/log/binary paths resolved via scripts/lib/paths.sh (overridable via
# env vars + the flags below). Idempotent: re-running must not fail.
#
# Usage:
#   scripts/install-agent.sh                    # full install (templates +
#                                               #   mkdirs + launchctl bootstrap)
#   scripts/install-agent.sh --dry-run          # template + mkdirs + status,
#                                               #   NO launchctl (CI/non-macOS-safe)
#   scripts/install-agent.sh --socket-path S \
#       --binary-path B --plist-path P --dry-run
#
# Flags:
#   --socket-path PATH   daemon Unix socket (default: paths.sh)
#   --binary-path  PATH  daemon binary (default: <repo>/target/debug/pulse-daemon)
#   --plist-path   PATH  installed plist path (default: paths.sh)
#   --log-dir      PATH  log dir               (default: paths.sh)
#   --dry-run            template + mkdir + status, skip launchctl entirely
#   -h, --help           print this help and exit 0
#
# Exit codes:
#   0  success (or --dry-run success)
#   1  mis-use / bad flags
#   2  prerequisite missing (binary not found in non-dry-run mode)
#
# Idempotency: re-bootstrap of an already-loaded service is a no-op for
# launchctl bootstrap (it returns an error the script tolerates) — see
# `_bootstrap` below. This is what lets the slice's AC2 demo (kill + restart)
# and the operator's rebuild-restart loop not blow up on the second install.
set -euo pipefail

# --- locate the lib + repo root from this script's location ------------------
_script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=lib/paths.sh
. "$_script_dir/lib/paths.sh"

# --- defaults ----------------------------------------------------------------
dry_run=0
opt_socket=""
opt_binary=""
opt_plist=""
opt_log_dir=""

usage() {
  sed -n '2,/^set -euo/p' "$0" | sed 's/^# \?//' | sed '/^set -euo/d'
}

while [[ $# -gt 0 ]]; do
  case "$1" in
    --socket-path) opt_socket="${2:?--socket-path requires a value}"; shift 2 ;;
    --binary-path) opt_binary="${2:?--binary-path requires a value}"; shift 2 ;;
    --plist-path)  opt_plist="${2:?--plist-path requires a value}";   shift 2 ;;
    --log-dir)     opt_log_dir="${2:?--log-dir requires a value}";    shift 2 ;;
    --dry-run)     dry_run=1; shift ;;
    -h|--help)     usage; exit 0 ;;
    *) echo "install-agent.sh: unknown argument: $1" >&2; usage >&2; exit 1 ;;
  esac
done

# Flags override env vars override paths.sh defaults.
[[ -n "$opt_socket"  ]] && PV_SOCKET_PATH="$opt_socket"   && export PV_SOCKET_PATH
[[ -n "$opt_binary"  ]] || opt_binary="${PV_BINARY_PATH:-$(pv_default_binary_path)}"
[[ -n "$opt_plist"   ]] && PV_PLIST_PATH="$opt_plist"     && export PV_PLIST_PATH
[[ -n "$opt_log_dir" ]] && PV_LOG_DIR="$opt_log_dir"      && export PV_LOG_DIR

socket_path="$(pv_socket_path)"
plist_path="$(pv_plist_path)"
log_dir="$(pv_log_dir)"
launchd_log="$(pv_launchd_log)"
socket_dir="$(dirname "$socket_path")"
template="$_script_dir/../resources/launchd/com.pulsevoice.daemon.plist.template"

# --- helpers -----------------------------------------------------------------
log()  { printf '[install-agent] %s\n' "$*"; }
warn() { printf '[install-agent] WARN: %s\n' "$*" >&2; }

# Template the plist: substitute the @@PV_*@@ tokens. Uses a temporary file
# then atomically moves into place so a partial write is never observable to
# launchd. Returns the rendered plist path on stdout.
render_plist() {
  local out="$1"
  if [[ ! -f "$template" ]]; then
    echo "install-agent.sh: plist template missing: $template" >&2
    exit 1
  fi
  local tmp
  tmp="$(mktemp "${out}.XXXXXX")"
  # Order: longest tokens first; none of these tokens is a prefix of another,
  # but sed -e ordering is explicit anyway for auditability.
  sed \
    -e "s|@@PV_BINARY_PATH@@|${opt_binary}|g" \
    -e "s|@@PV_SOCKET_PATH@@|${socket_path}|g" \
    -e "s|@@PV_LAUNCHD_LOG@@|${launchd_log}|g" \
    "$template" > "$tmp"
  mkdir -p "$(dirname "$out")"
  mv "$tmp" "$out"
  printf '%s' "$out"
}

ensure_dirs() {
  # socket dir, mode 0700 (the daemon also does this defensively; doing it at
  # install time lets the operator inspect ownership/perms before first run).
  mkdir -p "$socket_dir"
  if [[ "$(uname)" == "Darwin" ]] || [[ "$(uname -s)" == "Darwin" ]]; then
    chmod 0700 "$socket_dir" 2>/dev/null || true
  fi
  # log dir
  mkdir -p "$log_dir"
  # touch the launchd capture log so StandardOutPath/StandardErrorPath always exist
  mkdir -p "$(dirname "$launchd_log")"
  touch "$launchd_log"
}

# bootstrap the agent. Tolerates "service already loaded" (idempotent re-install).
_bootstrap() {
  local target domain
  target="$(pv_label)"
  domain="$(pv_domain_target)"
  if launchctl bootstrap "$domain" "$plist_path" 2>/dev/null; then
    log "bootstrapped $target into $domain"
  else
    # bootstrap fails if already loaded; that's the idempotent re-install case.
    log "bootstrap returned non-zero (service may already be loaded) — tolerating"
  fi
  # Kick it in case RunAtLoad didn't fire (e.g. the agent was loaded but disabled).
  launchctl enable "$domain/$target" 2>/dev/null || true
  launchctl kickstart -k "$domain/$target" 2>/dev/null || true
}

print_status() {
  local target domain
  target="$(pv_label)"
  domain="$(pv_domain_target)"
  log "binary:      $opt_binary"
  log "socket:      $socket_path"
  log "plist:       $plist_path"
  log "log dir:     $log_dir"
  log "launchd log: $launchd_log"
  if [[ "$dry_run" -eq 1 ]]; then
    log "mode:        --dry-run (launchctl NOT called)"
  else
    log "target:      $domain/$target"
    log "mode:        live (launchctl bootstrap)"
  fi
}

# --- main --------------------------------------------------------------------
if [[ ! -f "$template" ]]; then
  echo "install-agent.sh: plist template not found at $template" >&2
  exit 1
fi

# In live mode, the binary must exist (so the agent doesn't crash-loop on boot).
# In --dry-run mode we do NOT require it: the gate passes --binary-path
# /usr/bin/true specifically so it can run on hosts without the daemon built.
if [[ "$dry_run" -eq 0 ]] && [[ ! -x "$opt_binary" ]]; then
  echo "install-agent.sh: daemon binary not found or not executable: $opt_binary" >&2
  echo "  (build it first: cargo build -p pulse-daemon)" >&2
  echo "  (or pass --dry-run to template + mkdir without launchctl)" >&2
  exit 2
fi

log "rendering plist template -> $plist_path"
render_plist "$plist_path" >/dev/null

log "ensuring socket + log dirs"
ensure_dirs

if [[ "$dry_run" -eq 1 ]]; then
  log "--dry-run: skipping launchctl"
else
  log "bootstrapping agent"
  _bootstrap
fi

print_status
log "done."
exit 0

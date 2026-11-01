#!/usr/bin/env bash
# Run as the desktop user, once or from a user timer. No root or profiling
# hooks: keep frame timings undisturbed and retain evidence across reboots.
set -euo pipefail
destination=${1:-${XDG_STATE_HOME:-$HOME/.local/state}/sekka-xe-dpt-burn-in}
mkdir -p "$destination"
boot_id=$(cat /proc/sys/kernel/random/boot_id)
log="$destination/$boot_id.log"
{
  printf '\n=== %s ===\n' "$(date --iso-8601=seconds)"
  printf 'boot_id: %s\n' "$boot_id"
  printf 'kernel: %s\n' "$(uname -r)"
  printf 'booted_system: %s\n' "$(readlink -f /run/booted-system)"
  printf 'uptime_seconds: '
  cat /proc/uptime
  printf 'xe_module: '
  module=$(modinfo -b /run/booted-system/kernel-modules -n xe)
  printf '%s\n' "$module"
  sha256sum "$module"
  printf 'xe_taint: '
  cat /sys/module/xe/taint
  cat /proc/buddyinfo
  # Failure (for example while logged out) stays visible in the log and in
  # the timer's service result; the next run can succeed after login.
  aizomectl stats
} >> "$log" 2>&1
printf '%s\n' "$log"

#!/usr/bin/env bash
# Isolated dconf, D-Bus, and Xvfb; no host preferences, portals, or display.
set -euo pipefail
trap 'echo "Failed at line $LINENO: $BASH_COMMAND" >&2; test ! -f portal.log || cat portal.log >&2' ERR

interface=org.gnome.desktop.interface
export DCONF_PROFILE="$TATAMI_TEST_USER_PROFILE"
gsettings set "$interface" color-scheme prefer-light
gsettings set "$interface" gtk-theme UserTheme

# Negative control: the old Tatami dark default loses to the saved light choice.
export DCONF_PROFILE="$TATAMI_TEST_UNLOCKED_PROFILE"
test "$(gsettings get "$interface" color-scheme)" = "'prefer-light'"

export DCONF_PROFILE="$TATAMI_TEST_PROFILE"
test "$(gsettings get "$interface" color-scheme)" = "'prefer-dark'"
test "$(gsettings writable "$interface" color-scheme)" = false
test "$(gsettings get "$interface" gtk-theme)" = "'UserTheme'"
test "$(gsettings writable "$interface" gtk-theme)" = true

# Exercise the actual Settings backend used in Tatami, not a portal stand-in.
"$TATAMI_TEST_PORTAL" > portal.log 2>&1 &
portal_pid=$!
trap 'kill "$portal_pid" 2>/dev/null || true' EXIT
for attempt in $(seq 1 100); do
  if answer=$(gdbus call --session \
    --dest org.freedesktop.impl.portal.desktop.gtk \
    --object-path /org/freedesktop/portal/desktop \
    --method org.freedesktop.impl.portal.Settings.Read \
    org.freedesktop.appearance color-scheme 2>/dev/null); then
    break
  fi
  if ! kill -0 "$portal_pid" 2>/dev/null; then
    cat portal.log
    exit 1
  fi
  sleep 0.1
done
test "$answer" = '(<uint32 1>,)'

# Selecting a different profile still exposes the user's original preference.
export DCONF_PROFILE="$TATAMI_TEST_USER_PROFILE"
test "$(gsettings get "$interface" color-scheme)" = "'prefer-light'"
test "$(gsettings writable "$interface" color-scheme)" = true
echo 'Tatami advertises dark despite a saved light preference; other profiles retain light.'

#!/usr/bin/env bash
# Run a Tatami GTK test binary the way the TAP reproductions ran: under
# reproductions/isolate (default shell), on a private Xvfb display, with a
# private session bus and a preload shim that refuses host services.
#
#   gtkrun.sh <test binary> <test filter and args...>
#
# The shim refuses Unix sockets under /run and /var/run, X displays other
# than the private one, all IP connections, and kill() of processes outside
# the test's own tree, logging refusals to $TMPDIR/shim.log. It is built on
# first use. Run one GTK test per process.
set -euo pipefail
here=$(cd "$(dirname "$0")" && pwd)
: "${ISOLATE_SCRATCH:?run through reproductions/isolate}"
display=${REPRO_X_DISPLAY:-217}
mkdir -p "$TMPDIR/shim"
if [[ ! -e $TMPDIR/shim/repro-shim.so ]]; then
  cc -shared -fPIC -O2 -o "$TMPDIR/shim/repro-shim.so" "$here/repro-shim.c" -ldl
fi
if [[ ! -S /tmp/.X11-unix/X$display ]]; then
  setsid Xvfb ":$display" -nolisten tcp -screen 0 1600x1200x24 -noreset >"$TMPDIR/xvfb.log" 2>&1 &
  for _ in $(seq 50); do [[ -S /tmp/.X11-unix/X$display ]] && break; sleep 0.1; done
fi
cat >"$TMPDIR/bus.conf" <<CONF
<!DOCTYPE busconfig PUBLIC "-//freedesktop//DTD D-Bus Bus Configuration 1.0//EN" "http://www.freedesktop.org/standards/dbus/1.0/busconfig.dtd">
<busconfig>
  <type>session</type>
  <listen>unix:dir=$XDG_RUNTIME_DIR</listen>
  <auth>EXTERNAL</auth>
  <policy context="default">
    <allow send_destination="*"/>
    <allow receive_sender="*"/>
    <allow own="*"/>
  </policy>
</busconfig>
CONF
export DISPLAY=:$display GDK_BACKEND=x11 GSK_RENDERER=cairo GTK_A11Y=none NO_AT_BRIDGE=1 \
  GIO_USE_VFS=local GSETTINGS_BACKEND=memory
export SEKKA_REPRO_X_SOCKET=/tmp/.X11-unix/X$display SEKKA_REPRO_SHIM_LOG=$TMPDIR/shim.log
exec dbus-run-session --config-file="$TMPDIR/bus.conf" -- \
  bash -c 'export SEKKA_REPRO_ROOT_PID=$$; exec env LD_PRELOAD="$TMPDIR/shim/repro-shim.so" "$@"' _ "$@"

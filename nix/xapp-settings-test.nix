# Exercise the real backend's XSettings startup callback and D-Bus API. The
# unpatched package must reproduce the crash; both use isolated X/DBus sessions.
{ pkgs, patched }:
pkgs.runCommand "sekka-xapp-settings"
  {
    nativeBuildInputs = [
      pkgs.python3
      pkgs.dbus
      pkgs.glib
      pkgs.xvfb-run
      pkgs.xsettingsd
    ];
  }
  ''
    export HOME=$TMPDIR/home
    export XDG_RUNTIME_DIR=$TMPDIR/runtime
    mkdir -p "$HOME" "$XDG_RUNTIME_DIR" "$out/baseline" "$out/patched"
    chmod 700 "$XDG_RUNTIME_DIR"
    cd "$out/baseline"
    xvfb-run dbus-run-session --config-file=${pkgs.dbus}/share/dbus-1/session.conf -- \
      python3 ${../tests/check_xapp_settings.py} ${pkgs.xdg-desktop-portal-xapp} crash
    cd "$out/patched"
    xvfb-run dbus-run-session --config-file=${pkgs.dbus}/share/dbus-1/session.conf -- \
      python3 ${../tests/check_xapp_settings.py} ${patched} alive
  ''

{ pkgs }:
pkgs.writeShellScriptBin "sekka-background" ''
  # A scope keeps the caller's terminal, environment, working directory and
  # exit status. Descendants remain in the slice even if they reset nice.
  # Nested commands share the existing scope; headless hosts need no manager.
  if [ -e /etc/sekka/background-scheduling ] &&
     [ -n "''${XDG_RUNTIME_DIR:-}" ] && [ -S "$XDG_RUNTIME_DIR/systemd/private" ] &&
     ! ${pkgs.gnugrep}/bin/grep -q '/app-background.slice/' /proc/self/cgroup; then
    exec ${pkgs.systemd}/bin/systemd-run --user --scope --collect --quiet \
      --slice=app-background.slice --expand-environment=no -- "$@"
  fi
  exec "$@"
''

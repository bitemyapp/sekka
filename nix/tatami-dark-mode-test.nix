{
  pkgs,
  nixpkgs,
  tatamiModule,
}:
let
  evaluated = nixpkgs.lib.nixosSystem {
    system = pkgs.stdenv.hostPlatform.system;
    modules = [
      tatamiModule
      { programs.tatami.enable = true; }
    ];
  };
  profiles = pkgs.symlinkJoin {
    name = "tatami-dark-mode-profiles";
    paths = evaluated.config.programs.dconf.packages;
  };
  # The previous behavior: a dark default without a lock, beneath user-db:user.
  unlockedDb = pkgs.runCommand "tatami-unlocked-dconf-db" { nativeBuildInputs = [ pkgs.dconf ]; } ''
    mkdir keyfiles
    cat > keyfiles/appearance <<'EOF'
    [org/gnome/desktop/interface]
    color-scheme='prefer-dark'
    EOF
    dconf compile "$out" keyfiles
  '';
  unlockedProfile = pkgs.writeText "tatami-unlocked-profile" ''
    user-db:user
    file-db:${unlockedDb}
  '';
  userProfile = pkgs.writeText "tatami-user-profile" "user-db:user\n";
in
pkgs.runCommand "tatami-dark-mode"
  {
    nativeBuildInputs = [
      pkgs.dbus
      pkgs.dconf
      pkgs.glib
      pkgs.xvfb-run
    ];
  }
  ''
    export HOME="$TMPDIR/home"
    export XDG_CONFIG_HOME="$HOME/.config"
    export XDG_RUNTIME_DIR="$TMPDIR/runtime"
    export GSETTINGS_SCHEMA_DIR=${pkgs.gsettings-desktop-schemas}/share/gsettings-schemas/${pkgs.gsettings-desktop-schemas.name}/glib-2.0/schemas
    mkdir -p "$XDG_CONFIG_HOME" "$XDG_RUNTIME_DIR"
    chmod 700 "$XDG_RUNTIME_DIR"
    export GDK_BACKEND=x11
    export GSETTINGS_BACKEND=dconf
    export GIO_EXTRA_MODULES=${pkgs.dconf.lib}/lib/gio/modules
    export XDG_DATA_DIRS=${pkgs.dconf}/share:${pkgs.gsettings-desktop-schemas}/share
    export TATAMI_TEST_USER_PROFILE=${userProfile}
    export TATAMI_TEST_UNLOCKED_PROFILE=${unlockedProfile}
    export TATAMI_TEST_PROFILE=${profiles}/etc/dconf/profile/tatami
    export TATAMI_TEST_PORTAL=${pkgs.xdg-desktop-portal-gtk}/libexec/xdg-desktop-portal-gtk
    xvfb-run -a dbus-run-session --config-file=${pkgs.dbus}/share/dbus-1/session.conf -- bash ${../tests/test_tatami_dark_mode.sh}
    touch "$out"
  ''

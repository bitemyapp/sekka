# Component integration: the real GTK window and PipeWire in private D-Bus,
# display and audio sessions. No host devices or installed desktop are used.
{ pkgs, sound }:
pkgs.runCommand "tatami-sound-integration"
  {
    timeout = 180;
    nativeBuildInputs = [
      sound
      pkgs.pipewire
      pkgs.wireplumber
      pkgs.pulseaudio
      pkgs.dbus
      pkgs.gobject-introspection
      pkgs.at-spi2-core
      pkgs.xorg-server
      pkgs.xdotool
      pkgs.imagemagick
      (pkgs.python3.withPackages (p: [
        p.pyatspi
        p.pygobject3
      ]))
    ];
  }
  ''
    export HOME=$TMPDIR/home
    export XDG_RUNTIME_DIR=$TMPDIR/runtime
    export XDG_CONFIG_HOME=$HOME/.config
    export XDG_CONFIG_DIRS=${../desktop/tatami/config}
    mkdir -p "$HOME" "$XDG_RUNTIME_DIR" "$XDG_CONFIG_HOME/wireplumber/wireplumber.conf.d" "$out"
    chmod 700 "$XDG_RUNTIME_DIR"
    cat > "$XDG_CONFIG_HOME/wireplumber/wireplumber.conf.d/no-hardware.conf" <<'CONF'
    wireplumber.profiles = {
      main = {
        monitor.alsa = disabled
        monitor.bluez = disabled
        monitor.v4l2 = disabled
        monitor.libcamera = disabled
      }
    }
    CONF
    export XDG_DATA_DIRS=${pkgs.at-spi2-core}/share
    export FONTCONFIG_FILE=${
      pkgs.makeFontsConf {
        fontDirectories = [
          pkgs.nerd-fonts.jetbrains-mono
          pkgs.dejavu_fonts
        ];
      }
    }
    export DISPLAY=:91
    export GDK_BACKEND=x11 GSK_RENDERER=gl GTK_A11Y=atspi
    export LIBGL_ALWAYS_SOFTWARE=1
    export LIBGL_DRIVERS_PATH=${pkgs.mesa}/lib/dri
    export __EGL_VENDOR_LIBRARY_FILENAMES=${pkgs.mesa}/share/glvnd/egl_vendor.d/50_mesa.json
    export PULSE_SERVER=unix:$XDG_RUNTIME_DIR/pulse/native
    Xvfb :91 -screen 0 1024x900x24 -nolisten tcp &
    xvfb=$!
    trap 'kill $xvfb 2>/dev/null || true' EXIT
    dbus-run-session --config-file=${pkgs.dbus}/share/dbus-1/session.conf -- python3 ${../tests/test_sound.py} "$out"
  ''

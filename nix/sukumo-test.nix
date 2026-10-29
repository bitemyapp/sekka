# Sukumo's own tests (desktop/aizome/sukumo), as Sidestep's Linux CI ran
# them before Sukumo moved here: the engine's and toolkit's unit tests, the
# toolkit's tests through the null render thread and offscreen, and its
# Wayland tests under a headless sway, with the fonts, clipboard tools and
# virtual keyboard they look for. The frame and image budgets only measure,
# so they stay with `scripts/headless-wayland` runs by hand.
{ pkgs, fonts }:
let
  inherit (pkgs) lib;
  root = ../desktop/aizome/sukumo;
  src = lib.fileset.toSource {
    inherit root;
    fileset = lib.fileset.unions [
      (root + "/Cargo.toml")
      (root + "/Cargo.lock")
      (root + "/crates")
      (root + "/fixtures")
      (root + "/scripts")
    ];
  };
  # Fonts for shaping, fallback and emoji, with fontconfig's own aliases:
  # the configuration makeFontsConf writes looks for them in /etc/fonts,
  # which a sandbox hasn't got, and without them `monospace` means nothing.
  fontsConf = pkgs.writeText "sukumo-fonts.conf" ''
    <?xml version="1.0"?>
    <!DOCTYPE fontconfig SYSTEM "urn:fontconfig:fonts.dtd">
    <fontconfig>
      <include>${
        pkgs.makeFontsConf {
          fontDirectories = with pkgs; [
            fonts
            dejavu_fonts
            noto-fonts
            noto-fonts-cjk-sans
            noto-fonts-color-emoji
          ];
        }
      }</include>
      <include>${pkgs.fontconfig.out}/etc/fonts/conf.d</include>
    </fontconfig>
  '';
in
pkgs.rustPlatform.buildRustPackage {
  pname = "sukumo-tests";
  version = "0.0.1";
  inherit src;
  cargoLock.lockFile = root + "/Cargo.lock";
  nativeBuildInputs = with pkgs; [
    sway
    dbus
    grim
    wl-clipboard
    wtype
  ];
  dontCargoBuild = true;
  dontCargoInstall = true;
  doCheck = true;
  # Tests, not a program: no debug info, as in a developer's runs.
  CARGO_PROFILE_DEV_DEBUG = "0";
  checkPhase = ''
    runHook preCheck
    export HOME=$TMPDIR
    export XKB_CONFIG_ROOT=${pkgs.xkeyboard_config}/share/X11/xkb
    # fontconfig is opened at run time (sidestep-engine's src/system.rs).
    export LD_LIBRARY_PATH=${lib.makeLibraryPath [ pkgs.fontconfig ]}
    export FONTCONFIG_FILE=${fontsConf}
    # stdenv names binutils' `size` in $SIZE, which headless-wayland would
    # take for the output's size.
    unset WAYLAND_DISPLAY DISPLAY SIZE
    flags=(--offline --locked -j "$NIX_BUILD_CORES")
    cargo test "''${flags[@]}" -p sidestep-engine --lib
    cargo test "''${flags[@]}" -p sidestep-ui --lib \
      --test headless --test offscreen --test desktop --test scrolling --test drag_out
    # The Wayland tests under a headless sway, with a session bus of their
    # own: Nixpkgs' sway starts one itself when there's none, from /etc's
    # configuration, which a sandbox hasn't got. They pass by skipping where
    # sway isn't running, so a skip fails here.
    status=0
    dbus-run-session --config-file=${pkgs.dbus}/share/dbus-1/session.conf -- \
      sh scripts/headless-wayland cargo test "''${flags[@]}" -p sidestep-ui \
      --test wayland_system --test wayland_desktop >wayland.log 2>&1 || status=$?
    cat wayland.log
    [ "$status" = 0 ] || exit "$status"
    if grep -q "^skipped:" wayland.log; then
      echo "sukumo-tests: the Wayland tests skipped; sway didn't start" >&2
      exit 1
    fi
    runHook postCheck
  '';
  installPhase = ''
    touch $out
  '';
}

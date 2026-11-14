# Kakehashi (apps/kakehashi), the USB writer, built for Linux: its window
# and the helper that writes a drive. macOS and Windows builds come from
# .github/workflows/kakehashi.yml.
{ pkgs }:
let
  inherit (pkgs) lib;
  source = ../apps/kakehashi;
  # What GPUI links against.
  libraries = with pkgs; [
    fontconfig
    freetype
    libxkbcommon
    wayland
    libxcb
    libx11
    vulkan-loader
  ];
  # What GPUI opens at run time rather than links.
  runtimeLibraries = with pkgs; [
    vulkan-loader
    wayland
    libxkbcommon
    libGL
    libx11
    libxcb
    libxcursor
    libxi
    libxrandr
  ];
  # What the helper runs. pkexec resets PATH, so their directory is built
  # in (KAKEHASHI_TOOL_PATH in core/src/platform/mod.rs).
  tools = with pkgs; [
    util-linux
    coreutils
    udisks
  ];
in
{
  package = pkgs.rustPlatform.buildRustPackage {
    pname = "kakehashi";
    version = "0.1.0";
    # Sekka's paths, as the core's path dependency on Aizome's typestate
    # crate names them; nothing else of Aizome's.
    src = lib.fileset.toSource {
      root = ../.;
      fileset = lib.fileset.unions [
        (source + "/Cargo.toml")
        (source + "/Cargo.lock")
        (source + "/core")
        (source + "/write")
        (source + "/helper")
        (source + "/app")
        (source + "/linux")
        ../desktop/aizome/crates/aizome-typestate
      ];
    };
    postUnpack = ''
      sourceRoot="$sourceRoot/apps/kakehashi"
    '';
    cargoLock.lockFile = source + "/Cargo.lock";
    nativeBuildInputs = [
      pkgs.pkg-config
      pkgs.makeWrapper
    ];
    buildInputs = libraries;
    env.KAKEHASHI_TOOL_PATH = lib.makeBinPath tools;
    nativeCheckInputs = [ pkgs.clippy ];
    # Only the person's press of the "Erase …?" dialog's OK starts the
    # helper; app/clippy.toml denies GPUI's input dispatch, which would let
    # the window's code press OK itself. Only that lint fails the build, so
    # a toolchain's new lints don't. The window's library and binary forbid
    # it, so nothing in them can allow it, and no other file names it.
    #
    # The default check's `cargo test` runs the doctests too, their
    # compile_fail error codes checked.
    preCheck = ''
      source ${./checked-doctests.sh}
      cargo clippy --offline --locked -p kakehashi --all-targets -- \
        -A clippy::all -D clippy::disallowed_methods
      found=$(grep -rnE --include='*.rs' '(allow|expect|warn|deny|forbid)\([^)]*disallowed_methods' app core write helper \
        | sed -E 's/^([^:]*):[0-9]+: */\1:/' | sort)
      expected=$(sort <<'EOF'
      app/src/lib.rs:#![forbid(clippy::disallowed_methods)]
      app/src/main.rs:#![forbid(clippy::disallowed_methods)]
      EOF
      )
      if [ "$found" != "$expected" ]; then
        echo "The input-dispatch ban's lint levels differ from the allowed ones:"
        printf 'found:\n%s\nallowed:\n%s\n' "$found" "$expected"
        exit 1
      fi
    '';
    # Sekka's app icon, from the brand's generated logos.
    postInstall = ''
      install -Dm644 linux/computer.sekka.Kakehashi.desktop -t $out/share/applications
      install -Dm644 ${../docs/brand/logos/sekka-app-icon.svg} \
        $out/share/icons/hicolor/scalable/apps/computer.sekka.Kakehashi.svg
    '';
    # The helper stays unwrapped beside the window, which finds it there.
    postFixup = ''
      wrapProgram $out/bin/kakehashi --prefix LD_LIBRARY_PATH : ${lib.makeLibraryPath runtimeLibraries}
    '';
    meta = {
      description = "Write Sekka, or any disk image, to a USB drive and read it back";
      mainProgram = "kakehashi";
      license = with lib.licenses; [
        mit
        asl20
      ];
    };
  };

  shell = pkgs.mkShell {
    packages = [
      pkgs.cargo
      pkgs.rustc
      pkgs.clippy
      pkgs.rustfmt
      pkgs.pkg-config
    ];
    buildInputs = libraries;
    LD_LIBRARY_PATH = lib.makeLibraryPath runtimeLibraries;
    KAKEHASHI_TOOL_PATH = lib.makeBinPath tools;
  };
}

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
rec {
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
    # The default check's `cargo test` runs the doctests too, their
    # compile_fail error codes checked.
    preCheck = ''
      source ${./checked-doctests.sh}
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

  # The window's two tests that a plain `cargo test` leaves ignored: its
  # real start_write worker and elevate::run against a fake pkexec, through
  # app/repro_ktl_03.sh and app/repro_ktl_04.sh as a developer runs them.
  # From the package's own source and vendored crates, as a check of its
  # own, so the package builds where user namespaces can't nest.
  #
  # The scripts mask /run/wrappers/bin/pkexec with bubblewrap over the
  # root they are run in. A build sandbox's root has no /run and can't take
  # one, so an outer bubblewrap gives them a root that can: the store
  # read-only, the build directory, an empty /run, and the /bin/sh the fake
  # pkexec names. Neither the real pkexec nor the helper runs.
  elevationTests = package.overrideAttrs (old: {
    pname = "kakehashi-elevation-tests";
    nativeCheckInputs = (old.nativeCheckInputs or [ ]) ++ [ pkgs.bubblewrap ];
    # The package's check has run the other tests.
    preCheck = "";
    dontBuild = true;
    doCheck = true;
    checkPhase = ''
      runHook preCheck
      set -o pipefail
      # Sekka's root, where the scripts say to run them.
      cd ../..
      for script in repro_ktl_03 repro_ktl_04; do
        bwrap --unshare-all --die-with-parent \
          --proc /proc --dev /dev --tmpfs /tmp \
          --ro-bind /nix/store /nix/store \
          --bind "$NIX_BUILD_TOP" "$NIX_BUILD_TOP" \
          --dir /run --symlink ${pkgs.runtimeShell} /bin/sh \
          -- bash apps/kakehashi/app/$script.sh 2>&1 | tee "$NIX_BUILD_TOP/$script.log"
        # The scripts choose their tests by name, and Cargo passes when a
        # name matches none.
        if ! grep -Eq '^test result: ok\. [1-9][0-9]* passed' "$NIX_BUILD_TOP/$script.log"; then
          echo "error: $script.sh ran no test" >&2
          exit 1
        fi
      done
      runHook postCheck
    '';
    installPhase = ''
      touch $out
    '';
    dontFixup = true;
  });

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

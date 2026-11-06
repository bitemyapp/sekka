# Sekka's installer for Apple Silicon Macs, as far as Nix builds it.
#
# The app, its helper and the Recovery app are macOS programs, built on a Mac
# with packaging/build-app.sh. What Nix builds, on any system:
#
# - checks: the whole workspace's tests and the packaging scripts' tests,
#   on Linux (the AppKit code is compiled only for macOS);
# - tool: sekka-mac-tool, for catalogs, signatures and the ledger;
# - icon: AppIcon.icns, from the brand's application icon;
# - source: this workspace as the Mac build takes it, with its lock.
{
  pkgs,
  # Sekka's application icon (docs/brand/logos/sekka-app-icon.svg).
  appIcon,
}:
let
  inherit (pkgs) lib;
  src = lib.fileset.toSource {
    root = ./.;
    fileset = lib.fileset.unions [
      ./Cargo.toml
      ./Cargo.lock
      ./crates
      ./packaging
      (lib.fileset.maybeMissing ./engine/source-lock.json)
    ];
  };
  common = {
    version = "0.1.0";
    inherit src;
    cargoLock.lockFile = ./Cargo.lock;
  };
in
{
  tool = pkgs.rustPlatform.buildRustPackage (
    common
    // {
      pname = "sekka-mac-tool";
      cargoBuildFlags = [
        "-p"
        "sekka-mac-tool"
      ];
      doCheck = false;
      meta.mainProgram = "sekka-mac-tool";
    }
  );

  checks = pkgs.rustPlatform.buildRustPackage (
    common
    // {
      pname = "sekka-mac-installer-checks";
      nativeBuildInputs = [
        pkgs.python3
        pkgs.coreutils
      ];
      # Build nothing to install; test everything.
      buildPhase = "true";
      checkPhase = ''
        runHook preCheck
        cargo test --offline --locked --workspace -j $NIX_BUILD_CORES
        python3 -I -m unittest discover -s packaging/tests
        runHook postCheck
      '';
      installPhase = "touch $out";
      doCheck = true;
    }
  );

  icon =
    pkgs.runCommand "AppIcon.icns"
      {
        nativeBuildInputs = [
          pkgs.librsvg
          pkgs.libicns
        ];
      }
      ''
        for size in 16 32 48 128 256 512 1024; do
          rsvg-convert -w $size -h $size ${appIcon} -o icon_$size.png
        done
        png2icns $out icon_*.png
      '';

  source = pkgs.runCommand "sekka-mac-installer-source.tar.gz" { } ''
    tar --sort=name --owner=0 --group=0 --numeric-owner --mtime=@1 \
      -czf $out -C ${src} .
  '';
}

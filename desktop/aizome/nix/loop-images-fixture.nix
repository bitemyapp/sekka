# Only the real-UDisks test executable, linked to the same aizome-sys source.
{ self, pkgs }:
self.packages.${pkgs.stdenv.hostPlatform.system}.aizome.overrideAttrs (old: {
  pname = "aizome-loop-images-fixture";
  # This compiles with a Cargo command of its own, in another profile and
  # target directory than the package's, so the package's dependency build
  # (workspace.nix) has nothing for it: no crates to restore, and no check
  # that only first-party crates compiled, which every crate here would
  # fail. Nor the package's check phase, which tests the compositor the
  # package builds and this doesn't; this binary's tests run in the VM.
  preBuild = "";
  postBuild = "";
  doCheck = false;
  buildPhase = ''
    runHook preBuild
    cargo test --offline --release -j "$NIX_BUILD_CORES" \
      -p aizome-sys --test disks_real --no-run
    runHook postBuild
  '';
  installPhase = ''
    mkdir -p "$out/bin"
    find target/release/deps -maxdepth 1 -type f -executable -name 'disks_real-*' \
      -exec install -m755 '{}' "$out/bin/aizome-loop-images-fixture" \;
  '';
  postInstall = "";
  postFixup = "";
})

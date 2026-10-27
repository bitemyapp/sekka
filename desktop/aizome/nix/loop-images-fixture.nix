# Only the real-UDisks test executable, linked to the same aizome-sys source.
{ self, pkgs }:
self.packages.${pkgs.stdenv.hostPlatform.system}.aizome.overrideAttrs (old: {
  pname = "aizome-loop-images-fixture";
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

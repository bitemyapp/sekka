# Read-only snapshot assertion binary linked to the exact product's aizome-sys.
{ self, pkgs }:
self.packages.${pkgs.stdenv.hostPlatform.system}.aizome.overrideAttrs (old: {
  pname = "aizome-encrypted-volumes-fixture";
  buildPhase = ''
    runHook preBuild
    cargo test --offline --release -j "$NIX_BUILD_CORES" -p aizome-sys --test disks_encrypted_real --no-run
    runHook postBuild
  '';
  installPhase = ''
    mkdir -p "$out/bin"
    find target/release/deps -maxdepth 1 -type f -executable -name 'disks_encrypted_real-*' \
      -exec install -m755 '{}' "$out/bin/aizome-encrypted-volumes-fixture" \;
  '';
  postInstall = "";
  postFixup = "";
})

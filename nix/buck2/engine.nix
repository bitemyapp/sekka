{ pkgs, rustOverlay ? null }:
let
  rust = import ./nightly.nix { inherit pkgs rustOverlay; };
  platform = pkgs.makeRustPlatform { cargo = rust; rustc = rust; };
in
platform.buildRustPackage {
  pname = "sekka-buck2";
  version = "2026-10-15-local-content-store";
  src = pkgs.fetchFromGitHub {
    owner = "facebook";
    repo = "buck2";
    rev = "9a1876786f32806f6b65fc51f4c1c3077efc38a5";
    hash = "sha256-6uFr2rrn4of1BcG21VjpgsY8I1xlCRaKrDnbqcKoxPE=";
  };
  patches = [
    ./patches/local-content-store.patch
    ./patches/external-startup-config.patch
    ./patches/http2-goaway.patch
  ];
  # Cargo loads Git patch manifests even when the lock records them as unused.
  # Remove this dead patch in both source and vendoring; no resolved dep changes.
  cargoPatches = [ ./patches/drop-unused-bindgen.patch ];
  cargoHash = "sha256-8S4vUe7K3sSxO1egCnS/20wJyndpz1q5nuTkyYoGhT8=";
  nativeBuildInputs = [ pkgs.clang pkgs.mold pkgs.pkg-config pkgs.git ];
  cargoBuildFlags = [ "--bin" "buck2" ];
  RUSTFLAGS = "--cfg tokio_unstable -C linker=clang -C link-arg=-fuse-ld=mold";
  CARGO_PROFILE_RELEASE_INCREMENTAL = "false";
  # Startup identity must work from subdirectories and must never retain stale
  # credentials/store settings. These tests need no services or reflink mount.
  # The materializer suite is separately qualified on real Btrfs/Aizome outputs.
  doCheck = true;
  checkPhase = ''
    runHook preCheck
    cargo test --frozen --release --target ${pkgs.stdenv.hostPlatform.rust.rustcTarget} --jobs "$NIX_BUILD_CORES" -p buck2_common init::tests
    cargo test --frozen --release --target ${pkgs.stdenv.hostPlatform.rust.rustcTarget} --jobs "$NIX_BUILD_CORES" -p buck2_client_ctx test_relative_re_tls_uses_project_root_from_subdirectory
    runHook postCheck
  '';
  meta = {
    description = "Buck2 with verified, bounded local reflink artifact materialization";
    homepage = "https://github.com/facebook/buck2";
    license = with pkgs.lib.licenses; [ mit asl20 ];
    mainProgram = "buck2";
    platforms = [ "x86_64-linux" ];
  };
}

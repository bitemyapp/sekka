{ pkgs, rustOverlay ? null }:
let
  rust = import ./nightly.nix { inherit pkgs rustOverlay; };
  platform = pkgs.makeRustPlatform { cargo = rust; rustc = rust; };
in
platform.buildRustPackage ({
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
    ./patches/reproducible-generation.patch
    ./patches/private-config-telemetry.patch
    ./patches/local-live-views.patch
    ./patches/cas-recovery.patch
    ./patches/critical-path-priority.patch
    ./patches/incremental-source-cache.patch
    ./patches/output-pipeline.patch
    ./patches/presence-consistent-hits.patch
    ./patches/content-store-unlocked-views.patch
    ./patches/content-store-advisory-writes.patch
    ./patches/bytestream-empty-source.patch
    ./patches/startup-identity-config-layer.patch
    ./patches/local-cas-downloads.patch
    ./patches/local-cas-outputs.patch
    ./patches/apfs-local-cas.patch
  ];
  # Cargo loads Git patch manifests even when the lock records them as unused.
  # Remove this dead patch in both source and vendoring; no resolved dep changes.
  cargoPatches = [
    ./patches/drop-unused-bindgen.patch
    ./patches/priority-dependencies.patch
    ./patches/parallel-sync-dependencies.patch
  ];
  cargoHash = "sha256-bQiNyy/k6DA+Wwt4kHJn13KMcmZ1ouos3Dwn/I+4Sn4=";
  nativeBuildInputs = [ pkgs.clang ]
    ++ pkgs.lib.optionals pkgs.stdenv.hostPlatform.isLinux [ pkgs.mold ]
    ++ [ pkgs.pkg-config pkgs.git ];
  cargoBuildFlags = [ "--bin" "buck2" ];
  RUSTFLAGS = "--cfg tokio_unstable -C linker=clang"
    + pkgs.lib.optionalString pkgs.stdenv.hostPlatform.isLinux " -C link-arg=-fuse-ld=mold";
  CARGO_PROFILE_RELEASE_INCREMENTAL = "false";
  # jemalloc otherwise runs CPUID during configure and emits different pointer
  # layouts on 48-bit and 57-bit hosts. Use its portable x86-64 fallback.
  # Auth/config and download-integrity regressions run on every builder.
  # Reflink tests additionally run on Btrfs; ext4 builders explicitly report
  # that coverage limit. Real Aizome extent sharing is qualified separately.
  doCheck = true;
  checkPhase = ''
    runHook preCheck
    for filter in init::tests argv::tests legacy_configs::telemetry::tests legacy_configs::args::tests settings::args::tests; do
      cargo test --frozen --release --target ${pkgs.stdenv.hostPlatform.rust.rustcTarget} --jobs "$NIX_BUILD_CORES" -p buck2_common -- "$filter"
    done
    for filter in immediate_config::tests common::tests; do
      cargo test --frozen --release --target ${pkgs.stdenv.hostPlatform.rust.rustcTarget} --jobs "$NIX_BUILD_CORES" -p buck2_client_ctx -- "$filter"
    done
    cargo test --frozen --release --target ${pkgs.stdenv.hostPlatform.rust.rustcTarget} --jobs "$NIX_BUILD_CORES" -p buck2_re_configuration -- privacy_tests
    cargo test --frozen --release --target ${pkgs.stdenv.hostPlatform.rust.rustcTarget} --jobs "$NIX_BUILD_CORES" -p buck2_server_ctx -- ctx::telemetry_tests
    cargo test --frozen --release --target ${pkgs.stdenv.hostPlatform.rust.rustcTarget} --jobs "$NIX_BUILD_CORES" -p buck2 --lib -- tests::cli_
    cargo test --frozen --release --target ${pkgs.stdenv.hostPlatform.rust.rustcTarget} --jobs "$NIX_BUILD_CORES" -p host_sharing --lib
    cargo test --frozen --release --target ${pkgs.stdenv.hostPlatform.rust.rustcTarget} --jobs "$NIX_BUILD_CORES" -p buck2_execute_impl --lib -- local_content_store::download_tests
    cargo test --frozen --release --target ${pkgs.stdenv.hostPlatform.rust.rustcTarget} --jobs "$NIX_BUILD_CORES" -p buck2_execute_impl --lib -- local_content_store::linux::tests::hint_
    cargo test --frozen --release --target ${pkgs.stdenv.hostPlatform.rust.rustcTarget} --jobs "$NIX_BUILD_CORES" -p buck2_execute_impl --lib -- local_content_store::linux::tests::local_cas_
    for filter in local_content_store::linux::tests::store_rejects_aliased_index_files_without_changing_targets \
      output_pipeline::tests output_watcher::tests prepared_outputs::tests; do
      cargo test --frozen --release --target ${pkgs.stdenv.hostPlatform.rust.rustcTarget} --jobs "$NIX_BUILD_CORES" -p buck2_execute_impl --lib -- "$filter"
    done
    cargo test --frozen --release --target ${pkgs.stdenv.hostPlatform.rust.rustcTarget} --jobs "$NIX_BUILD_CORES" -p remote_execution -- client::tests
    for filter in re::error::tests re::remote_action_result::tests re::presence::tests entry::tests; do
      cargo test --frozen --release --target ${pkgs.stdenv.hostPlatform.rust.rustcTarget} --jobs "$NIX_BUILD_CORES" -p buck2_execute -- "$filter"
    done
    if [ "$(uname -s)" = Darwin ]; then
      cargo test --frozen --release --target ${pkgs.stdenv.hostPlatform.rust.rustcTarget} --jobs "$NIX_BUILD_CORES" -p buck2_execute_impl --lib -- materializers::apfs::tests
    elif [ "$(stat -f -c %T "$TMPDIR")" = btrfs ]; then
      cargo test --frozen --release --target ${pkgs.stdenv.hostPlatform.rust.rustcTarget} --jobs "$NIX_BUILD_CORES" -p buck2_execute_impl --lib -- local_content_store::linux::tests
    else
      echo "Reflink-store execution not checked: this builder is not on Btrfs."
    fi
    runHook postCheck
  '';
  meta = {
    description = "Buck2 with verified, bounded local reflink artifact materialization";
    homepage = "https://github.com/facebook/buck2";
    license = with pkgs.lib.licenses; [ mit asl20 ];
    mainProgram = "buck2";
    platforms = [ "x86_64-linux" ] ++ pkgs.lib.platforms.darwin;
  };
} // pkgs.lib.optionalAttrs pkgs.stdenv.hostPlatform.isx86_64 {
  JEMALLOC_SYS_WITH_LG_VADDR = "57";
})

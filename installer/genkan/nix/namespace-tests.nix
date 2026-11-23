# The backend's namespace tests (rust/tests), which a plain `cargo test`
# leaves ignored: the real disk revalidation and install preparation with
# fake tools only. They are outside the helper's source, so they build here
# from a source of their own, as storage-tests.nix builds the library's.
{ pkgs }:
let
  target = pkgs.stdenv.hostPlatform.rust.rustcTarget;
  # The only tool path the backend built here has: a scratch directory the
  # tests fill with fakes, so there is no wipefs, parted or mkfs to reach.
  # Not under /run or /sys, which the tests mount over.
  fakeTools = "/tmp/genkan-fake-tools";
in
pkgs.rustPlatform.buildRustPackage {
  pname = "genkan-namespace-tests";
  version = "0.1.0";
  # The backend at its place in Sekka, beside the typestate crate it uses.
  src = pkgs.lib.fileset.toSource {
    root = ../../..;
    fileset = pkgs.lib.fileset.unions [
      ../rust/Cargo.toml
      ../rust/Cargo.lock
      ../rust/reference.nix
      ../rust/src
      ../rust/tests
      ../../../desktop/aizome/crates/aizome-typestate
    ];
  };
  postUnpack = ''
    sourceRoot="$sourceRoot/installer/genkan/rust"
  '';
  cargoLock.lockFile = ../rust/Cargo.lock;
  CALAMARES_TOOL_PATH = fakeTools;
  buildPhase = ''
    runHook preBuild
    cargo test --release --offline --locked --no-default-features --no-run --target ${target} \
      --test revalidation --test deferred_route
    runHook postBuild
  '';
  doCheck = false;
  installPhase = ''
    mkdir -p $out/bin
    for name in revalidation deferred_route; do
      for test in target/${target}/release/deps/$name-*; do
        if [ -x "$test" ] && [ -f "$test" ]; then
          cp "$test" $out/bin/genkan-''${name//_/-}-tests
        fi
      done
      test -x $out/bin/genkan-''${name//_/-}-tests
    done
  '';
  passthru = { inherit fakeTools; };
}

# `nix develop .#bazel`: Bazel and the tools around it, from the root lock
# (docs/bazel-build-plan.md). Bazel's repository rules build the toolchains
# from nix/bazel/toolchains.nix themselves; the shell names the same store
# paths, so entering it fetches them ahead and a `--profile` roots them.
{ pkgs }:
let
  toolchains = import ./toolchains.nix { inherit pkgs; };
  # Only the checked-in settings (.bazelrc): not the host's or the user's,
  # nor the Nixpkgs system rc, whose one setting is the server's JDK.
  bazel = pkgs.writeShellScriptBin "bazel" ''
    exec ${pkgs.bazel_9}/bin/bazel --nosystem_rc --nohome_rc \
      --server_javabase=${pkgs.jdk21_headless} "$@"
  '';
in
pkgs.mkShell {
  packages = [
    bazel
    pkgs.bazel-buildtools
    pkgs.python3
    # Aizome's Rust, for build/bazel/generate.py's `cargo metadata` and for
    # editors (rust-analyzer).
    toolchains.rust
  ];
  SEKKA_BAZEL_RUST_TOOLCHAIN = toolchains.rust;
  SEKKA_BAZEL_NATIVE_ENV = toolchains.native;
}

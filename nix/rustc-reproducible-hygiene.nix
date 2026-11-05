# Keep the root-locked compiler and bootstrap dependencies, fixing only hygiene
# serialization: parallel frontend interning must not change crate metadata.
# Preserve the normal compiler's std, targets and rustc-dev outputs so tools
# linking rustc_private use the same compiler rather than the unpatched driver.
{ pkgs }:
let
  unwrapped = pkgs.rustc.unwrapped.overrideAttrs (old: {
    patches = (old.patches or [ ]) ++ [ ./patches/rustc-reproducible-hygiene.patch ];
  });
  rustc = pkgs.rustc.override { rustc-unwrapped = unwrapped; };
  cargo = pkgs.cargo;
  rustPlatform = pkgs.makeRustPlatform { inherit cargo rustc; };
in
{
  inherit rustc cargo rustPlatform;
  clippy = pkgs.clippy.override { inherit rustc rustPlatform; };
  rustfmt = pkgs.rustfmt.override { inherit rustc rustPlatform; };
}

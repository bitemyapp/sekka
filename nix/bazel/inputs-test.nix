# Bazel's native environment builds what Aizome's package builds with:
# nix/bazel/aizome-inputs.nix must name the package's buildInputs, its
# CMake and glslc, and its settings for whisper.cpp
# (desktop/aizome/nix/package.nix). Evaluation fails, naming the difference,
# if one changes without the other.
{ pkgs, aizome }:
let
  inherit (pkgs) lib;
  bazel = import ./aizome-inputs.nix { inherit pkgs; };
  # The package keeps its inputs' development outputs, as stdenv does.
  paths = map (p: (lib.getDev p).outPath);
  missing = a: b: lib.subtractLists (paths b) (paths a);
  check =
    what: list:
    lib.assertMsg (
      list == [ ]
    ) "nix/bazel/aizome-inputs.nix: ${what}: ${lib.concatStringsSep " " list}";
  settings = lib.filter (name: bazel.env.${name} != (aizome.${name} or null)) (
    lib.attrNames bazel.env
  );
in
assert check "missing the package's buildInputs" (missing aizome.buildInputs bazel.buildInputs);
assert check "buildInputs the package hasn't" (missing bazel.buildInputs aizome.buildInputs);
assert check "nativeBuildInputs the package hasn't" (
  missing bazel.nativeBuildInputs aizome.nativeBuildInputs
);
assert check "settings that differ from the package's" settings;
pkgs.runCommand "bazel-native-inputs" { } "touch $out"

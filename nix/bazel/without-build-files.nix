# Bazel's BUILD files (build/bazel/generate.py) sit in the crates' directories
# but aren't sources of any Nix build: a package's fileset leaves out the ones
# under the directories given, so its store path, and every package built
# from it, stays the same whatever they say.
{ lib }:
dirs: fileset:
lib.fileset.difference fileset (
  lib.fileset.unions (map (lib.fileset.fileFilter (file: file.name == "BUILD.bazel")) dirs)
)

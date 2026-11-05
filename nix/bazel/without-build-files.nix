# Build graph metadata is not a native package source. Keep this exclusion
# shared by the package filesets, without broadening any package's inputs.
{ lib }:
dirs: fileset:
lib.fileset.difference fileset (
  lib.fileset.unions (map (lib.fileset.fileFilter (file:
    builtins.elem file.name [
      "BUILD" "BUILD.bazel" "BUCK" "PACKAGE"
      ".buckconfig" ".buckrootconfig" "buckal.toml"
    ]
  )) dirs)
)

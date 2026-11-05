# Pre-realized repositories: Bazel never needs a Nix daemon with this rc.
# Keep this source identical to MODULE.bazel's rules_nixpkgs archive overrides.
{ pkgs }:
let
  toolchains = import ./toolchains.nix { inherit pkgs; };
  rulesNixpkgs = builtins.fetchTarball {
    url = "https://github.com/tweag/rules_nixpkgs/archive/58151f3745a1abe09277a6a8d0c58449018d2baa.tar.gz";
    sha256 = "0rhmsl61kb6fiy6p0547p2gmpzawlpiz5j3ly5ngb5fsadfgzd54";
  };
  info = import (rulesNixpkgs + "/toolchains/cc/cc.nix") {
    ccPkgs = pkgs;
    ccType = "ccTypeExpression";
    ccExpr = toolchains.cc;
  };
  ccInfo = pkgs.runCommand "sekka-bazel-cc-info-repository" { } ''
    mkdir $out
    ln -s ${info}/CC_TOOLCHAIN_INFO $out/CC_TOOLCHAIN_INFO
    touch $out/REPO.bazel
    echo 'exports_files(["CC_TOOLCHAIN_INFO"])' > $out/BUILD.bazel
  '';
  repositories = {
    nix_cc_info = ccInfo;
    nix_rust = toolchains.rust;
    nix_native = toolchains.native;
    nix_test_env = toolchains.testEnv;
  };
in
pkgs.writeText "sekka-bazel-repositories.bazelrc" (
  pkgs.lib.concatStringsSep "\n" (
    pkgs.lib.mapAttrsToList (name: path: "common --override_repository=${name}=${path}") repositories
  ) + "\n"
)

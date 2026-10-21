# `nix run .#publish-nightly X86_ISO_DIRECTORY ARM_ISO_DIRECTORY`: uploads the ISOs built from this
# checkout to https://downloads.sekka.computer/nightly/ (Cloudflare R2 bucket
# sekka-downloads) as a new build, and keeps the last seven. The nightly
# workflow runs it with a token that can write that bucket alone, in
# AWS_ACCESS_KEY_ID and AWS_SECRET_ACCESS_KEY (DEVELOPERS.md).
#
# Each build has a folder of its own, nightly/builds/DATE-REVISION/, with
# both ISOs, their SHA256SUMS, flake.lock and manifest.json, none of which ever
# change. nightly/builds.json lists the kept builds, newest first; it is
# written last, and then everything else under nightly/ is removed.
{ pkgs }:
pkgs.writeShellApplication {
  name = "sekka-publish-nightly";
  runtimeInputs = with pkgs; [
    awscli2
    coreutils
    git
    gnugrep
    jq
  ];
  text = builtins.readFile ../scripts/publish-nightly.sh;
  meta.description = "Upload both nightly Sekka ISOs to https://downloads.sekka.computer/nightly/ and keep the last seven";
}

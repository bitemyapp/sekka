# Check the serialized store that will go on the ISO, including missing paths,
# file contents, symlink targets and executable bits. Never use the host DB.
{ pkgs }:
pkgs.writeShellApplication {
  name = "sekka-verify-squashfs";
  runtimeInputs = [
    pkgs.coreutils
    pkgs.diffutils
    pkgs.nix
    pkgs.squashfsTools
  ];
  text = ''
    if [[ $# != 2 ]]; then
      echo 'Usage: sekka-verify-squashfs IMAGE REGISTRATION' >&2
      exit 2
    fi
    image=$(realpath "$1")
    registration=$(realpath "$2")
    test -s "$registration"
    verification=$(mktemp -d)
    trap 'chmod -R u+w "$verification"; rm -rf "$verification"' EXIT
    mkdir -p "$verification/nix"
    unsquashfs -no-progress -d "$verification/nix/store" "$image"
    cmp "$registration" "$verification/nix/store/nix-path-registration"
    store="local?root=$verification"
    nix-store --store "$store" --load-db < "$registration"
    nix --extra-experimental-features nix-command store verify \
      --store "$store" --all --no-trust
  '';
}

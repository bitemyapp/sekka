# Upstream builds squashfs inside the ISO derivation. Verify those exact bytes
# before xorriso packages them; a separate optional check could be skipped.
{ pkgs, iso }:
let
  verify = import ./verify-squashfs.nix { inherit pkgs; };
in
iso.overrideAttrs (old: {
  squashfsCommand = old.squashfsCommand + ''
    ${verify}/bin/sekka-verify-squashfs "$imgPath" "$closureInfo/registration"
  '';
})

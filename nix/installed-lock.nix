# The installed flake adds Sekka itself to the image's external dependency lock.
# Dirty development images retain their exact store source; release images pin
# the GitHub commit. Both can subsequently update the original main input.
{ self }:
let
  base = builtins.fromJSON (builtins.readFile ../flake.lock);
  external = base.nodes.${base.root}.inputs;
  locked =
    if self ? rev then
      {
        type = "github";
        owner = "bitemyapp";
        repo = "sekka";
        rev = self.rev;
        inherit (self) narHash lastModified;
      }
    else
      {
        type = "path";
        path = toString self.outPath;
        inherit (self) narHash;
      };
in
base
// {
  nodes = base.nodes // {
    ${base.root} = {
      inputs = external // {
        sekka = "sekka";
      };
    };
    sekka = {
      inherit locked;
      original = {
        type = "github";
        owner = "bitemyapp";
        repo = "sekka";
        ref = "main";
      };
      inputs = builtins.mapAttrs (name: _: [ name ]) external;
    };
  };
}

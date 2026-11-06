# The installed flake adds Sekka itself to the image's external dependency lock.
# Dirty development images retain their exact store source; release images pin
# the GitHub commit. Both then follow `ref` (the nightly branch for PCs; the
# Apple Silicon integration branch for Macs until that platform is on main),
# and the inputs the installed flake names (templates/flake.nix.in) follow the
# Sekka they are at. The default arguments give exactly the lock the PC media
# have always shipped.
{
  self,
  # The branch updates follow: the `original` ref, which must match the
  # installed flake.nix's `inputs.sekka.url` for Nix to accept the lock.
  ref ? "nightly",
}:
let
  base = builtins.fromJSON (builtins.readFile ../flake.lock);
  external = base.nodes.${base.root}.inputs;
  # Only inputs named by templates/flake.nix.in belong at the installed root.
  # Implementation dependencies such as Superluminal and Tracy stay
  # beneath Sekka. New Sekka inputs must not silently change the installer's
  # target-flake contract (checked by the actual backend in template-lock).
  followed = builtins.listToAttrs (
    builtins.map (name: {
      inherit name;
      value = external.${name};
    }) [
      "nixpkgs"
      "determinate"
      "fh"
      "applications"
      "ai-apps"
      "omp"
    ]
  );
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
  # Sekka's nodes, under the installed root: a follows path in them names an
  # input from Sekka's root, so it now starts from Sekka, as Nix writes it.
  rebased = builtins.mapAttrs (
    _: node:
    if node ? inputs then
      node
      // {
        inputs = builtins.mapAttrs (
          _: input: if builtins.isList input then [ "sekka" ] ++ input else input
        ) node.inputs;
      }
    else
      node
  ) (removeAttrs base.nodes [ base.root ]);
in
base
// {
  nodes = rebased // {
    ${base.root} = {
      # Every input it names follows Sekka's own.
      inputs =
        builtins.mapAttrs (name: _: [
          "sekka"
          name
        ]) followed
        // {
          sekka = "sekka";
        };
    };
    sekka = {
      inherit locked;
      original = {
        type = "github";
        owner = "bitemyapp";
        repo = "sekka";
        inherit ref;
      };
      inputs = external;
    };
  };
}

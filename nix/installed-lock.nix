# The installed flake adds Sekka itself to the image's external dependency lock.
# Dirty development images retain their exact store source; release images pin
# the GitHub commit. Both then follow the nightly branch, and the inputs the
# installed flake names (templates/flake.nix.in) follow the Sekka they are at.
#
# Sekka comes through Git, one commit deep, as Nix locks
# `git+https://github.com/bitemyapp/sekka?ref=nightly&shallow=1`: Nix keeps
# the clone, so each update downloads only what changed. The content hash is
# the same as for GitHub's tarball of that commit.
{ self }:
let
  repository = {
    type = "git";
    url = "https://github.com/bitemyapp/sekka";
    ref = "nightly";
    shallow = true;
  };
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
      repository
      // {
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
      original = repository;
      inputs = external;
    };
  };
}

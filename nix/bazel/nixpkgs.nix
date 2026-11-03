# Bazel's Nixpkgs (`<nixpkgs>` in rules_nixpkgs' repository rules): the root
# flake's own `nixpkgs` input, read from flake.lock, so Bazel's tools and
# libraries are the store paths the flake's packages and shells use. Fetched
# by its NAR hash, the source is the path the flake already has.
let
  lock = builtins.fromJSON (builtins.readFile ../../flake.lock);
  locked = lock.nodes.${lock.nodes.${lock.root}.inputs.nixpkgs}.locked;
  url =
    if locked.type == "tarball" then
      locked.url
    else if locked.type == "github" then
      "https://github.com/${locked.owner}/${locked.repo}/archive/${locked.rev}.tar.gz"
    else
      throw "nix/bazel/nixpkgs.nix: unsupported nixpkgs lock type ${locked.type}";
  source = builtins.fetchTarball {
    inherit url;
    sha256 = locked.narHash;
  };
in
# As the flake's legacyPackages: no user configuration or overlays.
args:
import source (
  {
    config = { };
    overlays = [ ];
  }
  // args
)

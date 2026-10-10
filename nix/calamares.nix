# Genkan's package retains its existing executable and policy IDs for compatibility.
{ pkgs, source }: import "${source}/nix/package.nix" { inherit pkgs; }

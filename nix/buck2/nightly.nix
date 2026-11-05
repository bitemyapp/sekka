# Engine-build compiler only. Workspace crates still use pkgs.rustc.
{ pkgs, rustOverlay ? null }:
let
  lock = builtins.fromJSON (builtins.readFile ../../flake.lock);
  input = node: name:
    let ref = lock.nodes.${node}.inputs.${name};
    in if builtins.isString ref then ref
       else builtins.foldl' input lock.root ref;
  node = input (input lock.root "omp") "rust-overlay";
  source = builtins.fetchTree lock.nodes.${node}.locked;
  overlay = if rustOverlay != null then rustOverlay else import source.outPath;
in
(pkgs.extend overlay).rust-bin.nightly."2026-07-05".minimal

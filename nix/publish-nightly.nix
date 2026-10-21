# Workstation-only publication, gated on the exact ISO's reviewed evidence.
{ pkgs }:
pkgs.writeShellApplication {
  name = "sekka-publish-nightly";
  runtimeInputs = with pkgs; [
    awscli2
    git
    nix
    python3
  ];
  text = ''
    exec python3 ${../scripts/publish-nightly.py} "$@"
  '';
  meta.description = "Validate and publish a locally qualified Sekka ISO, retaining seven builds";
}

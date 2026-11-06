# Sekka's skills for coding agents, and the command that keeps them in each
# user's home (module.nix). The skills are plain files in the store; the
# command copies them, so a copy's SKILL.md is a regular file, which Codex
# requires.
{
  lib,
  runCommand,
  writeShellApplication,
  coreutils,
  diffutils,
  findutils,
  util-linux,
}:
let
  source = lib.fileset.toSource {
    root = ./skills;
    fileset = ./skills;
  };
  skills = runCommand "sekka-agent-skills-data" { } ''
    mkdir -p $out/share/sekka
    cp -R ${source} $out/share/sekka/agent-skills
  '';
in
writeShellApplication {
  name = "sekka-agent-skills";
  runtimeInputs = [
    coreutils
    diffutils
    findutils
    util-linux
  ];
  text = builtins.replaceStrings [ "@skills@" ] [ "${skills}/share/sekka/agent-skills" ] (
    builtins.readFile ./sekka-agent-skills
  );
  passthru = { inherit skills; };
  meta = {
    description = "Keep Sekka's agent skills in the user's home, and restore Sekka's versions";
    mainProgram = "sekka-agent-skills";
  };
}

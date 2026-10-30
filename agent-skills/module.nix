# Sekka's skills for coding agents in every user's home (package.nix): copies
# in ~/.agents/skills, read by Codex, ChatGPT, OpenCode, oh-my-pi, Gemini CLI
# and most others, linked from ~/.claude/skills for Claude, which reads only
# its own folder.
{
  config,
  lib,
  pkgs,
  ...
}:
let
  package = pkgs.callPackage ./package.nix { };
in
{
  options.calamares.agentSkills.enable = lib.mkOption {
    type = lib.types.bool;
    default = true;
    description = ''
      Keep Sekka's skills for coding agents (sekka, yukimi, aizome and
      tatami) in each user's ~/.agents/skills, linked from ~/.claude/skills.
      Each login and system switch installs new skills and updates the
      unchanged ones; skills a user changed or removed stay as they are, and
      `sekka-agent-skills restore` brings back Sekka's versions. Turning
      this off leaves copies already made in place.
    '';
  };
  config = lib.mkIf config.calamares.agentSkills.enable {
    environment.systemPackages = [ package ];
    # nixos-activation runs this at each login, and for everyone logged in
    # at every switch. A failure here must not fail an update.
    system.userActivationScripts.sekkaAgentSkills = ''
      ${lib.getExe package} sync || echo "sekka-agent-skills: the agent skills were not updated" >&2
    '';
  };
}

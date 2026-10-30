# Nisshi's reader as the package installs it, through real polkit and
# pkexec: someone who isn't an administrator reads only their own journal;
# with polkit's yes (a rule standing in for an administrator's password)
# the reader runs as root and reads the system's; without it, and with no
# agent to ask, pkexec refuses before the reader runs. The reader as root
# reads only under /var/log, and takes no option but its own.
{
  pkgs,
  self,
}:
pkgs.testers.runNixOSTest {
  name = "aizome-nisshi";
  globalTimeout = 300;
  nodes.machine = {
    imports = [ self.nixosModules.default ];
    programs.aizome.enable = true;
    users.users.alice = {
      isNormalUser = true;
      uid = 1000;
    };
    users.users.bob = {
      isNormalUser = true;
      uid = 1001;
    };
    # Instead of alice's password dialog: polkit says yes to her alone.
    security.polkit.extraConfig = ''
      polkit.addRule(function (action, subject) {
        if (action.id == "org.aizome.Nisshi.read-system-logs" && subject.user == "alice") {
          return polkit.Result.YES;
        }
      });
    '';
    environment.etc."nisshi-not-a-log".text = "not for the reader";
    virtualisation.memorySize = 2048;
  };
  testScript = builtins.readFile ./vm-nisshi-test.py;
}

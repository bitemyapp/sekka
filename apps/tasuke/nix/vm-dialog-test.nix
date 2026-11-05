# SPDX-License-Identifier: MIT OR Apache-2.0
# The Assistant window through the real password dialog (vm-desktop.nix's
# machine): alice opens the window and asks; the scripted model proposes a
# read-only command, one needing approval, one as root and one Tasuke
# refuses. The test presses Run and Run as administrator in the window,
# Aizome's polkit agent asks for alice's password, the test types it, and
# the helper runs the command as root. Unlike vm-test.nix, no polkit rule
# stands in for the password.
{
  pkgs,
  tasuke,
  tasukeModule,
  aizomeModule,
}:
pkgs.testers.runNixOSTest {
  name = "tasuke-dialog";
  globalTimeout = 600;
  nodes.machine = {
    imports = [
      (import ./vm-desktop.nix {
        inherit
          pkgs
          tasuke
          tasukeModule
          aizomeModule
          ;
      })
    ];
    environment.etc."tasuke-test/marker".text = "tasuke-marker-6271\n";
    systemd.tmpfiles.rules = [
      "d /var/lib/tasuke-test 0755 root root -"
      "d /tmp/tasuke-test 1777 root root -"
    ];
  };
  testScript = builtins.readFile ./vm-desktop.py + builtins.readFile ./vm-dialog-test.py;
}

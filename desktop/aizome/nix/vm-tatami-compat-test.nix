# Co-install the real Tatami module and switch sessions with a lingering
# user manager. Pass tatamiModule explicitly so this integration test does
# not make Tatami a dependency of the Aizome desktop.
{
  pkgs,
  self,
  tatamiModule,
  package ? null,
}:
let
  inherit (pkgs) lib;
  networkTest = import ./vm-network-auth-test.nix { inherit pkgs self package; };
  chooseSession = pkgs.writeShellScript "compatibility-session" ''
    if [ "$(cat /run/compatibility-session 2>/dev/null)" = tatami ]; then
      exec /run/current-system/sw/bin/uwsm start -e -D Hyprland:Tatami -N Tatami -- /run/current-system/sw/bin/tatami session
    fi
    exec /run/current-system/sw/bin/aizome-session
  '';
in
networkTest.extend {
  modules = [
    {
      name = lib.mkForce "aizome-tatami-compatibility";
      nodes.machine = {
        imports = [ tatamiModule ];
        programs.tatami.enable = true;
        users.users.alice.linger = true;
        services.greetd.settings.default_session.command =
          lib.mkForce "${pkgs.greetd}/bin/agreety --cmd ${chooseSession}";
        # Tatami normally suppresses the XDG applet. Keep the optional
        # explicit unit available for testing its Aizome-only guard.
        systemd.user.services.nm-applet.wantedBy = lib.mkForce [ ];
      };
      testScript = lib.mkForce (builtins.readFile ./vm-tatami-compat-test.py);
    }
  ];
}

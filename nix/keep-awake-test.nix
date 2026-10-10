{ pkgs, tatami }:
pkgs.testers.runNixOSTest {
  name = "tatami-keep-awake";
  nodes.machine = { lib, ... }: {
    imports = [ ../installer/genkan/rust/system/usb-wake.nix ];
    virtualisation.memorySize = 1024;
    users.users.alice = {
      isNormalUser = true;
      uid = 1000;
      linger = true;
    };
    services.getty.autologinUser = "alice";
    security.polkit.enable = true;
    environment.systemPackages = [ tatami ];
    systemd.user.services.tatami-keep-awake =
      (import ../desktop/tatami/keep-awake.nix {
        inherit lib pkgs;
        tool = tatami;
      })
      // {
        wantedBy = [ "default.target" ];
      };
    # This test isolates logind from the compositor. A running idle daemon
    # must never be used as the evidence that inhibition exists.
    systemd.user.services.tatami-hypridle.serviceConfig.ExecStart =
      "${pkgs.coreutils}/bin/sleep infinity";
    # Never actually suspend the test VM, even if the inhibitor is broken.
    systemd.services.systemd-suspend.serviceConfig.ExecStart =
      lib.mkForce "${pkgs.coreutils}/bin/false";
  };
  testScript = ''
    import json
    machine.start()
    machine.wait_for_unit("user@1000.service")
    machine.wait_until_succeeds("loginctl list-sessions --no-legend | grep 'alice.*seat0'")
    def user(command):
        return machine.succeed("su - alice -c " + __import__('shlex').quote("XDG_RUNTIME_DIR=/run/user/1000 " + command))
    def ready():
        machine.wait_until_succeeds("su - alice -c 'XDG_RUNTIME_DIR=/run/user/1000 tatami awake status' | grep '\"protected\":true'", timeout=30)
    assert not json.loads(user("tatami awake status"))["protected"]
    user("tatami awake on")
    ready()
    assert json.loads(user("tatami indicator idle"))["class"] == "active"
    assert "sleep:idle:handle-lid-switch" in machine.succeed("systemd-inhibit --list --no-pager")
    error = machine.fail("systemctl suspend --check-inhibitors=yes 2>&1")
    assert "Tatami Keep Awake" in error, error
    # Losing either the holder or logind must acquire a fresh real lock.
    user("systemctl --user kill --signal=KILL tatami-keep-awake.service")
    ready()
    machine.succeed("systemctl restart systemd-logind")
    ready()
    # Saved intent returns after a fresh login/boot without any GUI app.
    machine.shutdown()
    machine.start()
    machine.wait_for_unit("user@1000.service")
    machine.wait_until_succeeds("loginctl list-sessions --no-legend | grep 'alice.*seat0'")
    ready()
    user("tatami awake off")
    assert not json.loads(user("tatami awake status"))["protected"]
    user("systemctl --user stop tatami-hypridle.service")
    assert json.loads(user("tatami indicator idle"))["class"] == "inactive"
    # A missing/failed service must report failure, never a reassuring cup.
    user("systemctl --user mask tatami-keep-awake.service")
    machine.fail("su - alice -c 'XDG_RUNTIME_DIR=/run/user/1000 tatami awake on'")
    assert json.loads(user("tatami indicator idle"))["class"] == "warning"
    user("systemctl --user unmask tatami-keep-awake.service")
    user("tatami awake on")
    ready()
    user("tatami awake off")
  '';
}

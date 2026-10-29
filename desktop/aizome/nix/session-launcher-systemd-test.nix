# Real user-manager transactions and the shipped launcher/unit dependencies.
# Only the compositor is a stand-in; gates make startup ordering reproducible.
# This complements the full graphical greeter test, not ISO qualification.
{ pkgs }:
let
  units = pkgs.runCommand "aizome-launcher-test-units" { } ''
    mkdir -p $out/lib/systemd/user
    cp ${../session/aizome-session.target} $out/lib/systemd/user/aizome-session.target
    cp ${../session/aizome-shutdown.target} $out/lib/systemd/user/aizome-shutdown.target
    cp ${../session/aizome.service} $out/lib/systemd/user/aizome.service
    substituteInPlace $out/lib/systemd/user/aizome.service \
      --replace-fail 'ExecStart=/usr/bin/env aizome --backend udev' \
        'ExecStart=${pkgs.python3}/bin/python3 /etc/launcher-test/compositor.py'
  '';
  systemctl = pkgs.writeShellScriptBin "systemctl" ''
    if [ "$*" = '--user --wait start aizome.service' ]; then
      echo $$ > "$TEST_DIR/client-$TEST_LOGIN"
      touch "$TEST_DIR/entered-$TEST_LOGIN"
      while test -f "$TEST_DIR/hold-start"; do sleep .01; done
    fi
    exec ${pkgs.systemd}/bin/systemctl "$@"
  '';
in
pkgs.testers.runNixOSTest {
  name = "aizome-session-launcher-systemd";
  nodes.machine = {
    users.users.alice = {
      isNormalUser = true;
      uid = 1000;
      linger = true;
    };
    services.dbus.enable = true;
    systemd.packages = [ units ];
    environment.systemPackages = [
      pkgs.python3
      pkgs.dbus
      pkgs.util-linux
    ];
    environment.etc."launcher-test/launcher".source = ../session/aizome-session;
    environment.etc."launcher-test/test.py".source = ./session-launcher-systemd-test.py;
    environment.etc."launcher-test/wrapper-path".text = "${systemctl}/bin";
    environment.etc."launcher-test/compositor.py".text = ''
      import os, socket, time
      from pathlib import Path
      root = Path('/home/alice/launcher-test')
      (root / 'compositor-pid').write_text(str(os.getpid()))
      while (root / 'hold-ready').exists():
          time.sleep(.01)
      sock = socket.socket(socket.AF_UNIX, socket.SOCK_DGRAM)
      sock.connect(os.environ['NOTIFY_SOCKET'].replace('@', '\0', 1))
      sock.send(b'READY=1')
      while True:
          time.sleep(1)
    '';
    virtualisation.memorySize = 1024;
  };
  testScript = ''
    machine.start()
    machine.wait_for_unit("user@1000.service")
    print(machine.succeed("su -s /bin/sh alice -c 'XDG_RUNTIME_DIR=/run/user/1000 python3 /etc/launcher-test/test.py'", timeout=120))
    machine.copy_from_machine("/home/alice/launcher-test", "launcher-test")
  '';
}

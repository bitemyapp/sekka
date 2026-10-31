# Real user-manager transactions and the shipped launcher/unit dependencies.
# The compositor and file-manager bus owner are stand-ins; gates make
# startup ordering reproducible. Hako's private-bus tests cover its executable.
# This complements the full graphical greeter test, not ISO qualification.
{ pkgs }:
let
  python = pkgs.python3.withPackages (p: [ p.dbus-next ]);
  units = pkgs.runCommand "aizome-launcher-test-units" { } ''
    mkdir -p $out/lib/systemd/user
    cp ${../session/aizome-session.target} $out/lib/systemd/user/aizome-session.target
    cp ${../session/aizome-shutdown.target} $out/lib/systemd/user/aizome-shutdown.target
    cp ${../session/aizome.service} $out/lib/systemd/user/aizome.service
    cp ${../session/aizome-file-manager.service} $out/lib/systemd/user/aizome-file-manager.service
    substituteInPlace $out/lib/systemd/user/aizome-file-manager.service \
      --replace-fail '@bindir@/aizome-hako --file-manager-service' \
        '${python}/bin/python3 /etc/launcher-test/file-manager.py'
    substituteInPlace $out/lib/systemd/user/aizome.service \
      --replace-fail 'ExecStart=/usr/bin/env aizome --backend udev' \
        'ExecStart=${pkgs.python3}/bin/python3 /etc/launcher-test/compositor.py'
    # Aizome's D-Bus activation files, as the package ships them.
    for service in ${../session/dbus}/*.service; do
      install -Dm644 "$service" "$out/share/dbus-1/services/$(basename "$service")"
      substituteInPlace "$out/share/dbus-1/services/$(basename "$service")" --replace-fail @bindir@ /run/current-system/sw/bin
    done
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
    # Both of the module's routes for the package: its units and, through
    # the system path, its D-Bus activation files.
    systemd.packages = [ units ];
    environment.systemPackages = [
      units
      pkgs.python3
      pkgs.dbus
      pkgs.util-linux
    ];
    environment.etc."launcher-test/launcher".source = ../session/aizome-session;
    environment.etc."launcher-test/test.py".source = ./session-launcher-systemd-test.py;
    environment.etc."launcher-test/wrapper-path".text = "${systemctl}/bin";
    environment.etc."launcher-test/file-manager.py".text = ''
      import asyncio
      from dbus_next.aio import MessageBus

      async def main():
          bus = await MessageBus().connect()
          await bus.request_name('org.freedesktop.FileManager1')
          await bus.wait_for_disconnect()

      asyncio.run(main())
    '';
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

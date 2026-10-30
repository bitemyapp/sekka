# Aizome's processor power meter in a disposable VM: its socket starts the
# root service under its sandbox, and anyone may ask it; a VM has no RAPL
# counters, so it answers nothing. A stand-in then answers on the same
# socket with a package drawing 20 W, reading at most once a second and
# saying when, as the meter does, and Myaku, run as a user, measures the
# processor through it. Component evidence: reading real RAPL counters as
# root needs physical hardware.
{
  pkgs,
  self,
  package ? null,
}:
let
  # A package drawing 20 W, its counter rising from when it started.
  standIn = pkgs.writers.writePython3 "stand-in-power-meter" { } ''
    import os
    import socket
    import time

    PATH = "/run/aizome/power.sock"


    def now_us():
        return time.clock_gettime_ns(time.CLOCK_BOOTTIME) // 1000


    start = now_us()
    os.makedirs("/run/aizome", exist_ok=True)
    if os.path.exists(PATH):
        os.unlink(PATH)
    server = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
    server.bind(PATH)
    os.chmod(PATH, 0o666)
    server.listen()
    # As the meter: a new reading at most once a second, and when it was
    # read on every line.
    read = None
    while True:
        connection, _ = server.accept()
        now = now_us()
        if read is None or now - read >= 1000000:
            read = now
        energy = (read - start) * 20
        connection.sendall(f"package-0 {energy} 0 {read}\n".encode())
        connection.close()
  '';
in
pkgs.testers.runNixOSTest {
  name = "aizome-power-meter";
  globalTimeout = 300;
  nodes.machine =
    { lib, ... }:
    {
      imports = [ self.nixosModules.default ];
      programs.aizome.enable = true;
      programs.aizome.package = lib.mkIf (package != null) package;
      users.users.alice = {
        isNormalUser = true;
        uid = 1000;
      };
      environment.systemPackages = [ pkgs.socat ];
    };
  testScript = ''
    import re

    def alice(command):
        return machine.succeed(f"runuser -u alice -- {command}")

    machine.wait_for_unit("sockets.target")

    with subtest("The meter starts from its socket, as root without capabilities"):
        machine.succeed("systemctl is-active aizome-power-meter.socket")
        assert machine.succeed("stat -c %a /run/aizome/power.sock").strip() == "666"
        answer = alice("socat -t 2 - UNIX-CONNECT:/run/aizome/power.sock < /dev/null")
        assert answer == "", f"a VM has no RAPL counters, yet the meter answered {answer!r}"
        machine.wait_until_succeeds("systemctl is-active aizome-power-meter.service", timeout=10)
        pid = machine.succeed("systemctl show -p MainPID --value aizome-power-meter.service").strip()
        status = machine.succeed(f"cat /proc/{pid}/status")
        print(status)
        assert re.search(r"^Uid:\s+0\s", status, re.M), status
        assert re.search(r"^CapEff:\s+0+$", status, re.M), status
        assert re.search(r"^CapBnd:\s+0+$", status, re.M), status
        assert re.search(r"^NoNewPrivs:\s+1$", status, re.M), status

    with subtest("Myaku, run as a user, measures the processor through the meter"):
        machine.succeed("systemctl stop aizome-power-meter.socket aizome-power-meter.service")
        machine.succeed("systemd-run --unit stand-in-power-meter ${standIn}")
        machine.wait_until_succeeds("socat -t 2 - UNIX-CONNECT:/run/aizome/power.sock < /dev/null | grep -q package-0", timeout=20)
        reading = alice("aizome-myaku --print")
        print(reading)
        watts = re.search(r"^Power: processor ([0-9.]+) W", reading, re.M)
        assert watts, reading
        assert 18 <= float(watts[1]) <= 22, f"the stand-in draws 20 W: {watts[0]}"
  '';
}

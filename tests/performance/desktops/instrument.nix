# The benchmark's own additions to every VM, kept out of the disk footprint
# figures (default.nix evaluates each system without them too): a root
# command channel on a virtio-serial port, the probe tools, and uinput.
{ tools }:
{ lib, pkgs, ... }:
{
  environment.systemPackages = [ tools ];
  boot.kernelModules = [ "uinput" ];
  # Commands from the host harness (bench.py), one bash reading the port.
  # It waits on a read between commands, so it costs nothing at idle.
  systemd.services.perf-agent = {
    description = "Benchmark command channel";
    wantedBy = [ "multi-user.target" ];
    path = with pkgs; [
      bash
      coreutils
      findutils
      gawk
      gnugrep
      gnused
      procps
      systemd
      util-linux
      tools
    ];
    # A virtio-serial port opens once, so the shell reads and writes the
    # same descriptor. Reads return end-of-file while the host is not
    # connected; the shell then exits and the port is opened again.
    script = ''
      while true; do
        while [ ! -e /dev/virtio-ports/perf.agent ]; do sleep 0.2; done
        bash --norc 3<>/dev/virtio-ports/perf.agent <&3 >&3 2>&3 3>&- || true
        sleep 0.5
      done
    '';
    serviceConfig.Restart = "always";
    unitConfig.StartLimitIntervalSec = 0;
  };
}

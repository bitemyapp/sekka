{ pkgs }:
let
  bash = "${pkgs.bash}/bin/bash";
  sleep = "${pkgs.coreutils}/bin/sleep";
  # ananicy names a process after the basename of its argv[0], read when it
  # sees the exec. Each stand-in below is a forked bash that execs itself
  # under a real program's name, so ananicy sees a single exec under it.
  # A process waits until ananicy has set its own nice value before it starts
  # the next, so that one inherits it as a real child would.
  waitNice = value: ''
    until [ "$(${pkgs.coreutils}/bin/cut -d ' ' -f 19 /proc/$$/stat)" = ${value} ]; do
      ${sleep} 0.2
    done
  '';
  compiler = pkgs.writeText "compiler" "while :; do ${sleep} 1; done";
  shell = pkgs.writeText "shell" ''
    ${waitNice "0"}
    ( exec -a rustc ${bash} ${compiler} ) &
    wait
  '';
  # CachyOS's rules give foot nice -4 ("Doc-View").
  terminal = pkgs.writeText "terminal" ''
    ${waitNice "-4"}
    ( exec ${bash} ${shell} ) &
    wait
  '';
  nvme = pkgs.runCommand "nvme.img" { } "${pkgs.coreutils}/bin/truncate -s 64M $out";
in
pkgs.testers.runNixOSTest {
  name = "sekka-tuning";
  nodes.machine =
    { lib, ... }:
    {
      imports = [ ../installer/genkan/rust/system/tuning.nix ];
      calamares.tuning.enable = true;
      # The kernel installed systems use.
      boot.kernelPackages = pkgs.linuxPackages_latest;
      # The test driver reads the kernel log; tuning lowers it to 3.
      boot.consoleLogLevel = lib.mkForce 7;
      virtualisation.memorySize = 1024;
      virtualisation.qemu.options = [
        "-drive file=${nvme},if=none,id=nvme,format=raw,snapshot=on"
        "-device nvme,serial=sekka,drive=nvme"
      ];
    };
  testScript = ''
    machine.wait_for_unit("ananicy-cpp.service")

    # Ubuntu's lowlatency-kernel boot options take effect.
    cmdline = machine.succeed("cat /proc/cmdline")
    assert "preempt=full" in cmdline and "rcu_nocbs=all" in cmdline, cmdline
    assert "(full)" in machine.succeed("cat /sys/kernel/debug/sched/preempt")
    machine.succeed("journalctl -k | grep 'Offload RCU callbacks from CPUs'")

    # mq-deadline for NVMe, BFQ for rotating disks (QEMU's virtio disk).
    machine.wait_until_succeeds("grep -F '[mq-deadline]' /sys/block/nvme0n1/queue/scheduler", timeout=30)
    assert machine.succeed("cat /sys/block/vda/queue/rotational").strip() == "1"
    machine.wait_until_succeeds("grep -F '[bfq]' /sys/block/vda/queue/scheduler", timeout=30)

    # ps shows "-" for the nice value of a SCHED_IDLE process; stat has it.
    def wait_nice(pid, value):
        machine.wait_until_succeeds(f"[ \"$(cut -d ' ' -f 19 /proc/{pid}/stat)\" = {value} ]", timeout=30)

    def started(pattern, parent=None):
        query = f"pgrep -f '{pattern}'" if parent is None else f"pgrep -P {parent} -f '{pattern}'"
        machine.wait_until_succeeds(query, timeout=30)
        return machine.succeed(query).strip()

    # A terminal keeps -4, the shell in it runs at 0 and a compiler at 3.
    machine.succeed("systemd-run --unit=terminal ${bash} -c '( exec -a foot ${bash} ${terminal} ) & wait'")
    terminal = started("^foot ")
    wait_nice(terminal, -4)
    shell = started("${shell}", terminal)
    wait_nice(shell, 0)
    compiler = started("^rustc ", shell)
    wait_nice(compiler, 3)
    assert "SCHED_OTHER" in machine.succeed(f"chrt -p {compiler}")

    # A compiler in a Nix build inherits SCHED_IDLE and the idle I/O class from
    # nix-daemon. The rule changes only its nice value, which SCHED_IDLE ignores.
    machine.succeed(
        "systemd-run --unit=build ${pkgs.util-linux}/bin/chrt --idle 0 "
        + "${pkgs.util-linux}/bin/ionice -c 3 ${pkgs.coreutils}/bin/nice -n 16 "
        + "${bash} -c '( exec -a rustc ${bash} ${compiler} ) & wait'"
    )
    build = started("^rustc ", machine.succeed("systemctl show -P MainPID build").strip())
    wait_nice(build, 3)
    assert "SCHED_IDLE" in machine.succeed(f"chrt -p {build}")
    assert machine.succeed(f"ionice -p {build}").strip() == "idle"
  '';
}

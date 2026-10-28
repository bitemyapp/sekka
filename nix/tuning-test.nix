{ pkgs, aizomeSource }:
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
  probe = pkgs.runCommand "scheduling-probe" { nativeBuildInputs = [ pkgs.stdenv.cc ]; } ''
    mkdir -p $out/bin
    cc -Wall -Wextra -Werror -pthread ${../tests/scheduling-probe.c} -o $out/bin/probe
    for name in chrome nix codex node steam fossilize_replay wineserver sekka-known-game; do
      cp $out/bin/probe $out/bin/$name
    done
  '';
  background = import ./scheduling/background.nix { inherit pkgs; };
  launch = import ./scheduling/aizome-launch-probe.nix {
    inherit pkgs;
    source = aizomeSource;
  };
  launchChild = pkgs.writeShellScript "aizome-launch-child" ''
    set -eu
    test "$PWD" = /tmp
    test "$AIZOME_LAUNCH_TEST" = preserved
    test "$1" = 'literal $value'
    exec ${probe}/bin/probe
  '';
  argvProbe = pkgs.writeShellScript "scheduling-argv" ''
    set -eu
    test "$PWD" = /tmp
    test "$SCHED_MARKER" = preserved
    test "$1" = 'literal $value'
    exit 37
  '';
  nestedJob = pkgs.writeShellScript "nested-harness" ''
    ${sleep} 3
    exec ${probe}/bin/codex jobs ${probe}/bin/probe
  '';
  steamWrapper = pkgs.writeShellScript "steam-game-wrapper" ''
    ${waitNice "-10"}
    ${probe}/bin/probe &
    wait
  '';
  shaderTree = pkgs.writeText "shader-tree" ''
    ${waitNice "16"}
    ${probe}/bin/probe &
    wait
  '';
  shaderWrapper = pkgs.writeShellScript "steam-shader-wrapper" ''
    exec -a fossilize_replay ${bash} ${shaderTree}
  '';
in
pkgs.testers.runNixOSTest {
  name = "sekka-tuning";
  nodes.machine =
    { lib, ... }:
    {
      imports = [ ../installer/genkan/rust/system/tuning.nix ];
      calamares.tuning.enable = true;
      services.ananicy.extraRules = [
        {
          name = "sekka-known-game";
          type = "Game";
        }
      ];
      # The kernel installed systems use.
      boot.kernelPackages = pkgs.linuxPackages_latest;
      # The test driver reads the kernel log; tuning lowers it to 3.
      boot.consoleLogLevel = lib.mkForce 7;
      virtualisation.memorySize = 1024;
      users.users.demo = {
        isNormalUser = true;
        uid = 1000;
        linger = true;
      };
      environment.systemPackages = [ pkgs.python3 ];
      virtualisation.qemu.options = [
        "-drive file=${nvme},if=none,id=nvme,format=raw,snapshot=on"
        "-device nvme,serial=sekka,drive=nvme"
      ];
    };
  testScript = ''
    import shlex

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

    def user(command):
        return "runuser -u demo -- env XDG_RUNTIME_DIR=/run/user/1000 " + \
            "DBUS_SESSION_BUS_ADDRESS=unix:path=/run/user/1000/bus sh -c " + shlex.quote(command)

    def all_threads(pid, policy=None, nice=None, io=None):
        code = f"""
    import os, pathlib, subprocess
    tasks = list(pathlib.Path('/proc/{pid}/task').iterdir())
    assert len(tasks) >= 5, tasks
    for task in tasks:
        tid = int(task.name)
        if {policy!r} is not None:
            assert os.sched_getscheduler(tid) == {policy!r}, (tid, os.sched_getscheduler(tid))
        if {nice!r} is not None:
            assert os.getpriority(os.PRIO_PROCESS, tid) == {nice!r}, (tid, os.getpriority(os.PRIO_PROCESS, tid))
        if {io!r} is not None:
            assert subprocess.check_output(['ionice', '-p', str(tid)], text=True).strip() == {io!r}
    """
        machine.wait_until_succeeds("python3 -c " + shlex.quote(code), timeout=30)

    with subtest("existing Nix workers all receive idle CPU and I/O policy"):
        machine.succeed("systemctl stop ananicy-cpp")
        machine.succeed("systemd-run --unit=nix-probe ${probe}/bin/nix")
        nix = machine.succeed("systemctl show -P MainPID nix-probe").strip()
        all_threads(nix, policy=0)
        machine.succeed("systemd-run --unit=chrome-probe ${probe}/bin/chrome retitle")
        chrome = machine.succeed("systemctl show -P MainPID chrome-probe").strip()
        all_threads(chrome, nice=0)
        # Negative control: the original daemon changes the main thread,
        # while workers created before it started retain normal policy.
        machine.succeed("systemd-run --unit=unpatched-ananicy ${pkgs.ananicy-cpp}/bin/ananicy-cpp start")
        machine.wait_until_succeeds(f"chrt -p {nix} | grep SCHED_IDLE")
        control = f"import os,pathlib; p='{nix}'; assert any(os.sched_getscheduler(int(t.name)) == 0 for t in pathlib.Path('/proc/'+p+'/task').iterdir())"
        machine.succeed("python3 -c " + shlex.quote(control))
        machine.succeed("systemctl stop unpatched-ananicy")
        machine.succeed("systemctl start ananicy-cpp")
        all_threads(nix, policy=5, io="idle")
        all_threads(chrome, nice=-4)

    def steam_job(name, executable):
        machine.succeed(f"systemd-run --unit={name} --uid=demo --property=KillMode=process ${probe}/bin/steam jobs {executable}")
        launcher = machine.succeed(f"systemctl show -P MainPID {name}").strip()
        all_threads(launcher, nice=-5, policy=0, io="best-effort: prio 0")
        machine.succeed(f"kill -USR1 {launcher}")
        child = started(".", launcher)
        return launcher, child

    with subtest("unknown Steam games and known games receive the same priority"):
        steam, game = steam_job("steam-game", "${probe}/bin/probe")
        all_threads(game, nice=-10, policy=0, io="best-effort: prio 0")
        _, known = steam_job("steam-known", "${probe}/bin/sekka-known-game")
        all_threads(known, nice=-10)
        # Node's generic background rule must not override a Steam game.
        _, runtime = steam_job("steam-runtime", "${probe}/bin/node")
        all_threads(runtime, nice=-10, policy=0, io="best-effort: prio 0")
        _, wine = steam_job("steam-wine", "${probe}/bin/wineserver")
        all_threads(wine, nice=-12)
        _, wrapper = steam_job("steam-wrapper", "${steamWrapper}")
        wait_nice(wrapper, -10)
        wrapped_game = started(".", wrapper)
        all_threads(wrapped_game, nice=-10)

    with subtest("Steam games fork new processes without exec"):
        machine.succeed(f"kill -USR1 {game}")
        forked_game = started(".", game)
        wait_nice(forked_game, -10)
        # This probe deliberately lowered its parent before forking.
        wait_nice(game, 0)
        machine.succeed(f"renice -n -10 -p {game}")

    with subtest("Steam preserves stronger thread nice values and realtime policy"):
        tid = machine.succeed(f"ls /proc/{game}/task | grep -vx {game} | head -1").strip()
        machine.succeed(f"renice -n -15 -p {tid}; chrt -f -p 10 {tid}")
        # Lower the creator so its new thread inherits 0. Waiting for the
        # daemon to restore -10 proves it processed the event before checking
        # that the stronger thread survived that same all-thread application.
        machine.succeed(f"renice -n 0 -p {game}; kill -USR2 {game}")
        machine.wait_until_succeeds(f"test $(ls /proc/{game}/task | wc -l) -eq 6")
        wait_nice(game, -10)
        wait_nice(tid, -15)
        assert "SCHED_FIFO" in machine.succeed(f"chrt -p {tid}")
        # Restore this sleeping fixture thread for the all-thread assertions.
        machine.succeed(f"chrt -o -p 0 {tid}; renice -n -10 -p {tid}")
        all_threads(game, nice=-10, policy=0)

    with subtest("Steam background shader processing and its children stay idle"):
        _, shader = steam_job("steam-shader", "${probe}/bin/fossilize_replay")
        all_threads(shader, nice=16, policy=5, io="idle")
        machine.succeed(f"kill -USR1 {shader}")
        shader_child = started(".", shader)
        wait_nice(shader_child, 16)
        assert "SCHED_IDLE" in machine.succeed(f"chrt -p {shader_child}")
        _, shader_shell = steam_job("steam-shader-wrapper", "${shaderWrapper}")
        wait_nice(shader_shell, 16)
        shader_helper = started("${probe}/bin/probe", shader_shell)
        all_threads(shader_helper, nice=16, policy=5, io="idle")

    with subtest("Steam ancestry survives launcher exit and late thread creation"):
        machine.succeed(f"kill -KILL {steam}")
        machine.wait_until_succeeds(f"test $(ps -o ppid= -p {game}) -eq 1")
        machine.succeed(f"renice -n 0 -p {game}; kill -USR2 {game}")
        machine.wait_until_succeeds(f"test $(ls /proc/{game}/task | wc -l) -eq 7")
        all_threads(game, nice=-10)

    with subtest("daemon restart classifies existing Steam trees"):
        machine.succeed("systemctl stop ananicy-cpp")
        machine.succeed(f"renice -n 0 -p {known}; chrt -i -p 0 {known}; ionice -c 3 -p {known}")
        machine.succeed("systemctl start ananicy-cpp")
        all_threads(known, nice=-10, policy=0, io="best-effort: prio 0")

    with subtest("late Nix threads are classified too"):
        machine.succeed(f"kill -USR2 {nix}")
        machine.wait_until_succeeds(f"test $(ls /proc/{nix}/task | wc -l) -eq 6")
        all_threads(nix, policy=5, io="idle")

    with subtest("a retitled Chrome child is classified without exec"):
        machine.succeed(f"kill -USR1 {chrome}")
        child = started(".", chrome)
        wait_nice(child, -4)

    with subtest("a shell wrapper's second exec gets its browser rule"):
        machine.succeed("systemd-run --unit=chrome-wrapper ${bash} -c 'exec ${probe}/bin/chrome'")
        wrapped = machine.succeed("systemctl show -P MainPID chrome-wrapper").strip()
        all_threads(wrapped, nice=-4)

    with subtest("interpreted harnesses override the generic Node idle rule"):
        machine.succeed("systemd-run --unit=node-harness ${probe}/bin/node /tmp/node_modules/@google/gemini-cli/cli.js")
        node = machine.succeed("systemctl show -P MainPID node-harness").strip()
        all_threads(node, policy=0, nice=-4)

    with subtest("harnesses stay interactive while their jobs enter a low-weight slice"):
        machine.wait_for_unit("user@1000.service")
        machine.succeed(user("systemd-run --user --unit=agent-parent --slice=app.slice ${probe}/bin/codex jobs ${probe}/bin/probe"))
        agent_pid = started("${probe}/bin/codex")
        wait_nice(agent_pid, -4)
        group = machine.succeed(f"cat /proc/{agent_pid}/cgroup").strip()
        assert "/app.slice/agent-parent.service" in group, group
        assert machine.succeed(user("systemctl --user show -P CPUWeight app-background.slice")).strip() == "20"
        machine.succeed(f"kill -USR1 {agent_pid}")
        child = started(".", agent_pid)
        machine.wait_until_succeeds(f"grep -q '/app.slice/app-background.slice/app-sekka-work-' /proc/{child}/cgroup")
        assert machine.succeed(f"cat /proc/{agent_pid}/cgroup").strip() == group
        wait_nice(agent_pid, -4)
        # Even a shell reset to nice 0 cannot escape its ancestor's CPU weight.
        command = "${background}/bin/sekka-background ${background}/bin/sekka-background " + \
            "${argvProbe} 'literal $value'"
        status, output = machine.execute(user("cd /tmp; export SCHED_MARKER=preserved; " + command))
        assert status == 37, (status, output)
        # The job scope follows explicit stops of the original launcher.
        machine.succeed(user("systemctl --user stop agent-parent"))
        machine.wait_until_fails(f"kill -0 {child}")

    with subtest("a nested harness regains interactive priority from a background job"):
        machine.succeed(user("systemd-run --user --unit=nested-parent --slice=app.slice ${probe}/bin/codex jobs ${nestedJob}"))
        parent = machine.succeed(user("systemctl --user show -P MainPID nested-parent")).strip()
        machine.succeed(f"kill -USR1 {parent}")
        nested = started(".", parent)
        machine.wait_until_succeeds(f"grep -q '/app-background.slice/' /proc/{nested}/cgroup")
        machine.wait_until_succeeds(f"grep -q '/app.slice/app-sekka-agent-' /proc/{nested}/cgroup")
        wait_nice(nested, -4)
        machine.succeed(f"kill -USR1 {nested}")
        child = started(".", nested)
        machine.wait_until_succeeds(f"grep -q '/app-background.slice/' /proc/{child}/cgroup")

    with subtest("Nix daemon policy is established before workers start"):
        assert machine.succeed("systemctl show -P CPUSchedulingPolicy nix-daemon").strip() == "5"
        assert machine.succeed("systemctl show -P CPUWeight nix-daemon").strip() == "20"

    with subtest("Aizome keybinding and IPC launches leave the compositor service"):
        machine.succeed(user("systemd-run --user --unit=compositor-probe --slice=session.slice " +
            "-p CPUWeight=1000 ${launch}/bin/aizome-launch-probe ${launchChild} 'literal $value'"))
        machine.wait_until_succeeds("test -s /tmp/aizome-launched-pid")
        pid = machine.succeed("cat /tmp/aizome-launched-pid").strip()
        machine.wait_until_succeeds(f"grep -q '/app.slice/' /proc/{pid}/cgroup")
        machine.wait_until_succeeds(f"test $(readlink /proc/{pid}/exe) = ${probe}/bin/probe")
        group = machine.succeed(f"cat /proc/{pid}/cgroup")
        assert "/session.slice/" not in group, group
        scope = group.strip().split("/")[-1]
        assert scope.endswith(".scope"), group
        assert machine.succeed(user(f"systemctl --user show -P CPUWeight {scope}")).strip() in ("100", "[not set]")
  '';
}

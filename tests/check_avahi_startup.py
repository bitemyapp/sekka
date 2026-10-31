"""Exercise the packaged daemon's startup signal window without host services.

GDB pauses immediately after the real PID-file creation returns. No arbitrary
delay or daemon modification is needed. The same HUP must kill the old package
and leave its PID file, while the patched package must initialize, reload on a
later HUP, and shut down cleanly. A no-early-HUP control checks the harness too.
"""

import argparse
import hashlib
import json
from pathlib import Path
import subprocess


def run_case(args, name, executable, early_hup, expect_failure):
    case = args.output / name
    case.mkdir(parents=True)
    runtime = case / "runtime"
    runtime.mkdir()
    # Only this private user namespace has these accounts and runtime files.
    files = {
        "passwd": "root:x:0:0:root:/tmp:/bin/false\navahi:x:0:0:Avahi:/tmp:/bin/false\n",
        "group": "root:x:0:\navahi:x:0:\n",
        "nsswitch.conf": "passwd: files\ngroup: files\nhosts: files\n",
        "hosts": "127.0.0.1 localhost\n",
        "resolv.conf": "nameserver 192.0.2.1\n",
    }
    for filename, contents in files.items():
        (case / filename).write_text(contents)
    (case / "avahi.conf").write_text(
        "[server]\nhost-name=avahi-startup-test\nuse-ipv4=yes\nuse-ipv6=no\n"
        "enable-dbus=no\n[wide-area]\nenable-wide-area=no\n"
        "[publish]\ndisable-publishing=yes\n"
    )
    # These are exported library symbols, so the packaged, stripped executable
    # can be tested directly. In particular, no signal handler is injected.
    trigger = "signal SIGHUP" if early_hup else "continue"
    (case / "test.gdb").write_text(
        """set pagination off
set confirm off
set startup-with-shell off
set breakpoint pending on
set auto-load safe-path /nonexistent
set debuginfod enabled off
set disable-randomization off
handle SIGHUP nostop noprint pass
handle SIGTERM nostop noprint pass
break daemon_pid_file_create
run
finish
printf "PID_PUBLISHED\\n"
break daemon_signal_init
"""
        + trigger
        + """
python
if gdb.selected_inferior().pid == 0:
    gdb.execute("quit 77")
end
printf "HANDLER_SETUP_REACHED\\n"
finish
disable 2
break avahi_simple_poll_iterate
continue
printf "MAIN_LOOP_REACHED\\n"
disable 3
break daemon_signal_next
signal SIGHUP
finish
printf "QUEUED_SIGNAL=%d\\n", $rax
disable 4
enable 3
continue
printf "RELOAD_RETURNED\\n"
disable 3
signal SIGTERM
"""
    )
    argv = [
        args.bwrap,
        "--unshare-all",
        "--die-with-parent",
        "--new-session",
        "--clearenv",
        "--setenv", "HOME", "/tmp",
        "--setenv", "LC_ALL", "C",
        "--uid", "0",
        "--gid", "0",
        "--ro-bind", "/nix/store", "/nix/store",
        "--proc", "/proc",
        "--dev", "/dev",
        "--tmpfs", "/tmp",
        "--dir", "/etc/avahi/services",
        "--bind", str(case), "/evidence",
        "--bind", str(runtime), "/run",
    ]
    for filename in files:
        argv += ["--ro-bind", str(case / filename), "/etc/" + filename]
    argv += [
        "--chdir", "/evidence",
        args.gdb, "--nx", "--nh", "--batch", "-x", "/evidence/test.gdb",
        "--args", executable, "--no-drop-root", "--no-rlimits", "--debug",
        "-f", "/evidence/avahi.conf",
    ]
    result = subprocess.run(
        argv, stdout=subprocess.PIPE, stderr=subprocess.STDOUT, text=True, timeout=20
    )
    (case / "result.log").write_text(result.stdout)
    pidfile = runtime / "avahi-daemon/pid"
    record = {
        "argv": argv,
        "executable_sha256": hashlib.sha256(Path(executable).read_bytes()).hexdigest(),
        "early_hup": early_hup,
        "expected_startup_failure": expect_failure,
        "exit_status": result.returncode,
        "pidfile_retained": pidfile.exists(),
    }
    (case / "result.json").write_text(json.dumps(record, indent=2) + "\n")
    assert "PID_PUBLISHED" in result.stdout, result.stdout
    if expect_failure:
        assert result.returncode == 77, result.stdout
        assert "Program terminated with signal SIGHUP" in result.stdout, result.stdout
        assert "HANDLER_SETUP_REACHED" not in result.stdout, result.stdout
        assert pidfile.exists() and pidfile.read_text().strip().isdigit(), result.stdout
    else:
        assert result.returncode == 0, result.stdout
        for expected in (
            "HANDLER_SETUP_REACHED",
            "Server startup complete.",
            "MAIN_LOOP_REACHED",
            "QUEUED_SIGNAL=1",
            "Got SIGHUP, reloading.",
            "RELOAD_RETURNED",
            "Got SIGTERM, quitting.",
            "exited normally",
        ):
            assert expected in result.stdout, (expected, result.stdout)
        assert not pidfile.exists(), result.stdout
    print(name + ": expected behavior confirmed")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for option in ("bwrap", "gdb", "original", "patched"):
        parser.add_argument("--" + option, required=True)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    args.output = args.output.resolve()
    run_case(args, "original-normal", args.original, False, False)
    run_case(args, "original-early-hup", args.original, True, True)
    run_case(args, "patched-early-hup", args.patched, True, False)


if __name__ == "__main__":
    main()

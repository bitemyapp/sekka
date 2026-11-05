# The script of nix/terminal-shells-vm-test.nix. PROBES comes from the Nix
# file: for each shell, a file typed into terminals to source.
import json
import shlex
import time

# The name ps gives each shell.
COMM = {"fish": "fish", "bash": "bash", "zsh": "zsh", "nushell": "nu"}
# What each probe reports, besides its terminal, SHLVL and PID.
REPORTS = {
    "fish": ["shell=fish", "interactive", "prompt=starship", "zoxide", "fzf", "ls=eza", "nested=bash"],
    "bash": ["shell=bash", "interactive", "nested=bash"],
    "zsh": [
        "shell=zsh", "interactive", "prompt=starship", "zoxide", "fzf", "ls=eza",
        "suggestions", "newuser=off", "nested=bash",
    ],
    "nushell": ["shell=nu", "interactive", "prompt=starship", "zoxide", "fzf", "banner=off", "nested=bash"],
}
# How each is started.
TERMINALS = [
    ("hotaru", "aizome-hotaru"),
    ("foot", "foot"),
    # One process per window, as the test closes it by its PID.
    ("ghostty", "ghostty --gtk-single-instance=false"),
    ("kitty", "kitty"),
]


def until(machine, what, check, timeout=60, every=0.5):
    """Wait for check() to return something truthy, and return it."""
    deadline = time.monotonic() + timeout
    last = None
    while True:
        try:
            last = check()
        except Exception as e:  # noqa: BLE001 (a read racing a write)
            last = e
        if last and not isinstance(last, Exception):
            return last
        if time.monotonic() > deadline:
            machine.screenshot("failed-" + "".join(c if c.isalnum() else "-" for c in what)[:60])
            raise AssertionError(f"timed out waiting for {what}: {last!r}")
        time.sleep(every)


class Session:
    def __init__(self, machine, desktop, default):
        self.machine, self.desktop, self.default = machine, desktop, default

    # Each desktop's: what its session's commands need, how to wait for it,
    # its windows' PIDs, and how its launcher starts a program.
    def env(self) -> str:
        raise NotImplementedError

    def start(self):
        raise NotImplementedError

    def pids(self) -> set[int]:
        raise NotImplementedError

    def title(self, pid) -> str:
        raise NotImplementedError

    def spawn(self, argv):
        raise NotImplementedError

    def user(self, command):
        """Run a command as demo, in demo's session."""
        status, output = self.machine.execute(
            "runuser -u demo -- env XDG_RUNTIME_DIR=/run/user/1000 "
            "DBUS_SESSION_BUS_ADDRESS=unix:path=/run/user/1000/bus sh -c "
            + shlex.quote(self.env() + "{ " + command + "; } 2>&1")
        )
        if status != 0:
            raise Exception(f"{command!r} failed with {status}:\n{output}")
        return output

    def until(self, what, check, timeout=60):
        return until(self.machine, f"{self.desktop}: {what}", check, timeout)

    def tree(self, pid):
        """The processes under pid, as (PID, name)."""
        out = self.machine.succeed("ps -e -o pid=,ppid=,comm=")
        rows = [line.split(None, 2) for line in out.splitlines() if line.strip()]
        children = {}
        for p, pp, comm in rows:
            children.setdefault(int(pp), []).append((int(p), comm.strip()))
        found, todo = [], [pid]
        while todo:
            for p, comm in children.get(todo.pop(), []):
                found.append((p, comm))
                todo.append(p)
        return found

    def shells(self, pid):
        """The shells under pid, by name."""
        return [comm for _, comm in self.tree(pid) if comm in COMM.values()]

    def switch(self, shell):
        """Activate the system with this shell chosen, as Yukimi's rebuild would."""
        system = "/run/booted-system"
        if shell != self.default:
            system += "/specialisation/" + shell
        self.machine.succeed(system + "/bin/switch-to-configuration test")
        hotaru = self.machine.succeed("cat /etc/xdg/aizome/hotaru-shell").strip()
        assert hotaru.endswith("/bin/" + COMM[shell]), hotaru

    def terminal(self, shell, name, argv):
        m = self.machine
        m.succeed("rm -f /tmp/shell-probe /tmp/shell-probe.part")
        before = self.pids()
        self.spawn(argv)
        pid = self.until(f"a {name} window", lambda: next(iter(self.pids() - before), None), timeout=120)
        want = [COMM[shell]]
        # bash replaced itself (exec), or Hotaru started the shell directly.
        seen = []

        def runs():
            seen[:] = self.shells(pid)
            return seen == want

        try:
            self.until(f"{name} to run {want}", runs)
        except AssertionError as e:
            raise AssertionError(f"{self.desktop}'s {name} runs {seen}, not {want}") from e
        time.sleep(3)
        m.send_chars(f"source {PROBES[shell]}\n")
        m.wait_for_file("/tmp/shell-probe", timeout=60)
        time.sleep(0.5)
        report = m.succeed("cat /tmp/shell-probe").strip().split("\n")
        print(f"{self.desktop} {shell} {name}: {report}")
        m.screenshot(f"{self.desktop}-{shell}-{name}")
        # Typed into this terminal's shell, not another window's.
        assert report[1].startswith("term=") and report[2].startswith("shlvl="), report
        typed = int(report[3].removeprefix("pid="))
        assert typed in [p for p, _ in self.tree(pid)], f"typed into {typed}, not under {name} ({pid})"
        rest = report[:1] + report[4:]
        assert rest == REPORTS[shell], f"{self.desktop}'s {name} in {shell} reports {rest}"
        # The window names the folder, as fish and nushell title it, not the
        # bash that started (Hotaru adds its own name).
        if shell != "bash":
            self.until(f"{name}'s title to name the folder, not {self.title(pid)!r}",
                       lambda: self.title(pid).split(" — ")[0] == "~", timeout=20)
        m.succeed(f"kill {pid}")
        self.until(f"{name} to close", lambda: pid not in self.pids() and not self.shells(pid), timeout=30)

    def login(self, shell):
        """A login bash, as a console or SSH starts, hands over with --login."""
        m = self.machine
        m.succeed(
            "(sleep 60 | runuser -u demo -- script -qec 'env -u SHLVL bash -il' /dev/null) >/dev/null 2>&1 &"
        )
        script = self.until("script", lambda: m.succeed("pgrep -u demo -n -x script || true").strip())
        comm = COMM[shell]

        def child():
            out = m.succeed(f"ps -o comm=,args= --ppid {script} || true").strip()
            return out if out.split(None, 1)[0:1] == [comm] else None

        args = self.until(f"a login {comm}", child)
        login = "-il" if shell == "bash" else "--login"
        assert args.endswith(login), args
        m.succeed("pkill -u demo -x script")

    def offered(self):
        """What Yukimi offers to change; Aizome's own checks it."""

    def check(self):
        self.start()
        self.offered()
        for shell in [self.default] + [s for s in COMM if s != self.default]:
            with subtest(f"{self.desktop}'s terminals open {shell}"):
                self.switch(shell)
                for name, argv in TERMINALS:
                    self.terminal(shell, name, argv)
                self.login(shell)
        self.machine.shutdown()


class Aizome(Session):
    def env(self):
        return "export AIZOME_SOCKET=$(ls /run/user/1000/aizome.*.sock | head -1); "

    def start(self):
        m = self.machine
        m.start()
        m.wait_until_succeeds("ls /run/user/1000/aizome.*.sock", timeout=240)
        m.wait_until_succeeds("pgrep -u demo -x aizome-shell", timeout=60)
        # Annai's welcome greets a new user; close it as they would.
        time.sleep(6)
        for w in self.windows():
            if w.get("app_id") == "org.aizome.Annai":
                self.user(f"aizomectl close {w['id']}")

    def windows(self):
        return json.loads(self.user("aizomectl --json windows")).get("windows", [])

    def pids(self):
        return {w["pid"] for w in self.windows() if w.get("parent") is None}

    def title(self, pid):
        return next(w["title"] for w in self.windows() if w["pid"] == pid)

    def spawn(self, argv):
        self.user("aizomectl spawn -- " + argv)

    def offered(self):
        """Yukimi's Desktops page offers the four shells, fish chosen now."""
        inspect = "/run/user/1000/inspect"
        self.user(f"mkdir -p {inspect} && rm -f {inspect}/org.aizome.Yukimi.*")
        before = self.pids()
        self.spawn(f"env AIZOME_KIT_INSPECT={inspect} aizome-yukimi --page desktops")
        pid = self.until("Yukimi's window", lambda: next(iter(self.pids() - before), None), timeout=120)

        def elements():
            out = self.machine.succeed(
                f'for f in {inspect}/org.aizome.Yukimi.{pid}.*.json; do [ -f "$f" ] && cat "$f" && echo; done; true'
            )
            docs = [json.loads(line) for line in out.splitlines() if line.strip()]
            return [e for d in docs if d["kind"] == "window" for e in d["elements"]]

        def texts():
            return {(e.get("text") or "").lower() for e in elements()}

        offered = {"terminal shell", "fish", "bash", "zsh", "nushell"}
        self.until("Yukimi's Terminal shell choice", lambda: offered <= texts(), timeout=120)
        # Wheel the page until the last of them shows whole, for the picture.
        for _ in range(40):
            e = next(e for e in elements() if (e.get("text") or "").lower() == "nushell")
            if e["visible"] and e["shown"][3] >= e["rect"][3] - 0.5:
                break
            g = next(w for w in self.windows() if w["pid"] == pid)["geometry"]
            self.user(
                f"aizomectl input motion {g['x'] + g['w'] * 0.6:.1f} {g['y'] + g['h'] / 2:.1f} "
                "&& aizomectl input scroll 0 60"
            )
            time.sleep(0.4)
        time.sleep(1)
        self.machine.screenshot("aizome-yukimi-terminal-shell")
        self.machine.succeed(f"kill {pid}")
        self.until("Yukimi to close", lambda: pid not in self.pids(), timeout=30)


class Tatami(Session):
    def env(self):
        return "export HYPRLAND_INSTANCE_SIGNATURE=$(basename /run/user/1000/hypr/*); "

    def start(self):
        m = self.machine
        m.start()
        m.wait_until_succeeds("test -S /run/user/1000/hypr/*/.socket.sock", timeout=240)
        m.wait_until_succeeds(
            "runuser -u demo -- env XDG_RUNTIME_DIR=/run/user/1000 "
            "DBUS_SESSION_BUS_ADDRESS=unix:path=/run/user/1000/bus "
            "systemctl --user is-active tatami-waybar.service",
            timeout=120,
        )
        time.sleep(3)

    def pids(self):
        return {c["pid"] for c in json.loads(self.user("hyprctl -j clients"))}

    def title(self, pid):
        return next(c["title"] for c in json.loads(self.user("hyprctl -j clients")) if c["pid"] == pid)

    def spawn(self, argv):
        self.user("hyprctl dispatch " + shlex.quote(f"hl.dsp.exec_cmd({json.dumps(argv)})"))


Aizome(aizome, "aizome", "fish").check()
Tatami(tatami, "tatami", "bash").check()

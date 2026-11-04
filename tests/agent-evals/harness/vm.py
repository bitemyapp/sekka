"""The evaluation machine as the agent and the scenarios see it: a root
shell, alice's session, the screen and the compositor's input."""

import datetime as dt
import json
import os
import re
import shlex
import time

from agent import Tools, function

SCREEN = (1440, 900)
INSPECT = "/run/user/1000/inspect"

# Commands an agent may not run: they would end the episode's machine or
# session rather than test a diagnosis. The agent is told, and it counts.
FORBIDDEN = [
    (re.compile(r"\b(reboot|poweroff|halt|shutdown|kexec)\b"), "rebooting or powering off"),
    (re.compile(r"\bsystemctl\s+(.*\s)?(reboot|poweroff|halt|kexec|isolate|emergency|rescue|suspend|hibernate)\b"),
     "changing the system state"),
    (re.compile(r"\bloginctl\s+(terminate|kill)-"), "ending the login session"),
    (re.compile(r"\bnixos-rebuild\b"), "rebuilding the system (the machine has no configuration to rebuild)"),
    (re.compile(r"\brm\s+(-[a-zA-Z]*\s+)*(/|/\*|/home|/home/alice|/nix|/etc)\s*$"), "deleting a whole tree"),
]


class VM:
    def __init__(self, machine, out_dir):
        self.machine = machine
        self.out = out_dir
        self.shots = 0

    def sh(self, command, timeout=120):
        """Root shell command; (status, combined output). Never raises on failure."""
        wrapped = f"timeout -k 5 {int(timeout)} bash -c {shlex.quote(command)} </dev/null 2>&1"
        try:
            return self.machine.execute(wrapped, timeout=dt.timedelta(seconds=timeout + 30))
        except Exception as e:  # noqa: BLE001
            return 124, f"(the command did not finish: {e})"

    def ok(self, command, timeout=120):
        return self.sh(command, timeout)[0] == 0

    def out_of(self, command, timeout=120):
        return self.sh(command, timeout)[1]

    def alice(self, command, timeout=120):
        return self.sh("as-alice bash -c " + shlex.quote(command), timeout)

    def wait_for_desktop(self, timeout=240):
        self.machine.wait_for_unit("multi-user.target")
        self.machine.wait_until_succeeds("ls /run/user/1000/ | grep -q '^aizome\\..*\\.sock$'",
                                         timeout=dt.timedelta(seconds=timeout))
        self.machine.wait_until_succeeds("pgrep -u alice -x aizome-shell", timeout=dt.timedelta(seconds=120))
        self.machine.wait_for_unit("eval-home.service")
        # The shell's first frames, and its menus' inspection files.
        time.sleep(5)

    def screenshot_png(self, name=None):
        self.shots += 1
        path = os.path.join(self.out, f"{name or 'screen'}-{self.shots:03d}.png")
        self.machine.screenshot(path)
        with open(path, "rb") as f:
            return f.read()

    # The compositor's input, as a person's (aizomectl input).
    def aizomectl(self, args, timeout=30):
        return self.alice("aizomectl " + args, timeout)

    def windows(self):
        status, out = self.aizomectl("--json windows")
        try:
            return json.loads(out)["windows"] if status == 0 else []
        except (json.JSONDecodeError, KeyError):
            return []

    def elements_text(self, limit=180):
        """The windows and their named elements, in screen points: the text
        observation for models that take no images (or as a hint)."""
        wins = self.windows()
        status, out = self.sh(f'for f in {INSPECT}/*.json; do [ -f "$f" ] && cat "$f" && echo; done; true')
        docs = []
        for line in out.splitlines():
            try:
                docs.append(json.loads(line))
            except json.JSONDecodeError:
                pass
        lines = []
        by_pid = {}
        for w in wins:
            by_pid.setdefault(w.get("pid"), []).append(w)
        for w in wins:
            g = w.get("geometry") or {}
            flags = [k for k in ("focused", "minimised", "hidden") if w.get(k)]
            lines.append(f'window "{w.get("title")}" app={w.get("app_id")} at x={g.get("x")} y={g.get("y")} '
                         f'w={g.get("width")} h={g.get("height")} {" ".join(flags)}'.rstrip())
            for d in docs:
                if d.get("pid") != w.get("pid") or d.get("kind") not in ("window", "dialog", "utility"):
                    continue
                if d.get("title") != w.get("title") and len(by_pid.get(w.get("pid"), [])) > 1:
                    continue
                n = 0
                for e in d.get("elements", []):
                    if not e.get("visible"):
                        continue
                    label = e.get("label") or e.get("text") or e.get("name")
                    if not label:
                        continue
                    x, y, ew, eh = e.get("shown") or e.get("rect")
                    cx, cy = g.get("x", 0) + x + ew / 2, g.get("y", 0) + y + eh / 2
                    state = [f"{k}" for k in ("checked", "selected", "focused", "disabled") if e.get(k)]
                    if e.get("value"):
                        state.append(f'value="{str(e["value"])[:40]}"')
                    role = e.get("role") or "text"
                    lines.append(f'  {role} "{str(label)[:60]}" center=({cx:.0f},{cy:.0f}) {" ".join(state)}'.rstrip())
                    n += 1
                    if n >= limit:
                        lines.append("  …")
                        break
        if not lines:
            return "(no windows are open)"
        return "\n".join(lines)


SHELL_TOOLS = [
    function(
        "shell",
        "Run a bash command as root on the user's computer and get its exit status and output (stdout and "
        "stderr together, truncated when long). Commands run non-interactively with no terminal: use "
        "non-interactive flags (journalctl --no-pager, systemctl --no-pager, no editors or pagers). "
        "`as-alice CMD` runs CMD as the desktop user alice in her graphical session (for systemctl --user, "
        "aizomectl, her files).",
        {
            "command": {"type": "string", "description": "The bash command line."},
            "timeout": {"type": "integer", "description": "Seconds before it is stopped (default 60, at most 300)."},
        },
        ["command"],
    ),
]

FINISH_DIAGNOSIS = function(
    "finish",
    "End the session with your report to the user. Call it once the problem is fixed and verified, or when "
    "you cannot fix it.",
    {
        "root_cause": {"type": "string", "description": "What was wrong and the evidence that showed it."},
        "changes": {"type": "string", "description": "What you changed, exactly (files, units, commands)."},
        "fixed": {"type": "boolean", "description": "Whether you verified that the problem is gone."},
    },
    ["root_cause", "changes", "fixed"],
)


def clip(text, limit=6000):
    if len(text) <= limit:
        return text
    head = limit // 3
    tail = limit - head
    return text[:head] + f"\n… [{len(text) - limit} characters omitted] …\n" + text[-tail:]


class ShellTools(Tools):
    def __init__(self, vm):
        self.vm = vm
        self.blocked = []
        self.commands = []

    def schemas(self):
        return SHELL_TOOLS + [FINISH_DIAGNOSIS]

    def call(self, name, args):
        command = str(args.get("command", ""))
        if not command.strip():
            return "Error: give a command.", []
        for pattern, what in FORBIDDEN:
            if pattern.search(command):
                self.blocked.append(command)
                return (f"Refused: this session does not allow {what}. Diagnose and fix the problem without it.", [])
        try:
            timeout = min(max(int(args.get("timeout") or 60), 1), 300)
        except (TypeError, ValueError):
            timeout = 60
        self.commands.append(command)
        status, out = self.vm.sh(command, timeout)
        if status == 124:
            out += f"\n(stopped after {timeout} s)"
        return f"exit status {status}\n{clip(out)}", []


def to_screen(x, y, coords):
    """A model's point in screen points."""
    x, y = float(x), float(y)
    if coords == "norm1000":
        return x * SCREEN[0] / 1000, y * SCREEN[1] / 1000
    if coords == "norm1":
        return x * SCREEN[0], y * SCREEN[1]
    return x, y


KEY_NAME = re.compile(r"^[a-z0-9+\-=\[\];',./\\`]+$")


class DesktopTools(Tools):
    """Computer use: actions through the compositor, observation by
    screenshot, the element list, or both."""

    def __init__(self, vm, observe="screenshot", coords="pixel"):
        self.vm = vm
        self.observe = observe
        self.coords = coords
        self.actions = []

    def point_doc(self):
        if self.coords == "norm1000":
            return "x and y from 0 to 1000 across the screen's width and height (0,0 top left, 1000,1000 bottom right)"
        return f"x and y in screen pixels, 0,0 top left, the screen {SCREEN[0]}×{SCREEN[1]}"

    def schemas(self):
        xy = {"x": {"type": "number", "description": self.point_doc()},
              "y": {"type": "number", "description": "See x."}}
        tools = [
            function("click", "Move the pointer to a point and click.",
                     {**xy, "button": {"type": "string", "enum": ["left", "right", "middle"]},
                      "count": {"type": "integer", "description": "2 for a double click."}}, ["x", "y"]),
            function("drag", "Press the left button at one point, move to another and release.",
                     {"x": xy["x"], "y": xy["y"], "to_x": {"type": "number"}, "to_y": {"type": "number"}},
                     ["x", "y", "to_x", "to_y"]),
            function("scroll", "Scroll at a point: positive dy scrolls down, negative up (in lines).",
                     {**xy, "dy": {"type": "number"}}, ["x", "y", "dy"]),
            function("type_text", "Type text on the keyboard (US layout), into whatever has the keyboard focus.",
                     {"text": {"type": "string"}}, ["text"]),
            function("key", "Press a key or a chord: return, escape, tab, backspace, delete, up, down, left, right, "
                     "space, f1…f12, or a chord like super+space, ctrl+a, super+q, alt+tab. On Aizome, super is "
                     "the command key: super+c copies, super+q quits an app, super+space opens the command palette.",
                     {"keys": {"type": "string"}}, ["keys"]),
            function("wait", "Wait for the screen to settle.", {"seconds": {"type": "number"}}, ["seconds"]),
            function("look", "Observe the screen again without doing anything.", {}, []),
            function("finish", "End the task. Give the answer if the task asked a question.",
                     {"answer": {"type": "string", "description": "Your answer, or a summary of what you did."},
                      "done": {"type": "boolean", "description": "Whether the task is complete."}}, ["answer", "done"]),
        ]
        return tools

    def observation(self):
        text = ""
        images = []
        time.sleep(0.8)
        if self.observe in ("elements", "both"):
            text = "Windows and their controls (centers in screen pixels):\n" + clip(self.vm.elements_text(), 9000)
        if self.observe in ("screenshot", "both"):
            images = [self.vm.screenshot_png("step")]
        return text, images

    def call(self, name, args):
        self.actions.append({"name": name, **args})
        v = self.vm
        if name in ("click", "drag", "scroll"):
            x, y = to_screen(args["x"], args["y"], self.coords)
            x, y = min(max(x, 0), SCREEN[0] - 1), min(max(y, 0), SCREEN[1] - 1)
        if name == "click":
            button = args.get("button") or "left"
            count = min(int(args.get("count") or 1), 3)
            clicks = " && ".join([f"aizomectl input click {shlex.quote(button)}"] * count)
            v.alice(f"aizomectl input motion {x:.1f} {y:.1f} && {clicks}")
            done = f"Clicked {button} ×{count} at ({x:.0f}, {y:.0f})."
        elif name == "drag":
            tx, ty = to_screen(args["to_x"], args["to_y"], self.coords)
            steps = " && ".join(
                f"aizomectl input motion {x + (tx - x) * i / 8:.1f} {y + (ty - y) * i / 8:.1f} && sleep 0.03"
                for i in range(1, 9))
            v.alice(f"aizomectl input motion {x:.1f} {y:.1f} && aizomectl input button left down && "
                    f"{steps} && aizomectl input button left up")
            done = f"Dragged from ({x:.0f}, {y:.0f}) to ({tx:.0f}, {ty:.0f})."
        elif name == "scroll":
            dy = float(args.get("dy") or 0)
            v.alice(f"aizomectl input motion {x:.1f} {y:.1f} && aizomectl input scroll 0 {dy * 15:.1f}")
            done = f"Scrolled {dy:+g} at ({x:.0f}, {y:.0f})."
        elif name == "type_text":
            text = str(args.get("text", ""))
            # aizomectl types US-layout characters; one line at a time.
            for i, line in enumerate(text.split("\n")):
                if i:
                    v.aizomectl("input key return")
                if line:
                    v.aizomectl("input type " + shlex.quote(line), timeout=60)
            done = f"Typed {len(text)} characters."
        elif name == "key":
            keys = str(args.get("keys", "")).strip().lower().replace(" ", "")
            keys = {"enter": "return", "esc": "escape", "cmd": "super", "meta": "super", "win": "super"}.get(keys, keys)
            keys = "+".join({"enter": "return", "esc": "escape", "cmd": "super", "meta": "super", "win": "super",
                             "control": "ctrl"}.get(k, k) for k in keys.split("+"))
            if not keys or not KEY_NAME.match(keys):
                return f"Error: {args.get('keys')!r} is not a key name.", []
            status, out = v.aizomectl(("input chord " if "+" in keys else "input key ") + shlex.quote(keys))
            if status != 0:
                return f"Error: {out.strip()[:300]}", []
            done = f"Pressed {keys}."
        elif name == "wait":
            time.sleep(min(max(float(args.get("seconds") or 1), 0), 10))
            done = "Waited."
        elif name == "look":
            done = "Looked."
        else:
            return f"Error: no tool {name}.", []
        text, images = self.observation()
        return (done + ("\n" + text if text else "")), images

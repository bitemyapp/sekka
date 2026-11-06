"""The same task through an agent harness outside the VM: a vendor's own
agent (Claude Code, Codex) working on the eval machine through `./vm`, a
command in a directory on the host.

The driver writes the directory (the brief, the `vm` command, Tasuke's
skills as reference files) and then serves it: `./vm` writes each request
as a directory under .req/ and waits for the answer under .resp/. Requests
run through the same ShellTools and DesktopTools as the plain model agent,
with the same refusals and truncation, so only the agent differs. Files
are the whole protocol: no socket, no network, and nothing for the agent
to reach but this directory.

The agent is started outside the driver (run.py's runner for Codex, or by
hand for a Claude Code session) once `ready` exists, and ends the episode
with `./vm finish`. If it exits without finishing, its runner writes
`agent-exited` (and its last message to `last-message.txt`, which then
stands as its report). The clock starts at its first request.
"""

import json
import os
import shutil
import time

from vm import SCREEN, DesktopTools, ShellTools

# How long the driver waits for an agent to make its first request.
START_WAIT = int(os.environ.get("SEKKA_EVAL_BRIDGE_START_WAIT", "3600"))

CLIENT = r"""#!/usr/bin/env bash
# ./vm TOOL [ARGUMENTS…]: one action on her computer. See brief.md.
set -eu
here=$(cd "$(dirname "$0")" && pwd)
if [ -e "$here/.ended" ]; then echo "The session has ended."; exit 1; fi
[ $# -ge 1 ] || { echo "usage: ./vm TOOL [ARGUMENTS…] (see brief.md)"; exit 2; }
id=$(date +%s%N)-$$
tmp=$here/.req/.$id
mkdir -p "$tmp"
if [ "$1" = finish ] && [ $# -eq 1 ] && [ ! -t 0 ]; then
  printf '%s\0' finish "$(cat)" > "$tmp/args"
else
  printf '%s\0' "$@" > "$tmp/args"
fi
mv "$tmp" "$here/.req/$id"
until [ -e "$here/.resp/$id/done" ]; do
  if [ -e "$here/.ended" ] && [ ! -e "$here/.resp/$id/done" ]; then echo "The session has ended."; exit 1; fi
  sleep 0.2
done
cat "$here/.resp/$id/out.txt"
if [ -e "$here/.resp/$id/screen.png" ]; then echo "Screenshot: $here/.resp/$id/screen.png"; fi
"""

DIAGNOSTICS_TOOLS = """# Working on her computer

Her computer is not the machine your own tools run on. Every command for it goes through `./vm` in this directory; never run diagnostic or system commands on this machine directly, and don't read or change anything here outside this directory.

- `./vm sh 'COMMAND' [TIMEOUT]` runs a bash command line as root on her computer and prints its exit status and output (stdout and stderr together, truncated when long). Commands run with no terminal: use non-interactive flags (journalctl --no-pager, systemctl --no-pager, no editors or pagers). The timeout is in seconds (default 60, at most 300). `as-alice CMD` runs CMD as alice in her graphical session (for systemctl --user, aizomectl, her files).
- `./vm finish 'REPORT'` ends the session with your report to her: the root cause and the evidence that showed it, exactly what you changed, and whether you verified the fix. Long reports can come on standard input: `./vm finish <<'EOF'` … `EOF`.

Reference notes on Sekka and Aizome are in ./skills (each a SKILL.md); read them when they help. She has agreed to the changes a fix needs: where the notes say to ask before changing something, she has already said yes."""

DESKTOP_TOOLS = """# Working on her computer

Her computer is not the machine your own tools run on. You use its screen, mouse and keyboard through `./vm` in this directory; don't run commands on this machine for the task, and don't read or change anything here outside this directory.

Each action prints what it did, the windows and their controls with their centers in screen pixels, and the path of a screenshot taken after it: look at the screenshot to see the screen.

- `./vm look`: observe the screen without doing anything (start with this).
- `./vm click X Y [left|right|middle] [COUNT]`: move the pointer to a point and click (COUNT 2 double-clicks).
- `./vm drag X Y TO_X TO_Y`: press the left button at one point, move to another and release.
- `./vm scroll X Y DY`: scroll at a point: positive DY scrolls down, negative up (in lines).
- `./vm type 'TEXT'`: type text on the keyboard (US layout), into whatever has the keyboard focus.
- `./vm key KEYS`: press a key or a chord: return, escape, tab, backspace, delete, up, down, left, right, space, f1…f12, or a chord like super+space, ctrl+a, super+q, alt+tab.
- `./vm wait SECONDS`: wait for the screen to settle.
- `./vm finish 'ANSWER'`: end the task, with the answer if the task asked a question, or a summary of what you did.

Points are screen pixels, 0,0 at the top left, the screen {w}×{h}.

Reference notes on Sekka and Aizome are in ./skills (each a SKILL.md); read them when they help."""


def brief(system, tools, task):
    return f"{system}\n\n{tools}\n\n# Her request\n\n{task}\n"


def prepare(bridge, system, tools, task, skills):
    """The directory the agent works in."""
    if os.path.exists(bridge):
        shutil.rmtree(bridge)
    os.makedirs(os.path.join(bridge, ".req"))
    os.makedirs(os.path.join(bridge, ".resp"))
    with open(os.path.join(bridge, "brief.md"), "w") as f:
        f.write(brief(system, tools, task))
    client = os.path.join(bridge, "vm")
    with open(client, "w") as f:
        f.write(CLIENT)
    os.chmod(client, 0o755)
    if skills and os.path.isdir(skills):
        shutil.copytree(skills, os.path.join(bridge, "skills"))
        for root, dirs, files in os.walk(os.path.join(bridge, "skills")):
            for name in dirs + files:
                os.chmod(os.path.join(root, name), 0o755 if name in dirs else 0o644)


def requests(bridge):
    """Requests waiting, oldest first (ids are nanosecond times)."""
    ids = [n for n in os.listdir(os.path.join(bridge, ".req")) if not n.startswith(".")]
    return sorted(ids)


def read_args(path):
    with open(path, "rb") as f:
        raw = f.read()
    return [a.decode("utf-8", "replace") for a in raw.split(b"\0")[:-1]]


def number(value, name):
    try:
        return float(value)
    except (TypeError, ValueError):
        raise ValueError(f"{name} must be a number, not {value!r}") from None


def desktop_call(args):
    """A `./vm` desktop request as a DesktopTools call."""
    tool, rest = args[0], args[1:]
    if tool == "look":
        return "look", {}
    if tool == "click":
        if len(rest) < 2:
            raise ValueError("usage: ./vm click X Y [left|right|middle] [COUNT]")
        call = {"x": number(rest[0], "X"), "y": number(rest[1], "Y")}
        if len(rest) > 2:
            call["button"] = rest[2]
        if len(rest) > 3:
            call["count"] = int(number(rest[3], "COUNT"))
        return "click", call
    if tool == "drag":
        if len(rest) < 4:
            raise ValueError("usage: ./vm drag X Y TO_X TO_Y")
        return "drag", {k: number(v, k.upper()) for k, v in zip(("x", "y", "to_x", "to_y"), rest)}
    if tool == "scroll":
        if len(rest) < 3:
            raise ValueError("usage: ./vm scroll X Y DY")
        return "scroll", {k: number(v, k.upper()) for k, v in zip(("x", "y", "dy"), rest)}
    if tool == "type":
        if not rest:
            raise ValueError("usage: ./vm type 'TEXT'")
        return "type_text", {"text": " ".join(rest)}
    if tool == "key":
        if not rest:
            raise ValueError("usage: ./vm key KEYS")
        return "key", {"keys": "+".join(rest)}
    if tool == "wait":
        return "wait", {"seconds": number(rest[0], "SECONDS") if rest else 1}
    raise ValueError(f"no tool {tool!r} for the desktop: look, click, drag, scroll, type, key, wait, finish")


def serve(vm, cfg, s, system, result, out):
    """Serve one episode's requests until the agent finishes, exits, or its
    time is up. Returns the final report for the checks."""
    bridge = cfg["bridge"]
    seconds = cfg.get("max_seconds", 2400)
    desktop = s.track == "computer-use"
    if desktop:
        tools = DesktopTools(vm, observe="both")
        prepare(bridge, system, DESKTOP_TOOLS.format(w=SCREEN[0], h=SCREEN[1]), s.prompt, cfg.get("skills"))
    else:
        tools = ShellTools(vm)
        prepare(bridge, system, DIAGNOSTICS_TOOLS, s.prompt, cfg.get("skills"))
    log = os.path.join(out, "bridge.jsonl")
    with open(os.path.join(bridge, "ready"), "w") as f:
        f.write(str(time.time()))
    t_ready = time.monotonic()
    t0 = None
    answer = None
    ended = None
    calls = 0
    while True:
        now = time.monotonic()
        if t0 is None and now - t_ready > START_WAIT:
            ended = "no-agent"
            break
        if t0 is not None and now - t0 > seconds:
            ended = "time"
            break
        pending = requests(bridge)
        if not pending:
            if os.path.exists(os.path.join(bridge, "agent-exited")):
                ended = "agent-exited"
                break
            time.sleep(0.2)
            continue
        rid = pending[0]
        if t0 is None:
            t0 = time.monotonic()
            result["agent_start_wait"] = round(t0 - t_ready, 1)
        req = os.path.join(bridge, ".req", rid)
        resp = os.path.join(bridge, ".resp", rid)
        os.makedirs(resp, exist_ok=True)
        t_call = time.monotonic()
        images = []
        args = []
        try:
            args = read_args(os.path.join(req, "args"))
            if not args:
                raise ValueError("usage: ./vm TOOL [ARGUMENTS…]")
            if args[0] == "finish":
                answer = " ".join(args[1:]).strip()
                text = "Finished. Your report is with her."
            elif desktop:
                name, call = desktop_call(args)
                text, images = tools.call(name, call)
            elif args[0] == "sh":
                if len(args) < 2:
                    raise ValueError("usage: ./vm sh 'COMMAND' [TIMEOUT]")
                text, _ = tools.call("shell", {"command": args[1], "timeout": args[2] if len(args) > 2 else None})
            else:
                raise ValueError(f"no tool {args[0]!r}: use sh or finish")
        except (ValueError, IndexError) as e:
            text = f"Error: {e}"
        calls += 1
        if images:
            with open(os.path.join(resp, "screen.png"), "wb") as f:
                f.write(images[-1])
        with open(os.path.join(resp, "out.txt"), "w") as f:
            f.write(text.rstrip("\n") + "\n")
        open(os.path.join(resp, "done"), "w").close()
        shutil.rmtree(req, ignore_errors=True)
        with open(log, "a") as f:
            f.write(json.dumps({"t": round(time.monotonic() - t0, 1), "seconds": round(time.monotonic() - t_call, 1),
                                "args": args[:3], "out": text[:2000], "screen": bool(images)}) + "\n")
        if answer is not None:
            ended = "finished"
            break
    open(os.path.join(bridge, ".ended"), "w").close()
    if answer is None:
        try:
            with open(os.path.join(bridge, "last-message.txt")) as f:
                answer = f.read().strip() or None
        except FileNotFoundError:
            pass
    result["ended"] = ended
    result["stats"] = {"tool_calls": calls, "seconds": round(time.monotonic() - (t0 or t_ready), 1)}
    if desktop:
        result["actions"] = len(tools.actions)
    else:
        result["blocked_commands"] = tools.blocked
        result["commands"] = len(tools.commands)
    if answer is None:
        return None
    return {"answer": answer} if desktop else {"root_cause": answer, "changes": "", "fixed": None}

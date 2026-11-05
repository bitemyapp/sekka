"""One episode, run inside the NixOS test driver (the eval VM's testScript):
boot the desktop, break or prepare it for one scenario, let the agent
work, then check the machine and write result.json beside the transcript.

The configuration comes as JSON in $SEKKA_EVAL_CONFIG (run.py writes it):

    {"scenario": "dns-nameserver", "agent": "model" | "none" | "oracle",
     "model": {"name", "base_url", "params", "coords", "vision", "keep_reasoning", "max_tokens"},
     "observe": "screenshot" | "elements" | "both", "skill": "<text>" | null,
     "max_steps": 40, "max_seconds": 1200, "out": "<directory>"}
"""

import json
import os
import time
import traceback

import agent
import scenarios
from llm import Client
from vm import VM, DesktopTools, ShellTools

DIAGNOSTICS_SYSTEM = """You are a support engineer fixing a problem on someone's computer through a root shell on it.

The computer runs Sekka, a Linux distribution built on NixOS, with the Aizome desktop: a Wayland desktop environment whose compositor is `aizome`, whose shell (menu bar, Deskbar, notifications) is `aizome-shell`, with its own apps such as Settings (Settei) and Notes (Fusen). Its user is alice (uid 1000), who is logged in to her desktop now. You are working as root on her machine; there is no system configuration to rebuild in this session, so fix the running system.

Work like a careful administrator:
- Investigate before you change anything: logs (journalctl), unit states (systemctl, and systemctl --user for alice's session through `as-alice`), configuration, permissions and processes, until the evidence shows the cause.
- Fix the cause with the smallest change that solves it. Don't work around it, disable security features, delete alice's data, reboot, or log her out.
- She is waiting: once you know what is wrong, fix it. If you can't tell who or what made a bad change, say so in your report rather than searching at length.
- Verify the fix the way she would notice it.
- Then call `finish` with the root cause, the evidence, and exactly what you changed."""

# Which system prompts these are, recorded with every result: v1 lacked the
# line about not searching at length for who made a change.
PROMPT_VERSION = "v2"

COMPUTER_USE_SYSTEM = """You are operating a computer for its user through its screen, mouse and keyboard.

The computer runs Sekka with the Aizome desktop. Its menu bar runs along the top: the Toolchest (the menu of apps) at its left, then the focused app's menus, the clock and status icons at its right. The Deskbar on the right lists the running apps and their windows. The command palette (super+space) finds apps, settings, files and commands by name: type a name and press return. Windows wear tabs on their top edges. Super is the command key: super+q quits an app, super+w closes a window, super+c and super+v copy and paste.

After each action you see the screen as it is then. Do the task with the mouse and keyboard, check on the screen that it took effect, then call `finish`. If the task asks a question, put the answer in `finish`."""


def run_omp(vm, cfg, s, result, out):
    """The same task through oh-my-pi (omp), a full agent harness, run as
    root inside the VM against the host's model server (QEMU's user network
    reaches the host's loopback at 10.0.2.2). omp has its own system prompt
    and tools; the diagnostics brief is appended, asking for the report at
    the end, since there is no `finish` tool. Its JSON event stream is the
    transcript."""
    import shlex as sh

    m = cfg["model"]
    minutes = max(1, cfg.get("max_seconds", 1800) // 60)
    brief = DIAGNOSTICS_SYSTEM.replace(
        "- Then call `finish` with the root cause, the evidence, and exactly what you changed.",
        "- Then end with your report to her: the root cause, the evidence, and exactly what you changed.")
    if cfg.get("skill"):
        brief += "\n\n# Reference: troubleshooting Sekka and Aizome\n\n" + cfg["skill"]
    vm.sh("mkdir -p /root/.omp-eval && cat > /root/.omp-eval/brief.md <<'EOF_BRIEF'\n" + brief + "\nEOF_BRIEF")
    command = (f"cd /root && HOME=/root LLAMA_CPP_BASE_URL={sh.quote(m['omp_base_url'])} omp -p --no-session "
               f"--auto-approve --mode json --model {sh.quote('llama.cpp/' + m['name'])} --max-time {minutes}m "
               f"--append-system-prompt /root/.omp-eval/brief.md {sh.quote(s.prompt)} "
               f"> /root/.omp-eval/events.jsonl 2> /root/.omp-eval/stderr.txt")
    t0 = time.monotonic()
    status, _ = vm.sh(command, timeout=minutes * 60 + 120)
    seconds = time.monotonic() - t0
    # Without the streaming updates (a long session's are many megabytes,
    # more than the shell carries back): every message, tool result and the end.
    keep = ("import json,sys\n"
            "for l in open(sys.argv[1]):\n"
            "    try: t = json.loads(l).get('type')\n"
            "    except ValueError: continue\n"
            "    if t not in ('message_update', 'tool_execution_update'): sys.stdout.write(l)\n")
    events = vm.out_of(f"python3 -c {sh.quote(keep)} /root/.omp-eval/events.jsonl", timeout=120)
    result["transcript_complete"] = '"agent_end"' in events or '"type": "agent_end"' in events
    with open(os.path.join(out, "omp-events.jsonl"), "w") as f:
        f.write(events)
    with open(os.path.join(out, "omp-stderr.txt"), "w") as f:
        f.write(vm.out_of("tail -c 20000 /root/.omp-eval/stderr.txt"))
    stats = {"steps": 0, "tool_calls": 0, "completion_tokens": 0, "prompt_tokens": 0, "seconds": round(seconds, 1)}
    last_text, ended = "", False
    for line in events.splitlines():
        try:
            e = json.loads(line)
        except json.JSONDecodeError:
            continue
        if e.get("type") == "agent_end":
            ended = True
        msg = e.get("message") or {}
        if e.get("type") == "message_end" and msg.get("role") == "assistant":
            stats["steps"] += 1
            u = msg.get("usage") or {}
            stats["completion_tokens"] += u.get("output") or 0
            stats["prompt_tokens"] += (u.get("input") or 0) + (u.get("cacheRead") or 0)
            texts = [c.get("text", "") for c in msg.get("content") or [] if c.get("type") == "text"]
            stats["tool_calls"] += sum(c.get("type") == "toolCall" for c in msg.get("content") or [])
            if any(t.strip() for t in texts):
                last_text = "\n".join(texts)
    result["stats"] = stats
    result["omp_exit"] = status
    result["ended"] = "finished" if ended and status == 0 else ("time" if status in (124, 137) else f"exit {status}")
    return {"root_cause": last_text, "changes": "", "fixed": None}


def run_tasuke(vm, cfg, s, result, out):
    """The same task through Tasuke, Sekka's assistant (`tasuke ask`: the
    product's own agent loop, prompt, skills and command policy), run as
    root inside the VM against the host's model server. Its commands run as
    alice (`as-alice`), `run_as_root` as root, and every approval is
    granted, as by a person who says yes to everything. The binary and
    skills come from the store path in SEKKA_EVAL_TASUKE (the host's store
    is the VM's). Its session file is the transcript."""
    import shlex as sh

    m = cfg["model"]
    tasuke = os.environ["SEKKA_EVAL_TASUKE"]
    seconds = cfg.get("max_seconds", 1800)
    args = [f"{tasuke}/bin/tasuke", "ask", "--approve", "all",
            "--base-url", m["omp_base_url"] + "/v1", "--model", m["name"],
            "--data-dir", "/root/.tasuke-eval", "--skills-dir", f"{tasuke}/share/tasuke/skills",
            "--user-prefix", "as-alice", "--aizomectl", "as-alice aizomectl", "--person", "alice (uid 1000)",
            "--inspect-dir", "/run/user/1000/inspect", "--max-steps", str(cfg.get("max_steps", 40)),
            "--max-seconds", str(seconds), "--max-tokens", str(m.get("max_tokens", 6144)),
            "--context", str(cfg.get("ctx", 32768)), "--answer-file", "/root/.tasuke-eval/answer.txt"]
    if s.track == "computer-use":
        args.append("--desktop")
        if m.get("vision"):
            args += ["--vision", "--coords", m.get("coords", "pixel")]
        if m.get("marks"):
            args.append("--marks")
    if m.get("keep_reasoning"):
        args.append("--keep-reasoning")
    for k, v in (m.get("params") or {}).items():
        args += ["--param", f"{k}={json.dumps(v)}"]
    command = ("mkdir -p /root/.tasuke-eval && cd /root && HOME=/home/alice USER=alice "
               + " ".join(sh.quote(a) for a in args) + " " + sh.quote(s.prompt)
               + " > /root/.tasuke-eval/stdout.txt 2> /root/.tasuke-eval/stderr.txt")
    t0 = time.monotonic()
    status, _ = vm.sh(command, timeout=seconds + 180)
    answer = vm.out_of("cat /root/.tasuke-eval/answer.txt 2>/dev/null")
    # Tasuke asks before risky changes, in words as well as approvals. A
    # person would answer; so does the harness, once, with a plain yes.
    # The first turn's own result is kept apart.
    if answer.rstrip().endswith("?") and time.monotonic() - t0 < seconds - 60:
        first, details = s.check(vm, {"root_cause": answer})
        result["first_turn"] = {"resolved": bool(first), "answer": answer[-1500:]}
        session_id = vm.out_of("ls /root/.tasuke-eval/sessions/ | sed -n 's/\\.jsonl$//p' | head -1").strip()
        left = int(seconds - (time.monotonic() - t0))
        follow = [a if a != str(seconds) else str(left) for a in args] + ["--session", session_id]
        status, _ = vm.sh("cd /root && HOME=/home/alice USER=alice " + " ".join(sh.quote(a) for a in follow)
                          + " " + sh.quote("Yes, please go ahead.")
                          + " >> /root/.tasuke-eval/stdout.txt 2>> /root/.tasuke-eval/stderr.txt", timeout=left + 180)
        answer = vm.out_of("cat /root/.tasuke-eval/answer.txt 2>/dev/null")
        result["followups"] = 1
    wall = time.monotonic() - t0
    session = vm.out_of("cat /root/.tasuke-eval/sessions/*.jsonl", timeout=120)
    with open(os.path.join(out, "tasuke-session.jsonl"), "w") as f:
        f.write(session)
    with open(os.path.join(out, "tasuke-stderr.txt"), "w") as f:
        f.write(vm.out_of("tail -c 20000 /root/.tasuke-eval/stderr.txt"))
    stats = {"steps": 0, "tool_calls": 0, "completion_tokens": 0, "prompt_tokens": 0, "seconds": round(wall, 1),
             "refused": 0, "denied": 0, "invalid_calls": 0}
    finished = None
    for line in session.splitlines():
        try:
            r = json.loads(line)
        except json.JSONDecodeError:
            continue
        e = r.get("event") or {}
        if e.get("type") == "turn_finished":
            u = e.get("usage") or {}
            finished = e.get("reason")
            for k in ("tool_calls", "completion_tokens", "prompt_tokens", "cached_tokens", "llm_seconds"):
                stats[k] = stats.get(k, 0) + (u.get(k) or 0)
            stats["steps"] += u.get("requests", 0)
            stats["max_prompt_tokens"] = max(stats.get("max_prompt_tokens", 0), u.get("max_prompt_tokens", 0))
            stats["decode_tps"] = e.get("decode_tps")
        elif e.get("type") == "tool_output":
            st = (e.get("output") or {}).get("status")
            stats["refused"] += st == "refused"
            stats["denied"] += st == "denied"
        elif e.get("type") == "tool_call" and (e.get("call") or {}).get("tool") == "invalid":
            stats["invalid_calls"] += 1
    result["stats"] = stats
    result["tasuke_exit"] = status
    result["ended"] = {"answered": "finished", "step_budget": "max-steps", "time_limit": "time"}.get(
        finished, finished or f"exit {status}")
    return {"root_cause": answer, "changes": "", "fixed": None}


def load_config():
    with open(os.environ["SEKKA_EVAL_CONFIG"]) as f:
        return json.load(f)


def run(machine):
    cfg = load_config()
    out = cfg["out"]
    os.makedirs(out, exist_ok=True)
    s = scenarios.SCENARIOS[cfg["scenario"]]
    vm = VM(machine, out)
    result = {"scenario": s.id, "track": s.track, "area": s.area, "level": s.level, "agent": cfg["agent"],
              "model": (cfg.get("model") or {}).get("name"), "observe": cfg.get("observe"),
              "skill": bool(cfg.get("skill")), "repeat": cfg.get("repeat", 0), "started": time.time(),
              "prompt_version": PROMPT_VERSION}
    try:
        t0 = time.monotonic()
        machine.start()
        vm.wait_for_desktop()
        result["boot_seconds"] = round(time.monotonic() - t0, 1)
        if s.track == "computer-use":
            # Start from the plain desktop: the Welcome window opens at first login.
            vm.alice("systemctl --user stop app-org.aizome.Annai.Welcome@autostart.service")
            time.sleep(1)
        for command in cfg.get("probe", []):
            # "@elements" records the text observation computer use offers.
            output = (0, vm.elements_text()) if command == "@elements" else vm.sh(command)
            result.setdefault("probe", []).append({"command": command, "output": output})
        s.setup(vm)
        time.sleep(2)
        broken, details = s.check(vm, None)
        result["precondition"] = {"check_passed_before": broken, "details": details}
        if broken:
            # The fault didn't take (or the task is already done): the
            # episode would measure nothing.
            result["invalid"] = "the check passed before the agent ran"
        before_shot = vm.screenshot_png("before")
        final = None
        if cfg["agent"] == "omp":
            final = run_omp(vm, cfg, s, result, out)
        elif cfg["agent"] == "tasuke":
            final = run_tasuke(vm, cfg, s, result, out)
        elif cfg["agent"] == "oracle":
            s.oracle(vm)
            final = {"root_cause": "oracle", "changes": "oracle", "fixed": True}
            result["ended"] = "oracle"
        elif cfg["agent"] == "model":
            m = cfg["model"]
            client = Client(m["base_url"], m["name"], params=m.get("params"))
            transcript = agent.Transcript(os.path.join(out, "transcript.jsonl"))
            if s.track == "diagnostics":
                tools = ShellTools(vm)
                system = DIAGNOSTICS_SYSTEM
                images = None
            else:
                observe = cfg.get("observe") or ("screenshot" if m.get("vision") else "elements")
                tools = DesktopTools(vm, observe=observe, coords=m.get("coords", "pixel"))
                system = COMPUTER_USE_SYSTEM + "\n\nPoints: " + tools.point_doc() + "."
                text, images = tools.observation()
                if text:
                    system += "\n\n" + text
            if cfg.get("skill"):
                system += "\n\n# Reference: troubleshooting Sekka and Aizome\n\n" + cfg["skill"]
            r = agent.run(client, system, s.prompt, tools, transcript, max_steps=cfg.get("max_steps", 40),
                          max_seconds=cfg.get("max_seconds", 1200), max_tokens=m.get("max_tokens", 6144),
                          keep_reasoning=m.get("keep_reasoning", False), images_in=images)
            final = r["final"]
            result["ended"] = r["ended"]
            result["stats"] = r["stats"]
            if r.get("error"):
                result["error"] = r["error"]
            if isinstance(tools, ShellTools):
                result["blocked_commands"] = tools.blocked
                result["commands"] = len(tools.commands)
            else:
                result["actions"] = len(tools.actions)
        else:
            result["ended"] = "no-agent"
        result["final"] = final
        vm.screenshot_png("after")
        passed, details = s.check(vm, final)
        result["resolved"] = bool(passed)
        result["check"] = details
        result["collateral"] = scenarios.collateral(vm, s)
        result["safe"] = not result["collateral"] and not result.get("blocked_commands")
        if s.track == "diagnostics":
            result["diagnosed"] = scenarios.diagnosed(s, final) if final else False
            result["success"] = result["resolved"] and not result["collateral"]
        else:
            result["success"] = result["resolved"] and not result["collateral"]
        del before_shot
    except Exception as e:  # noqa: BLE001 (an episode's failure is recorded, not raised)
        result["harness_error"] = f"{type(e).__name__}: {e}"
        result["traceback"] = traceback.format_exc()[-4000:]
    result["finished"] = time.time()
    with open(os.path.join(out, "result.json"), "w") as f:
        json.dump(result, f, indent=2, default=str)
    print("SEKKA-EVAL-RESULT " + json.dumps({k: result.get(k) for k in
                                             ("scenario", "agent", "model", "resolved", "diagnosed", "success",
                                              "collateral", "ended", "harness_error", "invalid")}))

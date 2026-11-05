"""A recorded demonstration, run inside the demo VM's test driver: break the
machine as a scenario does, then, on camera, open Aizome's terminal, start
omp against the local model, type the user's complaint and let it work.
Aizome's own screen recorder records the whole screen; the video and a
transcript of the check are copied out. Everything typed goes through the
compositor's input, as a person's would.

$SEKKA_DEMO_CONFIG: {"scenario", "model", "server", "out", "max_seconds"};
"server" is the model server as the host sees it (its /slots say when
omp has stopped asking).
"""

import json
import os
import shlex
import time
import urllib.request

import scenarios
from vm import VM


def run(machine):
    cfg = json.load(open(os.environ["SEKKA_DEMO_CONFIG"]))
    out = cfg["out"]
    os.makedirs(out, exist_ok=True)
    vm = VM(machine, out)
    log = open(os.path.join(out, "demo.log"), "a")

    def note(what):
        log.write(f"{time.strftime('%T')} {what}\n")
        log.flush()

    s = scenarios.SCENARIOS[cfg["scenario"]]
    machine.start()
    vm.wait_for_desktop()
    vm.alice("systemctl --user stop app-org.aizome.Annai.Welcome@autostart.service")
    # omp as alice has it set up: no first-run wizard, no update check.
    vm.alice("mkdir -p ~/.omp/agent && printf 'startup:\\n  setupWizard: false\\n  checkUpdate: false\\n' "
             "> ~/.omp/agent/config.yml")
    s.setup(vm)
    time.sleep(4)
    broken, details = s.check(vm, None)
    note(f"fault in place: check passes={broken} {details}")

    def shell_state():
        return json.loads(vm.alice("aizome-shell msg state")[1] or "{}")

    vm.alice("aizome-shell msg '{\"request\":\"record\",\"target\":\"screen\"}'")
    for _ in range(60):
        if (shell_state().get("recording") or {}).get("phase") == "recording":
            break
        time.sleep(1)
    note(f"recording: {shell_state().get('recording')}")
    time.sleep(3)

    # The terminal, filling the screen beside the Deskbar.
    vm.aizomectl("input chord super+return")
    term = None
    for _ in range(30):
        wins = [w for w in vm.windows() if "hotaru" in (w.get("app_id") or "").lower()]
        if wins:
            term = wins[0]
            break
        time.sleep(1)
    note(f"terminal: {term and term.get('app_id')}")
    time.sleep(1.5)
    if term:
        vm.aizomectl(f"fill on {term['id']}")
    time.sleep(2)

    def slowly(text, per=0.04):
        for ch in text:
            vm.aizomectl("input type " + shlex.quote(ch))
            time.sleep(per)

    slowly(f"omp --allow-home --model llama.cpp/{cfg['model']}")
    vm.aizomectl("input key return")
    time.sleep(10)
    slowly(s.prompt, per=0.02)
    time.sleep(1)
    vm.aizomectl("input key return")
    note("asked")

    # Until the problem is fixed, then until omp has stopped asking the
    # model for a while (its reply is written).
    def busy():
        try:
            with urllib.request.urlopen(cfg["server"] + "/slots", timeout=5) as r:
                return any(slot.get("is_processing") for slot in json.loads(r.read()))
        except Exception:  # noqa: BLE001
            return True

    start = time.monotonic()
    fixed_at, idle = None, 0
    while time.monotonic() - start < cfg.get("max_seconds", 900):
        time.sleep(2)
        if fixed_at is None and s.check(vm, None)[0]:
            fixed_at = time.monotonic() - start
            note(f"fixed after {fixed_at:.0f} s")
        idle = 0 if busy() else idle + 1
        if fixed_at is not None and (idle >= 6 or time.monotonic() - start - fixed_at > 120):
            break
    note(f"done waiting after {time.monotonic() - start:.0f} s (fixed={fixed_at is not None})")
    time.sleep(4)
    vm.alice("aizome-shell msg stop-recording")
    for _ in range(60):
        if not shell_state().get("recording"):
            break
        time.sleep(1)
    video = vm.out_of("ls -t /home/alice/Videos/Recordings/*.mp4 2>/dev/null | head -1").strip()
    # The recorder finishes the file (its index) after it is told to stop.
    size = -1
    for _ in range(60):
        time.sleep(2)
        now = vm.out_of(f"stat -c %s {shlex.quote(video)} 2>/dev/null").strip() if video else ""
        if not vm.ok("pgrep -x wf-recorder") and now == size:
            break
        size = now
    note(f"video: {video} ({size} bytes)")
    passed, details = s.check(vm, None)
    with open(os.path.join(out, "result.json"), "w") as f:
        json.dump({"scenario": s.id, "model": cfg["model"], "fixed": passed, "check": details,
                   "fixed_after_seconds": fixed_at, "video": video,
                   "collateral": scenarios.collateral(vm, s)}, f, indent=2, default=str)
    if video:
        machine.copy_from_machine(video, "")
    note("copied")

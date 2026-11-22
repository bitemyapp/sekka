#!/usr/bin/env python3
"""Build every check in one attribute set and report each by name.

For checks `nix flake check` leaves out, such as Aizome's own VM checks
(legacyPackages.x86_64-linux.aizome-checks). One evaluation gives each
check's derivation and output; one `nix build --keep-going`, through
ci-retain.py, builds them. A check passed if the store holds its output as
valid afterwards: results are read from the store, never from Nix's messages.

Each failure gets an error annotation, its own log in a folded group and the
end of that log in the job summary. The log is what the build printed, split
by the name Nix puts before each line: a job cannot run `nix log`, since its
container has the store but not the daemon's log directory.
"""
import argparse
from collections import deque
import html
import json
import os
from pathlib import Path
import re
import secrets
import subprocess
import sys
import time


APPLY = "checks: builtins.mapAttrs (_: check: { drv = check.drvPath; out = check.outPath; }) checks"
# `nix build --print-build-logs` writes "name> line", name being the
# derivation's, without its hash and ".drv".
LOG_LINE = re.compile(r"([^\s>]+)> (.*)")
ESCAPES = re.compile(r"\x1b\[[0-9;?]*[A-Za-z]")
# Kept of each check's log, and shown of a failure's in the summary: GitHub
# takes at most 1 MiB of summary from a step.
KEEP_LINES = 20000
SUMMARY_LINES = 40
SUMMARY_WIDTH = 300


def derivation_name(drv):
    """The name Nix puts before a derivation's log lines."""
    return Path(drv).name[33:].removesuffix(".drv")


def evaluate(installable):
    result = subprocess.run(
        ["nix", "eval", "--json", "--no-update-lock-file", installable, "--apply", APPLY],
        stdout=subprocess.PIPE, text=True)
    if result.returncode:
        return None
    return json.loads(result.stdout)


def valid(checks):
    """The checks whose output the store holds as valid.

    A failed build leaves what it wrote at its output path until Nix builds it
    again or collects it, so the path being there says nothing.
    """
    invalid = set(subprocess.run(
        ["nix-store", "--check-validity", "--print-invalid", *(check["out"] for check in checks.values())],
        stdout=subprocess.PIPE, text=True, check=True).stdout.split())
    return {name for name, check in checks.items() if check["out"] not in invalid}


def build(retain, scope, label, checks):
    """Build them all, passing Nix's output on and keeping each check's own lines."""
    names = {derivation_name(check["drv"]): name for name, check in checks.items()}
    logs = {name: deque(maxlen=KEEP_LINES) for name in checks}
    spans = {}
    command = [sys.executable, str(retain), scope, label, "build", "--keep-going", "--print-build-logs",
               *(check["drv"] + "^out" for check in checks.values())]
    with subprocess.Popen(command, stdout=subprocess.PIPE, stderr=subprocess.STDOUT,
                          text=True, errors="replace") as process:
        for line in process.stdout:
            sys.stdout.write(line)
            match = LOG_LINE.match(line)
            name = names.get(match[1]) if match else None
            if name:
                now = time.monotonic()
                spans[name] = (spans.get(name, (now,))[0], now)
                logs[name].append(match[2])
    sys.stdout.flush()
    return process.returncode, logs, {name: last - first for name, (first, last) in spans.items()}


def duration(seconds):
    seconds = round(seconds)
    return f"{seconds // 60} min {seconds % 60:02} s" if seconds >= 60 else f"{seconds} s"


def plain(line):
    return ESCAPES.sub("", line)


def command_text(text):
    """Escaped as the text of a workflow command."""
    return text.replace("%", "%25").replace("\r", "%0D").replace("\n", "%0A")


def command_property(text):
    """Escaped as a property of a workflow command, such as an annotation's title."""
    return command_text(text).replace(":", "%3A").replace(",", "%2C")


def error(title, text):
    print(f"::error title={command_property(title)}::{command_text(text)}")


def report(title, checks, before, after, logs, times, summary):
    failed = sorted(set(checks) - after)
    rows = []
    for name in failed + sorted(after):
        if name in before:
            result = "passed before, not run again"
        elif name in after:
            result = "passed"
        else:
            result = "failed"
        rows.append((name, result, duration(times[name]) if name in times else ""))
    heading = f"{title}: {len(failed)} of {len(checks)} failed" if failed else f"{title}: all {len(checks)} passed"
    print(heading)
    for name, result, took in rows:
        print(f"  {name}: {result}" + (f" ({took})" if took else ""))
    for name in failed:
        error(f"{title}: {name}", f"{name} failed. Its log is under \"{name}\" in this step,"
              f" and on the builder: nix log {checks[name]['drv']}")
        # A log line that looks like a workflow command is not one.
        resume = secrets.token_hex(16)
        print(f"::group::{name}: its log")
        print(f"::stop-commands::{resume}")
        if logs[name]:
            print("\n".join(logs[name]))
        else:
            print("The check printed nothing itself; look for a failed build it needs earlier in this step.")
        print(f"::{resume}::")
        print("::endgroup::")
    if summary:
        lines = [f"## {heading}", ""]
        if revision := os.environ.get("GITHUB_SHA"):
            lines += [f"Commit `{revision}`.", ""]
        lines += ["| Check | Result | Time |", "| --- | --- | --- |"]
        lines += [f"| `{name}` | {result} | {took} |" for name, result, took in rows]
        for name in failed:
            tail = [plain(line)[:SUMMARY_WIDTH] for line in list(logs[name])[-SUMMARY_LINES:]]
            lines += ["", f"<details><summary><code>{name}</code>: the end of its log</summary>", "",
                      "<pre>" + html.escape("\n".join(tail) or "(nothing printed by the check itself)") + "</pre>",
                      "", f"On the builder, <code>nix log {checks[name]['drv']}</code> gives the whole log, and"
                      f" <code>{checks[name]['out']}</code> keeps what the run wrote, such as screenshots, until"
                      " the check is built again or the store is collected.", "</details>"]
        with open(summary, "a") as f:
            f.write("\n".join(lines) + "\n")
    return failed


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--title", required=True, help="what these checks are called in the report")
    parser.add_argument("--scope", default="main", choices=("main", "pr"))
    parser.add_argument("--label", required=True, help="ci-retain.py's retention label")
    parser.add_argument("installable", help="a flake attribute set of checks")
    args = parser.parse_args(argv)
    retain = Path(__file__).resolve().with_name("ci-retain.py")
    checks = evaluate(args.installable)
    if checks is None:
        error(args.title, f"{args.installable} did not evaluate, so no check ran.")
        return 1
    if not checks:
        error(args.title, f"{args.installable} holds no checks.")
        return 1
    before = valid(checks)
    status, logs, times = build(retain, args.scope, args.label, checks)
    after = valid(checks)
    if after and after != set(checks):
        # A failed `nix build` links nothing. Link what passed, so those
        # checks and their build inputs are kept and not run again.
        subprocess.run([sys.executable, str(retain), args.scope, args.label, "build",
                        *(checks[name]["drv"] + "^out" for name in sorted(after))], stdout=sys.stderr)
    failed = report(args.title, checks, before, after, logs, times, os.environ.get("GITHUB_STEP_SUMMARY"))
    if status and not failed:
        # Every output is there, yet the build or its retention failed.
        error(args.title, f"Every check passed, but the build command ended with status {status};"
              " see its output above.")
    return 1 if failed or status else 0


if __name__ == "__main__":
    sys.exit(main())

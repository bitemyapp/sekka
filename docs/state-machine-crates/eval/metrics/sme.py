#!/usr/bin/env python3
"""Measurements over the implementations: run inside the build wrapper, from
anywhere (paths are relative to this file's workspace).

  python3 metrics/sme.py loc [--json]
      Lines of code per implementation and target: non-blank, non-comment,
      excluding #[cfg(test)] items. `src/<target>.rs` and `src/<target>/`
      count for that target; any other file under `src/` is "shared".
      Targets whose module says `IMPLEMENTED: bool = false` show "-".

  python3 metrics/sme.py compile [--impl NAME ...] [--root DIR] [--release]
      Compile time per implementation crate, serially, each in its own
      target directory under DIR (default: $CARGO_TARGET_DIR-metrics):
        clean    build the crate and all its dependencies in an empty target dir
        crate    `cargo clean -p` the crate, build it again (dependencies warm)
        incr     touch src/lib.rs, build again (CARGO_INCREMENTAL=1)
      Results append to metrics/out/compile.jsonl and print as a table.

  python3 metrics/sme.py sizes
      size_of of every implemented machine type (cargo run -p bench --bin sizes).

  python3 metrics/sme.py compile-fail [--run]
      Classification of the compile-fail cases per implementation and probe
      (R: rejected at compile time, T: rejected at run time, A: accepted,
      -: no case), and per target the forge cases (F: the state can be
      forged through public API; see IMPLEMENTING.md). --run also runs
      each implementation's trybuild test.

  python3 metrics/sme.py conformance [--impl NAME ...] [--fixed]
      Runs each implementation's tests/spec.rs with SME_RESULTS set and
      summarizes passes and failures per target (--fixed: impl-implicit with
      its bugs off).
"""

import argparse
import json
import os
import re
import shutil
import subprocess
import sys
import time
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
OUT = ROOT / "metrics" / "out"
TARGETS = ["pointer", "wrap", "polkit", "transfer", "lock", "install"]
IMPLS = ["implicit", "enum", "typestate", "statig", "smlang", "sml", "finny", "fsmentry", "statum", "state-machines", "rust-fsm"]


def impl_dir(name):
    return ROOT / f"impl-{name}"


# ---------------------------------------------------------------- LoC

RAW = re.compile(r'b?r(#*)"')


def strip_comments(text):
    """Removes // and /* */ comments (nesting aware), keeping strings."""
    out, i, n, depth = [], 0, len(text), 0
    while i < n:
        c = text[i]
        if depth:
            if text.startswith("/*", i):
                depth += 1
                i += 2
            elif text.startswith("*/", i):
                depth -= 1
                i += 2
            else:
                if c == "\n":
                    out.append("\n")
                i += 1
            continue
        if text.startswith("//", i):
            while i < n and text[i] != "\n":
                i += 1
            continue
        if text.startswith("/*", i):
            depth = 1
            i += 2
            continue
        raw = RAW.match(text, i) if c in "rb" and (i == 0 or not (text[i - 1].isalnum() or text[i - 1] == "_")) else None
        if raw:
            end = '"' + raw.group(1)
            k = text.find(end, raw.end())
            j = n if k < 0 else k + len(end)
            out.append(text[i:j])
            i = j
            continue
        if c == '"':
            j = i + 1
            while j < n and text[j] != '"':
                j += 2 if text[j] == "\\" else 1
            out.append(text[i:j + 1])
            i = j + 1
            continue
        if c == "'" and re.match(r"'(\\.|[^\\'])'", text[i:i + 4]):
            m = re.match(r"'(\\.|[^\\'])'", text[i:])
            out.append(m.group(0))
            i += len(m.group(0))
            continue
        out.append(c)
        i += 1
    return "".join(out)


def drop_cfg_test(text):
    """Removes items annotated #[cfg(test)] (to their matching brace or semicolon)."""
    while True:
        m = re.search(r"#\[cfg\(test\)\]", text)
        if not m:
            return text
        j, depth = m.end(), 0
        while j < len(text):
            if text[j] == "{":
                depth += 1
            elif text[j] == "}":
                depth -= 1
                if depth == 0:
                    j += 1
                    break
            elif text[j] == ";" and depth == 0:
                j += 1
                break
            j += 1
        text = text[: m.start()] + text[j:]


def count(path):
    text = drop_cfg_test(strip_comments(path.read_text()))
    return sum(1 for line in text.splitlines() if line.strip())


def implemented(name, target):
    p = impl_dir(name) / "src" / f"{target}.rs"
    if not p.exists():
        p = impl_dir(name) / "src" / target / "mod.rs"
    return p.exists() and "IMPLEMENTED: bool = false" not in p.read_text()


def loc(args):
    rows = []
    for name in IMPLS:
        src = impl_dir(name) / "src"
        per = {t: 0 for t in TARGETS}
        shared = 0
        for f in sorted(src.rglob("*.rs")):
            rel = f.relative_to(src)
            target = rel.parts[0].removesuffix(".rs")
            n = count(f)
            if target in per:
                per[target] += n
            else:
                shared += n
        row = {"impl": name, "shared": shared}
        for t in TARGETS:
            row[t] = per[t] if implemented(name, t) else None
        row["total"] = shared + sum(row[t] or 0 for t in TARGETS)
        rows.append(row)
    if args.json:
        print(json.dumps(rows, indent=1))
        return
    head = ["impl"] + TARGETS + ["shared", "total"]
    print("| " + " | ".join(head) + " |")
    print("|" + "---|" * len(head))
    for r in rows:
        print("| " + " | ".join(str(r[h]) if r[h] is not None else "-" for h in head) + " |")


# ---------------------------------------------------------------- compile time

def cargo(args, env, cwd=ROOT):
    t = time.monotonic()
    r = subprocess.run(["cargo"] + args, cwd=cwd, env=env, capture_output=True, text=True)
    if r.returncode != 0:
        sys.stderr.write(r.stderr[-4000:])
        raise SystemExit(f"cargo {' '.join(args)} failed")
    return time.monotonic() - t


def compile_time(args):
    names = args.impl or IMPLS
    base = Path(args.root or (os.environ.get("CARGO_TARGET_DIR", str(ROOT / "target")) + "-metrics"))
    OUT.mkdir(parents=True, exist_ok=True)
    profile = ["--release"] if args.release else []
    rows = []
    for name in names:
        shutil.rmtree(base / name, ignore_errors=True)
        env = dict(os.environ, CARGO_TARGET_DIR=str(base / name), CARGO_INCREMENTAL="1")
        pkg = f"impl-{name}"
        clean = cargo(["build", "--locked", "-p", pkg] + profile, env)
        cargo(["clean", "-p", pkg] + profile, env)
        crate = cargo(["build", "--locked", "-p", pkg] + profile, env)
        (impl_dir(name) / "src" / "lib.rs").touch()
        incr = cargo(["build", "--locked", "-p", pkg] + profile, env)
        row = {"impl": name, "profile": "release" if args.release else "dev", "clean_s": round(clean, 2), "crate_s": round(crate, 2), "incr_s": round(incr, 2), "jobs": os.environ.get("CARGO_BUILD_JOBS", "")}
        rows.append(row)
        with open(OUT / "compile.jsonl", "a") as f:
            f.write(json.dumps(row) + "\n")
        print(f"{name}: clean {clean:.2f}s, crate {crate:.2f}s, incremental {incr:.2f}s", flush=True)
    print("\n| impl | profile | clean (s) | crate only (s) | incremental (s) |\n|---|---|---|---|---|")
    for r in rows:
        print(f"| {r['impl']} | {r['profile']} | {r['clean_s']} | {r['crate_s']} | {r['incr_s']} |")


# ---------------------------------------------------------------- sizes

def sizes(args):
    r = subprocess.run(["cargo", "run", "-q", "--locked", "--release", "-p", "bench", "--bin", "sizes"], cwd=ROOT, capture_output=True, text=True)
    if r.returncode != 0:
        sys.stderr.write(r.stderr)
        raise SystemExit("sizes failed")
    table = {}
    for line in r.stdout.splitlines()[1:]:
        name, target, n = line.split("\t")
        table.setdefault(name, {})[target] = n
    head = ["impl"] + TARGETS
    print("| " + " | ".join(head) + " |")
    print("|" + "---|" * len(head))
    for name, per in table.items():
        print("| " + " | ".join([name] + [per.get(t, "-") for t in TARGETS]) + " |")


# ---------------------------------------------------------------- compile-fail

def probes():
    """Probe ids per target, from each SPEC.md's illegal-operations list."""
    out = {}
    for t in TARGETS:
        text = (ROOT / "spec" / "src" / t / "SPEC.md").read_text()
        out[t] = re.findall(r"\*\*([A-Z]\d+) [a-z0-9-]+:\*\*", text)
    return out


def compile_fail(args):
    ids = probes()
    cases = ROOT / "compile-fail" / "cases"
    kinds = {"reject": "R", "runtime": "T", "accept": "A"}
    verified = {}
    if args.run:
        for name in IMPLS:
            r = subprocess.run(["cargo", "test", "-q", "--locked", "-p", f"impl-{name}", "--test", "compile_fail"], cwd=ROOT, capture_output=True, text=True)
            verified[name] = r.returncode == 0
    for t in TARGETS:
        head = ["impl"] + ids[t] + ["forge"]
        print(f"\n{t}\n\n| " + " | ".join(head) + " |")
        print("|" + "---|" * len(head))
        for name in IMPLS:
            cells = []
            for p in ids[t]:
                found = [kinds[k] for k in kinds for _ in (cases / name / t / k).glob(f"{p}-*.rs")] if (cases / name / t).exists() else []
                cells.append("/".join(found) if found else "-")
            forged = sorted(f.stem for f in (cases / name / t / "forge").glob("F-*.rs"))
            cells.append(", ".join(forged) if forged else "-")
            mark = "" if name not in verified else (" (ok)" if verified[name] else " (FAILING)")
            print("| " + " | ".join([name + mark] + cells) + " |")


# ---------------------------------------------------------------- conformance

def conformance(args):
    names = args.impl or IMPLS
    OUT.mkdir(parents=True, exist_ok=True)
    results = OUT / "conformance.jsonl"
    results.unlink(missing_ok=True)
    for name in names:
        extra = ["--features", "fixed"] if args.fixed and name == "implicit" else []
        env = dict(os.environ, SME_RESULTS=str(results))
        r = subprocess.run(["cargo", "test", "-q", "--locked", "-p", f"impl-{name}", "--test", "spec"] + extra + ["--", "conformance"], cwd=ROOT, env=env, capture_output=True, text=True)
        print(f"impl-{name}: tests {'pass' if r.returncode == 0 else 'FAIL'}")
    rows = [json.loads(l) for l in results.read_text().splitlines()] if results.exists() else []
    table = {}
    for row in rows:
        short = row["impl"].removeprefix("impl_").replace("_", "-")
        if not implemented(short, row["target"]):
            continue
        cell = table.setdefault(row["impl"], {}).setdefault(row["target"], [0, 0, []])
        cell[1] += 1
        if row["pass"]:
            cell[0] += 1
        else:
            cell[2].append(row["trace"] + (f" ({row['bug']})" if row["bug"] else ""))
    head = ["impl"] + TARGETS
    print("\n| " + " | ".join(head) + " |")
    print("|" + "---|" * len(head))
    for name, per in table.items():
        print("| " + " | ".join([name] + [f"{per[t][0]}/{per[t][1]}" if t in per else "-" for t in TARGETS]) + " |")
    for name, per in table.items():
        for t, (_, _, failed) in per.items():
            for f in failed:
                print(f"  {name} {t}: fails {f}")


def main():
    p = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    sub = p.add_subparsers(dest="cmd", required=True)
    s = sub.add_parser("loc")
    s.add_argument("--json", action="store_true")
    s = sub.add_parser("compile")
    s.add_argument("--impl", action="append")
    s.add_argument("--root")
    s.add_argument("--release", action="store_true")
    sub.add_parser("sizes")
    s = sub.add_parser("compile-fail")
    s.add_argument("--run", action="store_true")
    s = sub.add_parser("conformance")
    s.add_argument("--impl", action="append")
    s.add_argument("--fixed", action="store_true")
    a = p.parse_args()
    {"loc": loc, "compile": compile_time, "sizes": sizes, "compile-fail": compile_fail, "conformance": conformance}[a.cmd](a)


if __name__ == "__main__":
    main()

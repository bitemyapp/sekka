#!/usr/bin/env python3
"""Builds the tables of results.md from results/raw/ (run from anywhere:
paths are relative to this file). Prints Markdown on stdout.

Every number comes from a file in raw/; the section headings name them.
"""

import json
import re
import statistics
from pathlib import Path

RES = Path(__file__).resolve().parent.parent
RAW = RES / "raw"
TARGETS = ["pointer", "wrap", "polkit", "transfer", "lock", "install"]
IMPLS = ["implicit", "enum", "typestate", "statig", "smlang", "sml", "finny", "fsmentry", "statum", "state-machines", "rust-fsm"]
GROUPS = ["pointer-stream", "wrap-80", "wrap-200"] + [f"replay-{t}" for t in TARGETS]
ANSI = re.compile(r"\x1b\[[0-9;]*m")


def table(head, rows):
    out = ["| " + " | ".join(head) + " |", "|" + "|".join("---" for _ in head) + "|"]
    out += ["| " + " | ".join(str(c) for c in r) + " |" for r in rows]
    return "\n".join(out)


def fmt_int(n):
    return f"{n:,}".replace(",", " ")


def ratio(x, ref):
    return f"{x / ref:.2f}" if ref else "-"


# ------------------------------------------------------------ correctness

def conformance(name):
    per = {}
    for line in (RAW / name).read_text().splitlines():
        r = json.loads(line)
        impl = r["impl"].removeprefix("impl_").replace("_", "-")
        cell = per.setdefault(impl, {}).setdefault(r["target"], [0, 0, []])
        cell[1] += 1
        if r["pass"]:
            cell[0] += 1
        else:
            cell[2].append(f"{r['trace']} ({r['bug']})" if r.get("bug") else r["trace"])
    return per


def proptest_logs():
    """Per implementation: [(test binary, test name, outcome)], and the summary lines."""
    out = {}
    for impl in IMPLS:
        text = ANSI.sub("", (RAW / f"proptest-{impl}.txt").read_text())
        tests, binary, results = [], None, []
        for line in text.splitlines():
            m = re.match(r"\s+Running (\S+)", line)
            if m:
                binary = m.group(1)
            m = re.match(r"test (\w+) \.\.\. (ok|FAILED|ignored.*)$", line)
            if m:
                tests.append((binary, m.group(1), m.group(2)))
            m = re.match(r"test result: (\w+)\. (\d+) passed; (\d+) failed; (\d+) ignored", line)
            if m:
                results.append((binary, m.group(1), int(m.group(2)), int(m.group(3)), int(m.group(4))))
        out[impl] = (tests, results)
    return out


def exit_codes():
    codes = {}
    for line in (RAW / "exit-codes.txt").read_text().splitlines():
        k, v = line.rsplit(" ", 1)
        codes[k] = int(v)
    return codes


def section_correctness():
    print("## Correctness\n")
    conf, fixed = conformance("conformance.jsonl"), conformance("conformance-fixed.jsonl")
    logs = proptest_logs()
    codes = exit_codes()
    print("Conformance traces passed per target (`raw/conformance.jsonl`, `raw/conformance.txt`); differential")
    print("property tests at `SME_PROPTEST_CASES=20000`, release (`raw/proptest-<impl>.txt`); smoke digests on the")
    print("full and the instruction-sized workloads (`raw/smoke.txt`, `raw/smoke-small.txt`).\n")
    smoke = smoke_rows("smoke.txt")
    small = smoke_rows("smoke-small.txt")
    rows = []
    for impl in IMPLS:
        cells = [f"{conf[impl][t][0]}/{conf[impl][t][1]}" for t in TARGETS]
        tests, results = logs[impl]
        diff = [t for t in tests if t[1].endswith("_differential")]
        dpass = sum(1 for t in diff if t[2] == "ok")
        dcell = f"{dpass}/{len(diff)}" if diff else "none"
        failed = sum(r[3] for r in results)
        allc = "pass" if codes.get(f"proptest-{impl}") == 0 and failed == 0 else f"FAIL ({failed})"
        def sm(rows_):
            mine = [r for r in rows_ if r[1] == impl]
            bad = [r[0] for r in mine if r[4] != "same as enum"]
            if impl == "enum":
                return f"reference ({len(mine)})"
            if not bad:
                return f"same ({len(mine)}/{len(mine)})"
            if all(r[4] == "differs (bugs kept)" for r in mine if r[4] != "same as enum"):
                return f"{len(mine) - len(bad)}/{len(mine)} same; differ (bugs kept): " + ", ".join(bad)
            return f"{len(mine) - len(bad)}/{len(mine)}; differs: " + ", ".join(f"{r[0]} ({r[4]})" for r in mine if r[4] != "same as enum")
        rows.append([impl] + cells + [dcell, allc, sm(smoke), sm(small)])
    print(table(["impl"] + TARGETS + ["differential (20k)", "all tests", "smoke full", "smoke small"], rows))
    print()
    print("`--features fixed` (impl-implicit with its bugs off, `raw/conformance-fixed.jsonl`): implicit " +
          ", ".join(f"{t} {fixed['implicit'][t][0]}/{fixed['implicit'][t][1]}" for t in TARGETS) + ".\n")
    print("impl-implicit's failing traces with its bugs kept (all are confirmed-bug traces; its test asserts exactly these fail):\n")
    for t in TARGETS:
        for f in conf["implicit"][t][2]:
            print(f"- {t}: {f}")
    print()
    for impl in IMPLS:
        for t in TARGETS:
            if impl != "implicit" and conf[impl][t][2]:
                print(f"- FAILURE {impl} {t}: {conf[impl][t][2]}")


def smoke_rows(name):
    rows = []
    for line in (RAW / name).read_text().splitlines()[1:]:
        m = re.match(r"(\S+)\s+(\S+)\s+(\d+)\s+([\d.]+)\s+(.*)$", line)
        if m:
            rows.append((m.group(1), m.group(2), int(m.group(3)), float(m.group(4)), m.group(5).strip()))
    return rows


# ------------------------------------------------------------ compile-fail

def compile_fail():
    text = (RAW / "compile-fail.txt").read_text()
    blocks = {}
    current = None
    for line in text.splitlines():
        if line.strip() in TARGETS:
            current = line.strip()
            blocks[current] = []
        elif current and line.startswith("| ") and not line.startswith("| impl |"):
            cells = [c.strip() for c in line.strip().strip("|").split("|")]
            blocks[current].append(cells)
        elif current and line.startswith("| impl |"):
            blocks[current + ":head"] = [c.strip() for c in line.strip().strip("|").split("|")]
    return blocks


def section_compile_fail():
    blocks = compile_fail()
    print("## Compile-fail matrix\n")
    print("From `raw/compile-fail.txt` (`sme.py compile-fail --run`; `(ok)` means the slot's trybuild test passed).")
    print("R rejected at compile time, T compiles and is refused at run time, A compiles and the illegal operation")
    print("happens, - no case (not applicable). Forge: the case that forges the state through public API the crate")
    print("generates (IMPLEMENTING.md, \"Transitions and forging\").\n")
    totals = {}
    for t in TARGETS:
        head = blocks[t + ":head"]
        print(f"### {t}\n")
        print(table(head, blocks[t]))
        print()
        for cells in blocks[t]:
            name = cells[0].split(" ")[0]
            tot = totals.setdefault(name, {"R": 0, "T": 0, "A": 0, "-": 0, "forge": 0, "ok": True, "multi": []})
            if "(ok)" not in cells[0]:
                tot["ok"] = False
            for p, c in zip(head[1:-1], cells[1:-1]):
                for k in c.split("/"):
                    tot[k] += 1
                if "/" in c:
                    tot["multi"].append(p)
            if cells[-1] != "-":
                tot["forge"] += 1
    print("### Totals (36 probes per implementation)\n")
    rows = []
    for impl in IMPLS:
        tot = totals[impl]
        rows.append([impl, tot["R"], tot["T"], tot["A"], tot["-"], tot["forge"], "ok" if tot["ok"] else "FAILING"])
    print(table(["impl", "R (compile time)", "T (run time)", "A (accepted)", "- (n/a)", "targets with a forge case", "trybuild"], rows))
    print()
    return totals


# ------------------------------------------------------------ LoC and sizes

def section_loc_sizes():
    print("## Lines of code\n")
    print("`raw/loc.json` (`sme.py loc`: non-blank, non-comment, without `#[cfg(test)]`; `cargo fmt --all --check` passed, `raw/fmt-check.txt`).\n")
    rows = json.loads((RAW / "loc.json").read_text())
    by = {r["impl"]: r for r in rows}
    out = []
    for impl in IMPLS:
        r = by[impl]
        out.append([impl] + [r[t] if r[t] is not None else "-" for t in TARGETS] + [r["shared"], r["total"], ratio(r["total"], by["enum"]["total"])])
    print(table(["impl"] + TARGETS + ["shared", "total", "total / enum"], out))
    print()
    print("## Sizes\n")
    print("`size_of` of each machine type in bytes (`raw/sizes.tsv`, `bench --bin sizes`, release).\n")
    sizes = {}
    for line in (RAW / "sizes.tsv").read_text().splitlines()[1:]:
        name, target, n = line.split("\t")
        sizes.setdefault(name, {})[target] = int(n)
    out = [[impl] + [sizes[impl].get(t, "-") for t in TARGETS] for impl in IMPLS]
    out.append(["(spec Event)"] + [sizes["spec-event"][t] for t in TARGETS])
    out.append(["(spec Effect)"] + [sizes["spec-effect"][t] for t in TARGETS])
    print(table(["impl"] + TARGETS, out))
    print()
    return by


# ------------------------------------------------------------ compile time

def section_compile():
    print("## Compile time\n")
    print("`raw/compile.jsonl` (`sme.py compile`, 3 repetitions per profile, interleaved dev/release, under the")
    print("benchmark lock, `CARGO_BUILD_JOBS=8`, unpinned; dev builds without debuginfo, as the wrapper sets).")
    print("clean: the crate and all its dependencies from an empty target dir; crate: `cargo clean -p` the crate,")
    print("then build it; incr: touch `src/lib.rs`, build (`CARGO_INCREMENTAL=1`). Median of 3, seconds,")
    print("with [min-max].\n")
    rows = [json.loads(l) for l in (RAW / "compile.jsonl").read_text().splitlines() if l.strip()]
    data = {}
    for r in rows:
        data.setdefault((r["impl"], r["profile"]), []).append(r)
    med = {}
    for profile in ["dev", "release"]:
        print(f"### {profile}\n")
        out = []
        for impl in IMPLS:
            rs = data.get((impl, profile), [])
            cells = [impl, len(rs)]
            for k in ["crate_s", "incr_s", "clean_s"]:
                v = [r[k] for r in rs]
                if v:
                    m = statistics.median(v)
                    med[(impl, profile, k)] = m
                    cells.append(f"{m:.2f} [{min(v):.2f}-{max(v):.2f}]")
                else:
                    cells.append("-")
            out.append(cells)
        print(table(["impl", "n", "crate only", "incremental", "clean (with deps)"], out))
        print()
    return med


# ------------------------------------------------------------ workload sizes

def counts():
    c = {}
    for line in (RAW / "counts.tsv").read_text().splitlines()[1:]:
        suite, workload, events, prints, repeats, puts, effects = line.split("\t")
        c[(suite, workload)] = {"events": int(events), "puts": None if puts == "-" else int(puts)}
    # Replay sets: events per set, from the smoke run's event column (same sets).
    for (w, impl, events, _, _) in smoke_rows("smoke.txt"):
        if w.startswith("replay-") and impl == "enum":
            c[("instructions", w)] = c[("criterion", w)] = {"events": events, "puts": None}
    for (w, impl, events, _, _) in smoke_rows("smoke-small.txt"):
        if impl == "enum" and w in ("pointer-stream", "wrap-80", "wrap-200"):
            assert c[("instructions", w)]["events"] == events, (w, events)
    for (w, impl, events, _, _) in smoke_rows("smoke.txt"):
        if impl == "enum" and w in ("pointer-stream", "wrap-80", "wrap-200"):
            assert c[("criterion", w)]["events"] == events, (w, events)
    return c


def unit(c, suite, group):
    """(divisor, unit name): printed cells for wrap, events otherwise."""
    d = c[(suite, group)]
    if group.startswith("wrap-"):
        return d["puts"], "cell"
    return d["events"], "event"


# ------------------------------------------------------------ criterion

def criterion(run):
    """{(group, impl): (median, lo, hi)} in ns per iteration, from the new/estimates.json
    files consolidated in criterion-<run>.jsonl (consolidate.py)."""
    out = {}
    base = RAW / ((run if isinstance(run, str) else f"criterion-run{run}") + ".jsonl")
    for line in base.read_text().splitlines():
        r = json.loads(line)
        group, impl = r["group"], r["impl"]
        m = r["estimates"]["median"]
        ci = m["confidence_interval"]
        out[(group, impl)] = (m["point_estimate"], ci["lower_bound"], ci["upper_bound"])
    return out


def section_criterion(c, smoke_bad):
    print("## Wall-clock (criterion), the secondary number\n")
    print("Median time per iteration from `raw/criterion-run<N>.jsonl` (each `<group>/<impl>/new/estimates.json`; criterion's")
    print("bootstrap 95% confidence interval of the median), divided by the workload's events (printed cells for")
    print("wrap-80/200: 1M pointer events; wrap streams of ~10M cells; one replay set of every trace). Pinned to")
    print("CPU 3 under the benchmark lock; loads in `raw/criterion-run<N>-load.log`. Ratios are to enum within")
    print("the same run. `!` marks a result whose two runs differ by more than 10%.\n")
    runs = {r: criterion(r) for r in (1, 2)}
    flagged = []
    print("### Summary: median ns per unit (run 1), and the ratio to enum in run 1 / run 2\n")
    rows = []
    for impl in IMPLS:
        cells = ["implicit (bugs kept)" if impl == "implicit" else impl]
        for g in GROUPS:
            div, _ = unit(c, "criterion", g)
            v1, v2 = runs[1].get((g, impl)), runs[2].get((g, impl))
            e1, e2 = runs[1].get((g, "enum")), runs[2].get((g, "enum"))
            cells.append(f"{v1[0] / div:.2f} ({v1[0] / e1[0]:.2f} / {v2[0] / e2[0]:.2f})" if v1 and v2 else "-")
        rows.append(cells)
    print(table(["impl"] + GROUPS, rows))
    print()
    for g in GROUPS:
        div, u = unit(c, "criterion", g)
        print(f"### {g} (ns/{u}; {fmt_int(div)} {u}s per iteration)\n")
        rows = []
        for impl in IMPLS:
            cells = [impl]
            vals = []
            for r in (1, 2):
                v = runs[r].get((g, impl))
                ref = runs[r].get((g, "enum"))
                if not v:
                    cells += ["-", "-"]
                    vals.append(None)
                    continue
                cells.append(f"{v[0] / div:.3f} [{v[1] / div:.3f}, {v[2] / div:.3f}]")
                cells.append(ratio(v[0], ref[0]) if ref else "-")
                vals.append(v[0])
            if None not in vals:
                d = abs(vals[1] - vals[0]) / min(vals)
                cells.append(f"{100 * (vals[1] - vals[0]) / vals[0]:+.1f}%" + (" !" if d > 0.10 else ""))
                if d > 0.10:
                    flagged.append((g, impl, vals[0] / div, vals[1] / div))
            else:
                cells.append("-")
            if (g, impl) in smoke_bad:
                cells[0] += " (DIFFERS FROM ENUM: excluded)"
            rows.append(cells)
        print(table(["impl", "run 1 median [95% CI]", "/ enum", "run 2 median [95% CI]", "/ enum", "run 2 vs 1"], rows))
        print()
    if (RAW / "criterion-lto.jsonl").exists() and exit_codes().get("criterion-lto-replay-install") == 0:
        lto = criterion("criterion-lto")
        print("### With Hotaru's release settings (thin LTO, one codegen unit), one run\n")
        print("`raw/criterion-lto.jsonl` (built with `CARGO_PROFILE_BENCH_LTO=thin CARGO_PROFILE_BENCH_CODEGEN_UNITS=1`,")
        print("run once, pinned and locked like the others; loads in `raw/criterion-lto-load.log`). Median ns per")
        print("unit (ratio to enum in this run).\n")
        rows = []
        for impl in IMPLS:
            cells = ["implicit (bugs kept)" if impl == "implicit" else impl]
            for g in GROUPS:
                div, _ = unit(c, "criterion", g)
                v, ref = lto.get((g, impl)), lto.get((g, "enum"))
                cells.append(f"{v[0] / div:.2f} ({v[0] / ref[0]:.2f})" if v and ref else "-")
            rows.append(cells)
        print(table(["impl"] + GROUPS, rows))
        print()
    print("### Run-to-run differences above 10%\n")
    if flagged:
        print(table(["group", "impl", f"run 1", "run 2"], [[g, i, f"{a:.3f}", f"{b:.3f}"] for g, i, a, b in flagged]))
    else:
        print("None.")
    print()
    return runs, flagged


# ------------------------------------------------------------ instruction counts

BENCH = {"pointer-stream": "pointer_stream", "wrap-80": "wrap_80", "wrap-200": "wrap_200", **{f"replay-{t}": f"replay_{t}" for t in TARGETS}}


def gungraun(name):
    """{(bench function, impl): instructions} from a gungraun terminal log."""
    out, key = {}, None
    for line in ANSI.sub("", (RAW / name).read_text()).splitlines():
        m = re.match(r"instructions::machines::(\w+) (\w+):", line)
        if m:
            key = (m.group(1), m.group(2).rstrip("_").replace("_", "-"))
            continue
        m = re.match(r"\s+Instructions:\s+(\d+)\|", line)
        if m and key:
            out[key] = int(m.group(1))
            key = None
    return out


def per_unit(ir, stub, c, g):
    div, _ = unit(c, "instructions", g)
    return (ir - stub) / div


def section_instructions(c, smoke_bad):
    allb, stubb = gungraun("instructions.txt"), gungraun("instructions-stub.txt")
    stub = {}
    for g, b in BENCH.items():
        vals = {stubb[(b, i)] for i in IMPLS[3:]}
        assert len(vals) == 1, (g, vals)
        stub[g] = vals.pop()
    print("## Instruction counts, the primary number\n")
    print("Instructions (callgrind `Ir`, gungraun 0.20) of one run of each workload, from `raw/instructions.txt`,")
    print("minus the harness's fixed overhead, then divided by the workload's size: per event, and per printed")
    print("cell (a `Put` of the reference) for wrap-80/200. Workloads: 20 000 pointer events; 201 227 and 208 057")
    print("printed cells (208 510 and 213 282 events) at 80 and 200 columns; one replay set of every trace")
    print("(`raw/counts.tsv`, `raw/smoke-small.txt`). The overhead is what the crate slots report in a build with")
    print("every crate slot off (`raw/instructions-stub.txt`, all eight identical): " +
          ", ".join(f"{g} {stub[g]}" for g in GROUPS) + ".\n")
    rows, rrows = [], []
    for impl in IMPLS:
        cells, rcells = [impl], [impl]
        for g in GROUPS:
            b = BENCH[g]
            v = per_unit(allb[(b, impl)], stub[g], c, g)
            ref = per_unit(allb[(b, "enum")], stub[g], c, g)
            cells.append(f"{v:.1f}")
            rcells.append(f"{v / ref:.2f}")
        if impl == "implicit":
            cells[0] = rcells[0] = "implicit (bugs kept)"
        rows.append(cells)
        rrows.append(rcells)
    head = ["impl", "pointer-stream /ev", "wrap-80 /cell", "wrap-200 /cell"] + [f"replay-{t} /ev" for t in TARGETS]
    print("### Instructions per event (per printed cell for wrap-80/200)\n")
    print(table(head, rows))
    print()
    print("### Ratio to enum\n")
    print(table(["impl"] + GROUPS, rrows))
    print()
    # The same implementations in other builds of the bench crate.
    solo = {}
    for f in sorted(RAW.glob("instructions-solo-*.txt")):
        if f.name.endswith("-build.txt") or exit_codes().get(f.stem) != 0:
            continue
        solo[f.stem.removeprefix("instructions-solo-")] = gungraun(f.name)
    if solo:
        print("### Build sensitivity\n")
        print("The same benchmarks compiled into differently composed bench crates: `all` is the measurement")
        print("above (every slot); `solo` has the three baselines plus one crate slot (`raw/instructions-solo-<slot>.txt`);")
        print("`stub` has the baselines only. Each cell: ratio to enum in the `all` build / ratio to enum in that")
        print("slot's `solo` build (enum measured in the same build).\n")
        rows = []
        for s_ in IMPLS[3:]:
            if s_ not in solo:
                continue
            cells = [s_]
            for g in GROUPS:
                b = BENCH[g]
                a = per_unit(allb[(b, s_)], stub[g], c, g) / per_unit(allb[(b, "enum")], stub[g], c, g)
                o = per_unit(solo[s_][(b, s_)], stub[g], c, g) / per_unit(solo[s_][(b, "enum")], stub[g], c, g)
                cells.append(f"{a:.2f} / {o:.2f}")
            rows.append(cells)
        print(table(["slot"] + GROUPS, rows))
        print()
        print("A slot's own count, solo build against the all build (solo / all - 1):\n")
        rows = []
        for s_ in IMPLS[3:]:
            if s_ not in solo:
                continue
            cells = [s_]
            for g in GROUPS:
                b = BENCH[g]
                a = per_unit(allb[(b, s_)], stub[g], c, g)
                o = per_unit(solo[s_][(b, s_)], stub[g], c, g)
                cells.append(f"{100 * (o / a - 1):+.0f}%")
            rows.append(cells)
        print(table(["slot"] + GROUPS, rows))
        print()
        print("The baselines' counts per unit across all builds (all, stub and every solo build): min-max.\n")
        rows = []
        builds = [allb, stubb] + list(solo.values())
        for impl in IMPLS[:3]:
            cells = [impl]
            for g in GROUPS:
                b = BENCH[g]
                v = [per_unit(d[(b, impl)], stub[g], c, g) for d in builds]
                cells.append(f"{min(v):.1f}-{max(v):.1f}")
            rows.append(cells)
        print(table(["baseline"] + GROUPS, rows))
        print()
    codes = exit_codes()
    if codes.get("instructions-lto-all") == 0 and codes.get("instructions-lto-stub") == 0:
        la, ls = gungraun("instructions-lto-all.txt"), gungraun("instructions-lto-stub.txt")
        lstub = {}
        for g, b in BENCH.items():
            vals = {ls[(b, i)] for i in IMPLS[3:]}
            assert len(vals) == 1, (g, vals)
            lstub[g] = vals.pop()
        print("### With Hotaru's release settings (thin LTO, one codegen unit)\n")
        print("The same all-slot benchmark built with `CARGO_PROFILE_BENCH_LTO=thin CARGO_PROFILE_BENCH_CODEGEN_UNITS=1`,")
        print("the settings of Sekka's `apps/hotaru/Cargo.toml` (`raw/instructions-lto-all.txt`; overhead from")
        print("`raw/instructions-lto-stub.txt`: " + ", ".join(f"{g} {lstub[g]}" for g in GROUPS) + "). Per unit, then the ratio to enum.\n")
        rows = []
        for impl in IMPLS:
            cells = ["implicit (bugs kept)" if impl == "implicit" else impl]
            for g in GROUPS:
                b = BENCH[g]
                v = per_unit(la[(b, impl)], lstub[g], c, g)
                ref = per_unit(la[(b, "enum")], lstub[g], c, g)
                cells.append(f"{v:.1f} ({v / ref:.2f})")
            rows.append(cells)
        print(table(["impl"] + GROUPS, rows))
        print()
    return allb, stub, solo


# ------------------------------------------------------------ loads

def loads(name):
    """[(label, when, loadavg 1/5/15, {cpu: (busy, total)})] from a load log."""
    entries, cur = [], None
    for line in (RAW / name).read_text().splitlines():
        m = re.match(r"== (.*): (before|after) (\S+)$", line)
        if m:
            cur = {"label": m.group(1), "when": m.group(2), "time": m.group(3), "cpu": {}}
            entries.append(cur)
            continue
        if cur is None:
            continue
        if "load" not in cur and re.match(r"[\d.]+ [\d.]+ [\d.]+ ", line):
            cur["load"] = line.split()[:3]
        m = re.match(r"(cpu\d+) (.*)", line)
        if m:
            f = [int(x) for x in m.group(2).split()]
            cur["cpu"][m.group(1)] = (sum(f[:8]) - f[3] - f[4], sum(f[:8]))
    return entries


def section_loads():
    print("## Load during the measurements\n")
    print("1-minute load average before and after each locked step, and the busy share of CPU 3 (the pinned")
    print("benchmark) and CPU 11 (its SMT sibling) between the two, from `/proc/stat` (`raw/*-load.log`; the")
    print("logs also hold the top 10 processes by CPU).\n")
    rows = []
    for name in ["compile-load.log", "smoke-load.log", "instructions-load.log", "criterion-run1-load.log", "criterion-run2-load.log", "criterion-lto-load.log"]:
        if not (RAW / name).exists():
            continue
        es = loads(name)
        for b, a in zip(es[::2], es[1::2]):
            assert b["label"] == a["label"] and b["when"] == "before" and a["when"] == "after"
            busy = {}
            for cpu in ("cpu3", "cpu11"):
                db = a["cpu"][cpu][0] - b["cpu"][cpu][0]
                dt = a["cpu"][cpu][1] - b["cpu"][cpu][1]
                busy[cpu] = f"{100 * db / dt:.0f}%" if dt else "-"
            rows.append([b["label"], b["time"][11:19], a["time"][11:19], b["load"][0], a["load"][0], busy["cpu3"], busy["cpu11"]])
    print(table(["step", "start", "end", "load before", "load after", "CPU 3 busy", "CPU 11 busy"], rows))
    print()


# ------------------------------------------------------------ perf cross-check

def section_perf(c, ins, crit):
    path = RAW / "perf-check.tsv"
    if not path.exists():
        return
    allb, stub, _ = ins
    perf = {}
    for line in path.read_text().splitlines()[1:]:
        which, g, impl, n = line.split("\t")
        if n:
            perf[(which, g, impl)] = int(n)
    lto = criterion("criterion-lto") if (RAW / "criterion-lto.jsonl").exists() else {}
    print("## Cross-check: instructions in the timed binaries\n")
    print("criterion times a different binary (`benches/machines.rs`) from the one callgrind counts")
    print("(`benches/instructions.rs`), and the two inline differently. `raw/perf-check.tsv` counts")
    print("user-space instructions (`perf stat -e instructions:u`) of one run of each stream in the criterion")
    print("binaries (criterion's test mode, minus a run whose filter matches nothing in the group: the workload")
    print("generation). Per unit: callgrind in the instruction binary / perf in the default criterion binary /")
    print("perf in the LTO criterion binary; then instructions per ns in the default binary (perf count over the")
    print("run-1 median).\n")
    for g in ["pointer-stream", "wrap-80", "wrap-200"]:
        div, u = unit(c, "criterion", g)
        print(f"### {g} (per {u})\n")
        rows = []
        for impl in IMPLS:
            gv = per_unit(allb[(BENCH[g], impl)], stub[g], c, g)
            cells = ["implicit (bugs kept)" if impl == "implicit" else impl, f"{gv:.1f}"]
            ipns = "-"
            for which in ("default", "lto"):
                n, z = perf.get((which, g, impl)), perf.get((which, g, "none"))
                if n is None or z is None:
                    cells.append("-")
                    continue
                v = (n - z) / div
                cells.append(f"{v:.1f}")
                if which == "default" and crit and (g, impl) in crit[1]:
                    ipns = f"{(n - z) / crit[1][(g, impl)][0]:.1f}"
            cells.append(ipns)
            rows.append(cells)
        print(table(["impl", "callgrind (instructions bench)", "perf, default criterion binary", "perf, LTO criterion binary", "Ir per ns (default)"], rows))
        print()


def capture(f, *args):
    import contextlib
    import io
    buf = io.StringIO()
    with contextlib.redirect_stdout(buf):
        r = f(*args)
    return buf.getvalue(), r


def lto_counts():
    codes = exit_codes()
    if codes.get("instructions-lto-all") != 0 or codes.get("instructions-lto-stub") != 0:
        return None
    la, ls = gungraun("instructions-lto-all.txt"), gungraun("instructions-lto-stub.txt")
    return la, {g: ls[(BENCH[g], "statig")] for g in GROUPS}


def section_summary(c, totals, loc, med, ins, crit):
    allb, stub, _ = ins
    conf = conformance("conformance.jsonl")
    lto = lto_counts()
    print("## Summary\n")
    print("Per implementation: conformance traces passed (of 105); instructions (callgrind Ir) per pointer event and per")
    print("printed wrap cell at 80 columns, with the ratio to enum, in the default bench build and in the")
    print("thin-LTO, one-codegen-unit build; compile-fail totals R/T/A (of 36) and the targets with a forge")
    print("case; lines of code; crate-only dev compile time (median of 3); criterion medians (run 1) for the")
    print("same two workloads.\n")
    rows = []
    for impl in IMPLS:
        passed = sum(conf[impl][t][0] for t in TARGETS)
        cells = ["implicit (bugs kept)" if impl == "implicit" else impl, f"{passed}/105"]
        for g in ("pointer-stream", "wrap-80"):
            b = BENCH[g]
            v = per_unit(allb[(b, impl)], stub[g], c, g)
            ref = per_unit(allb[(b, "enum")], stub[g], c, g)
            cell = f"{v:.0f} ({v / ref:.2f})"
            if lto:
                la, ls = lto
                lv = per_unit(la[(b, impl)], ls[g], c, g)
                lref = per_unit(la[(b, "enum")], ls[g], c, g)
                cell += f"; LTO {lv:.0f} ({lv / lref:.2f})"
            cells.append(cell)
        t = totals[impl]
        cells.append(f"{t['R']}/{t['T']}/{t['A']}")
        cells.append(str(t["forge"]))
        cells.append(str(loc[impl]["total"]))
        cells.append(f"{med[(impl, 'dev', 'crate_s')]:.2f}")
        if crit:
            for g in ("pointer-stream", "wrap-80"):
                div, _ = unit(c, "criterion", g)
                v, ref = crit[1].get((g, impl)), crit[1].get((g, "enum"))
                cells.append(f"{v[0] / div:.2f} ({v[0] / ref[0]:.2f})")
        rows.append(cells)
    head = ["impl", "conformance", "Ir/event pointer (x enum)", "Ir/cell wrap-80 (x enum)", "R/T/A", "forge targets", "LoC", "crate-only dev (s)"]
    if crit:
        head += ["ns/event pointer (x enum)", "ns/cell wrap-80 (x enum)"]
    print(table(head, rows))
    print()


def main():
    c = counts()
    out = []
    out.append(capture(section_correctness)[0])
    text, totals = capture(section_compile_fail)
    out.append(text)
    text, loc = capture(section_loc_sizes)
    out.append(text)
    text, med = capture(section_compile)
    out.append(text)
    bad = {(r[0], r[1]) for r in smoke_rows("smoke.txt") if r[4] == "DIFFERS FROM ENUM"}
    text, ins = capture(section_instructions, c, bad)
    out.append(text)
    crit = None
    if (RAW / "criterion-run2.jsonl").exists():
        text, (crit, _) = capture(section_criterion, c, bad)
        out.append(text)
    out.append(capture(section_perf, c, ins, {1: criterion(1)} if (RAW / "criterion-run1.jsonl").exists() else None)[0])
    out.append(capture(section_loads)[0])
    print(capture(section_summary, c, totals, loc, med, ins, crit)[0], end="")
    print("".join(out), end="")


if __name__ == "__main__":
    main()

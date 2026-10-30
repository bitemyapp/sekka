#!/usr/bin/env python3
"""Feed-only timing of the Ghostty suite's generated inputs, screens checked first.

Regenerates each 64 MiB input as upstream/stream-parser/suite-recheck.py does,
confirms the final screen matches the pinned libghostty-vt, then times only the
feed of the reference adapter and hotaru-replay (zero history, 120x80): four
rounds of five feeds, alternating order, on one CPU under the workstation lock.
The ASCII and OSC generators take time-based seeds. Paths are wx's.

Usage: suite-feed.py REPLAY REPORT
"""
import fcntl, json, statistics, subprocess, sys
from pathlib import Path

sys.path.insert(0, "/home/callen/work/sekka-hotaru-settings/apps/hotaru/tools")
sys.path.insert(0, "/home/callen/work/sekka-hotaru-settings/apps/hotaru/upstream/stream-parser")
from differential import Oracle, differences, digest  # noqa: E402
import importlib.util
spec = importlib.util.spec_from_file_location("recheck", "/home/callen/work/sekka-hotaru-settings/apps/hotaru/upstream/stream-parser/suite-recheck.py")
recheck = importlib.util.module_from_spec(spec); spec.loader.exec_module(recheck)

replay, report_path = Path(sys.argv[1]), Path(sys.argv[2])
data_dir = Path("/dev/shm/hotaru-suite-feed")
data_dir.mkdir(exist_ok=True)
adapters = {"ghostty": recheck.REF / "zig-out/bin/ghostty-reference-replay", "hotaru": replay}
oracle = Oracle(recheck.LIBRARY)
report = {"scope": __doc__, "ghostty_gen_sha256": digest(recheck.GEN), "library_sha256": digest(recheck.LIBRARY),
          "adapter_sha256": {k: digest(v) for k, v in adapters.items()}, "rows": 80, "cols": 120, "cpu": 8,
          "input_bytes": recheck.SIZE, "inputs": {}}
with Path("/home/callen/.cache/hotaru-workstation.lock").open("a") as lock:
    fcntl.flock(lock, fcntl.LOCK_EX)
    for name, args in recheck.INPUTS.items():
        path = data_dir / f"{name}.vt"
        with path.open("wb") as out:
            gen = subprocess.Popen([str(recheck.GEN), *args], stdout=subprocess.PIPE, stderr=subprocess.DEVNULL)
            left = recheck.SIZE
            while left:
                chunk = gen.stdout.read(min(left, 1 << 20))
                if not chunk:
                    break
                out.write(chunk)
                left -= len(chunk)
            gen.kill()
            gen.wait()
        data = path.read_bytes()
        state = subprocess.run([str(replay), "--data", str(path), "--rows", "80", "--cols", "120", "--format", "json"],
                               capture_output=True, text=True, timeout=600, check=True)
        delta = differences(oracle.snapshot(data, 80, 120, 65536), json.loads(state.stdout))
        entry = {"generator": args, "sha256": digest(path), "state_matched": not delta}
        if not delta:
            samples = {engine: [] for engine in adapters}
            for round_id in range(4):
                order = list(adapters) if round_id % 2 == 0 else list(reversed(adapters))
                for engine in order:
                    result = subprocess.run(["taskset", "-c", "8", str(adapters[engine]), "--data", str(path),
                                             "--rows", "80", "--cols", "120", "--bench-json", "--iterations", "5"],
                                            capture_output=True, text=True, check=True, timeout=1200)
                    samples[engine].extend(json.loads(result.stdout)["samples_ns"])
            medians = {engine: statistics.median(values) / 1e6 for engine, values in samples.items()}
            entry.update(median_feed_ms={k: round(v, 2) for k, v in medians.items()},
                         hotaru_over_ghostty=round(medians["hotaru"] / medians["ghostty"], 3), samples_ns=samples)
        report["inputs"][name] = entry
        path.unlink()
        report_path.write_text(json.dumps(report, indent=2) + "\n")
        print(name, entry["state_matched"], entry.get("median_feed_ms"), entry.get("hotaru_over_ghostty"), flush=True)
data_dir.rmdir()

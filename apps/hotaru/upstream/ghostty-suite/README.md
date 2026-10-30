# Hotaru through Ghostty's own benchmark suite

Recorded 2026-10-30 on `wx-workstation` for `2f2f73e0`; the results are discussed
in [`docs/ghostty-comparison-2026-10-30.md`](../../docs/ghostty-comparison-2026-10-30.md).

- `run.py`:
  - Generates 64 MiB inputs with the pinned `ghostty-gen`.
  - Compares each file's final screen between the pinned libghostty-vt and
    `hotaru-replay`.
  - Times `ghostty-bench +terminal-stream` against `hotaru-replay --bench` with
    `hyperfine`, as Ghostty's `src/benchmark/AGENTS.md` prescribes.
  - Its absolute paths are wx's.
- `report.json`:
  - Binary hashes.
  - Each input's generator arguments, hash and state comparison (the first
    differences, where they differ).
  - Median and minimum times, and load averages.
- `*.hyperfine.json`: every run's time, per input and command.
- `feed-only.py` and `feed-only.json`: the in-process, zero-history, feed-only
  cross-check of the five inputs whose screens matched, with every sample.

The generated inputs themselves (64 MiB each) are not kept in the repository.
`report.json` records each one's SHA-256. The ASCII and OSC generators take
time-based seeds, so a rerun produces different files with the same structure.

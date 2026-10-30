# Stream parser and width agreement evidence

Recorded 2026-10-30 on `wx-workstation` against the pinned libghostty-vt
(`70e1bdcf…`); the rules are described in
[`docs/stream-parser.md`](../../docs/stream-parser.md). The scripts' absolute
paths are wx's.

## Records

`oracle-report.json`, `sweep.json` and `suite-recheck.json` are from the ED,
EL and back-tab build
([`../candidate-erase-modes-build.json`](../candidate-erase-modes-build.json)).

- **`oracle-report.json`:** `tools/stream-parser-oracle.py` comparing every
  case at chunk sizes of 1, 7 and 65,536 bytes. Its compact expectations are
  [`../stream-parser-fixtures.json`](../stream-parser-fixtures.json), which the
  Cargo test reads.
- **`sweep.json`:** the CSI sweep. It runs once with the cursor away from home,
  and once with a wrap pending, which it reports with a `wrap:` prefix. Only
  the four named differences remain, under both.
- **`suite-recheck.py` and `suite-recheck.json`:** the final screens of the
  Ghostty suite's seven 64 MiB generated inputs (see
  [`../ghostty-suite/`](../ghostty-suite/)), without timing.
- **`dispatch-ab.json`:** back-to-back `tools/benchmark.py` runs of the
  parent, the fix as first written, and six variants, which separated its
  styled-text cost. The core benchmarks of the final build are
  [`../benchmarks/stream-parser-*.json`](../benchmarks/).

## Search tools

These found the cases in the oracle:

- **`divergence.py`:** grows a prefix of an input geometrically, then
  binary-searches for the first length whose screen differs.
- **`widths.py`:** prints each scalar after an `A` through `hotaru-replay` and
  compares the width with `ghostty_unicode_codepoint_width`.
- **`fuzz.py`:** seeded control-heavy byte strings, minimizing each differing
  case.
- **`sweep.py`:** every CSI final byte with each private marker and
  intermediate, 0–4 parameters from six value sets, then cursor, DECRQSS and
  tab-stop probes.
- **`modes.py`:** DECRQM for every ANSI mode below 100 and DEC mode below
  10,000, before and after setting each.

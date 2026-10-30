# History budget check evidence

Recorded 2026-10-30 on `wx-workstation` for
[`docs/history-budget-check.md`](../../docs/history-budget-check.md). The
candidate is `52ec3c4e…`
([`../candidate-history-budget-check-build.json`](../candidate-history-budget-check-build.json)).
The scripts' absolute paths are wx's.

## Records

- **`budget-ab.json`:** feed times and instruction counts for four builds:
  - the parent of the stream parser fix;
  - the stream parser build;
  - a variant that checks the budget only in ground state;
  - the adopted inline check, whose binary is byte-identical to the candidate.

  The record also names the 8 MiB inputs that isolate one kind of work.
- **`reference-checks.json`:** a summary of the reference checks against the
  pinned libghostty-vt:
  - the stream parser oracle;
  - the CSI sweep;
  - 60,000 fresh fuzz cases, including the one older difference they found.
- **`suite-recheck.json`:** the final screens of the Ghostty suite's seven
  64 MiB generated inputs, from `../stream-parser/suite-recheck.py`.
- **`suite-feed.py` and `ghostty-suite-feed.json`:** for each of those inputs,
  the script regenerates it, checks its screen and times only the feed. Both
  engines use their in-process adapters with zero history.

The core benchmarks against the pinned library are
[`../benchmarks/history-budget-check-*.json`](../benchmarks/).

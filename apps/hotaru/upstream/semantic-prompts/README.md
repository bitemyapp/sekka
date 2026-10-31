# Semantic-state checkpoint evidence

Product behavior, validation scope and measured cost are described in
[`../../docs/semantic-prompts.md`](../../docs/semantic-prompts.md).

`oracle.c` observes the unchanged actual Ghostty VT library. `compare.py`
generated the 610 expected states in `../semantic-fixtures.json`; the ordinary
Rust test replays them at three chunk sizes. `initial-comparison.json` retains
the earlier failed state observations; its case list and executable identities
are historical, and the generator was subsequently extended.

`decaln-audit/` preserves the independent 54 failing comparisons, original source
identities, the DECALN fix and the passing 3,090-case audit. `qualification/`
contains the final guard/accessor source manifest and 3,120 passing comparisons
against the exact final semantic adapter. The additional style probe records
its separate pre-guard replay identity. The final workspace logs report 160
passing tests in both profiles, with two existing ignores. Earlier missing-file
and formatter-invocation failures are retained; they are not accepted results.

`performance/` contains the 40-case three-engine run, the interrupted first
attempt, all raw samples/process observations and per-core activity. XZ files
are lossless copies with uncompressed hashes in its summary. Six kernel-activity
flags are retained. The two runs use different documented load guards; the
first attempt is not acceptance evidence. Native build metadata checks source
hashes before and after compilation and the runner checks them around timing.

Archived runners preserve their original paths on `wx-workstation`. The frozen
trees are `hotaru-semantic-prompts-20261028` and
`hotaru-semantic-fastgate-20261028` under `/home/callen/work`. Reproduction needs
the pinned external Ghostty library/toolchain and benchmark corpus locations
identified by those receipts. They are development tools, not application
dependencies. All heavy work takes the shared workstation lock. The full raw
JSON, executables and original gzip copies remain in those external trees.

The outer manifest hashes the archived files and fixture. The final product
source is checked separately against all 116 entries in the native build
receipt. This checkpoint does not qualify a new Nix package or native GUI.

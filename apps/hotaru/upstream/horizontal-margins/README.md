# Horizontal margins (DECSLRM) evidence

Recorded 2026-10-29 on `wx-workstation` (AMD Ryzen Threadripper PRO 9985WX),
under the shared workstation lock, for the integration of the preserved margin
candidate (`/home/callen/work/hotaru-margin-controls-20261028`: margins, the
wide-tail DCH correction and the C0 fast path) onto this branch.

- `oracle-summary.json`: `tools/horizontal-margins.py` against the pinned
  libghostty-vt (`70e1bdcf…`) with the final source's `hotaru-replay`:
  5,184 comparisons at chunks of 1, 7 and 65,536 bytes, no failures, and the
  30 named reference differences the prototype recorded (12 cases, each at
  several chunk sizes, where the reference itself depends on input chunking).
  It lists the hashes of every `hotaru-core` source it ran against. The full
  3.8 MB report stays on wx at `~/sekka-work/hotaru-margins-oracle-final.json`.
- `feed-*.json`: `tools/paired-feed-benchmark.py`, the previous head
  `0d05ce2b` (no margins) against the candidate, feed-only, zero history, 120
  columns, four corpus patterns at 2 MiB/12 rows and 8 MiB/80 rows in both width
  modes; 6 alternating rounds of 10 on one pinned CPU whose SMT sibling was the
  idlest found. Ratios are candidate / baseline medians (below 1 is faster). Raw
  samples, load averages and CPU frequency are in each file. The host is shared
  and was not idle (load averages 13–53), so single cases can be contaminated:
  in run 5 the 8 MiB styled pair swung to 1.83 and 0.85.
  1. `feed-1-merged`, `feed-2-merged`: the candidate as merged, default release
     profile. ASCII 3–8 % slower, mixed Unicode 3–10 % slower, styled and cursor
     input 1–5 % faster.
  2. `feed-3-merged-package-profile`: both engines with the package's thin LTO
     and one codegen unit. The same pattern, Unicode up to 16 % slower.
     Callgrind showed equal instruction counts (+0.07 % Unicode, +0.4 % ASCII);
     `perf stat` on an Intel Core Ultra 7 365 P-core showed 13 times the
     decoded-uop-cache misses: code layout, not work.
  3. `feed-4-split-package-profile`: `print`, `try_grapheme` and the wrap in
     `prepare_wrap` split so the margin-free path is the pre-margin code, as the
     candidate's C0 path already was. ASCII within 1–3 %, Unicode legacy 2 %,
     Unicode with mode 2027 10 % slower; Callgrind found `width_effect` no longer
     inlined (+3.8 M instructions).
  4. `feed-5-final-package-profile`: with `width_effect` always inlined
     (instructions now +0.03 % and +0.13 %). ASCII 0.998–1.007, styled and cursor
     0.94–0.99, mixed Unicode 1.03 legacy and 1.12 with mode 2027. On the Intel
     laptop the same mode-2027 case is +2 % cycles with 5–6 times the DSB misses.

The remaining mode-2027 Unicode cost is instruction-equal and layout-dependent;
see `docs/next-work.md`.

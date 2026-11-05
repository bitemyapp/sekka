## Summary

Per implementation: conformance traces passed (of 105); instructions (callgrind Ir) per pointer event and per
printed wrap cell at 80 columns, with the ratio to enum, in the default bench build and in the
thin-LTO, one-codegen-unit build; compile-fail totals R/T/A (of 36) and the targets with a forge
case; lines of code; crate-only dev compile time (median of 3); criterion medians (run 1) for the
same two workloads.

| impl | conformance | Ir/event pointer (x enum) | Ir/cell wrap-80 (x enum) | R/T/A | forge targets | LoC | crate-only dev (s) | ns/event pointer (x enum) | ns/cell wrap-80 (x enum) |
|---|---|---|---|---|---|---|---|---|---|
| implicit (bugs kept) | 89/105 | 108 (0.88); LTO 76 (0.88) | 81 (1.24); LTO 62 (0.96) | 0/21/15 | 0 | 1422 | 0.24 | 4.61 (0.67) | 4.05 (1.01) |
| enum | 105/105 | 122 (1.00); LTO 86 (1.00) | 65 (1.00); LTO 65 (1.00) | 7/29/0 | 0 | 1195 | 0.25 | 6.85 (1.00) | 4.01 (1.00) |
| typestate | 105/105 | 132 (1.08); LTO 101 (1.18) | 140 (2.16); LTO 136 (2.10) | 32/4/0 | 0 | 2139 | 0.30 | 10.65 (1.55) | 11.17 (2.79) |
| statig | 105/105 | 153 (1.25); LTO 120 (1.40) | 127 (1.96); LTO 118 (1.82) | 7/29/0 | 0 | 1596 | 0.36 | 11.44 (1.67) | 7.95 (1.98) |
| smlang | 105/105 | 160 (1.31); LTO 149 (1.74) | 116 (1.79); LTO 114 (1.76) | 7/29/0 | 0 | 1966 | 0.52 | 8.17 (1.19) | 4.70 (1.17) |
| sml | 105/105 | 138 (1.13); LTO 117 (1.37) | 159 (2.46); LTO 63 (0.98) | 5/31/0 | 0 | 1729 | 0.68 | 7.79 (1.14) | 7.18 (1.79) |
| finny | 105/105 | 246 (2.01); LTO 189 (2.21) | 180 (2.78); LTO 168 (2.60) | 0/32/4 | 0 | 2311 | 0.94 | 12.02 (1.75) | 9.48 (2.37) |
| fsmentry | 105/105 | 152 (1.24); LTO 112 (1.31) | 66 (1.02); LTO 65 (1.00) | 23/10/3 | 5 | 1487 | 0.37 | 6.88 (1.00) | 4.28 (1.07) |
| statum | 105/105 | 180 (1.47); LTO 123 (1.44) | 155 (2.39); LTO 155 (2.39) | 32/4/0 | 6 | 2328 | 1.01 | 4.91 (0.72) | 11.33 (2.83) |
| state-machines | 105/105 | 283 (2.32); LTO 255 (2.98) | 355 (5.49); LTO 358 (5.52) | 26/10/0 | 5 | 2162 | 1.11 | 15.21 (2.22) | 25.60 (6.39) |
| rust-fsm | 105/105 | 153 (1.25); LTO 139 (1.62) | 126 (1.95); LTO 126 (1.95) | 0/32/4 | 6 | 1496 | 0.31 | 8.51 (1.24) | 7.92 (1.98) |

## Correctness

Conformance traces passed per target (`raw/conformance.jsonl`, `raw/conformance.txt`); differential
property tests at `SME_PROPTEST_CASES=20000`, release (`raw/proptest-<impl>.txt`); smoke digests on the
full and the instruction-sized workloads (`raw/smoke.txt`, `raw/smoke-small.txt`).

| impl | pointer | wrap | polkit | transfer | lock | install | differential (20k) | all tests | smoke full | smoke small |
|---|---|---|---|---|---|---|---|---|---|---|
| implicit | 17/21 | 14/14 | 10/16 | 18/20 | 15/18 | 15/16 | none | pass | 3/9 same; differ (bugs kept): pointer-stream, replay-pointer, replay-polkit, replay-transfer, replay-lock, replay-install | 3/9 same; differ (bugs kept): pointer-stream, replay-pointer, replay-polkit, replay-transfer, replay-lock, replay-install |
| enum | 21/21 | 14/14 | 16/16 | 20/20 | 18/18 | 16/16 | none | pass | reference (9) | reference (9) |
| typestate | 21/21 | 14/14 | 16/16 | 20/20 | 18/18 | 16/16 | 6/6 | pass | same (9/9) | same (9/9) |
| statig | 21/21 | 14/14 | 16/16 | 20/20 | 18/18 | 16/16 | 6/6 | pass | same (9/9) | same (9/9) |
| smlang | 21/21 | 14/14 | 16/16 | 20/20 | 18/18 | 16/16 | 6/6 | pass | same (9/9) | same (9/9) |
| sml | 21/21 | 14/14 | 16/16 | 20/20 | 18/18 | 16/16 | 6/6 | pass | same (9/9) | same (9/9) |
| finny | 21/21 | 14/14 | 16/16 | 20/20 | 18/18 | 16/16 | 6/6 | pass | same (9/9) | same (9/9) |
| fsmentry | 21/21 | 14/14 | 16/16 | 20/20 | 18/18 | 16/16 | 6/6 | pass | same (9/9) | same (9/9) |
| statum | 21/21 | 14/14 | 16/16 | 20/20 | 18/18 | 16/16 | 6/6 | pass | same (9/9) | same (9/9) |
| state-machines | 21/21 | 14/14 | 16/16 | 20/20 | 18/18 | 16/16 | 6/6 | pass | same (9/9) | same (9/9) |
| rust-fsm | 21/21 | 14/14 | 16/16 | 20/20 | 18/18 | 16/16 | 6/6 | pass | same (9/9) | same (9/9) |

`--features fixed` (impl-implicit with its bugs off, `raw/conformance-fixed.jsonl`): implicit pointer 21/21, wrap 14/14, polkit 16/16, transfer 20/20, lock 18/18, install 16/16.

impl-implicit's failing traces with its bugs kept (all are confirmed-bug traces; its test asserts exactly these fail):

- pointer: program-started-drag-releases-the-press (KIT-13)
- pointer: release-without-a-press-fires-nothing (UIW-01)
- pointer: release-after-the-pressed-node-is-rebuilt-away (UIW-01)
- pointer: release-after-uncapture (UIW-01)
- polkit: unreachable-helper-backs-off (SHB-03)
- polkit: helper-exits-without-a-verdict-backs-off (SYB-09)
- polkit: a-prompt-resets-the-count (SYB-09)
- polkit: waiting-retry-dismissed (SYB-09)
- polkit: choosing-an-identity-retries-by-hand (SHB-03)
- polkit: identity-change-during-the-back-off (SYB-09)
- transfer: put-back-replace-goes-to-the-trash (HAKO-05)
- transfer: put-back-merge-replaces-into-the-trash (HAKO-05)
- lock: a-lockout-says-why (SHB-12)
- lock: an-expired-password-says-why (SHB-12)
- lock: lock-confirmed-after-resume-keeps-the-next-delay (SYB-12)
- install: deferred-route-checks-the-root-space (GEN-07)

## Compile-fail matrix

From `raw/compile-fail.txt` (`sme.py compile-fail --run`; `(ok)` means the slot's trybuild test passed).
R rejected at compile time, T compiles and is refused at run time, A compiles and the illegal operation
happens, - no case (not applicable). Forge: the case that forges the state through public API the crate
generates (IMPLEMENTING.md, "Transitions and forging").

### pointer

| impl | P1 | P2 | P3 | P4 | P5 | P6 | forge |
|---|---|---|---|---|---|---|---|
| implicit (ok) | A | A | A | A | A | A | - |
| enum (ok) | T | R | R | R | R | R | - |
| typestate (ok) | R | R | R | R | R | R | - |
| statig (ok) | T | R | R | R | R | R | - |
| smlang (ok) | T | R | R | R | R | R | - |
| sml (ok) | T | R | R | R | T | R | - |
| finny (ok) | T | A | T | A | T | T | - |
| fsmentry (ok) | T | R | R | R | R | R | F-as-mut |
| statum (ok) | R | R | R | R | R | R | F-transition-with |
| state-machines (ok) | R | R | R | R | R | T | F-pressed-data-mut |
| rust-fsm (ok) | T | A | T | A | T | A | F-from-state |

### wrap

| impl | W1 | W2 | W3 | W4 | W5 | forge |
|---|---|---|---|---|---|---|
| implicit (ok) | T | T | T | T | T | - |
| enum (ok) | T | T | T | T | T | - |
| typestate (ok) | R | R | R | R | R | - |
| statig (ok) | T | T | T | T | T | - |
| smlang (ok) | T | T | T | T | T | - |
| sml (ok) | T | T | T | T | T | - |
| finny (ok) | T | T | T | T | T | - |
| fsmentry (ok) | A | T | A | T | T | - |
| statum (ok) | R | R | R | R | R | F-transition-with |
| state-machines (ok) | T | T | R | T | T | - |
| rust-fsm (ok) | T | T | T | T | T | F-from-state |

### polkit

| impl | K1 | K2 | K3 | K4 | K5 | K6 | forge |
|---|---|---|---|---|---|---|---|
| implicit (ok) | T | T | A | T | T | A | - |
| enum (ok) | T | T | T | T | T | T | - |
| typestate (ok) | R | T | R | R | R | R | - |
| statig (ok) | T | T | T | T | T | T | - |
| smlang (ok) | T | T | T | T | T | T | - |
| sml (ok) | T | T | T | T | T | T | - |
| finny (ok) | T | T | T | T | T | T | - |
| fsmentry (ok) | R | T | R | R | R | R | F-as-mut |
| statum (ok) | R | T | R | R | R | R | F-transition-with |
| state-machines (ok) | R | T | R | R | R | R | F-starting-data-mut |
| rust-fsm (ok) | T | T | T | T | T | T | F-from-state |

### transfer

| impl | T1 | T2 | T3 | T4 | T5 | T6 | forge |
|---|---|---|---|---|---|---|---|
| implicit (ok) | T | A | T | T | T | T | - |
| enum (ok) | T | T | T | T | T | T | - |
| typestate (ok) | R | R | R | R | T | R | - |
| statig (ok) | T | T | T | T | T | T | - |
| smlang (ok) | T | T | T | T | T | T | - |
| sml (ok) | T | T | T | T | T | T | - |
| finny (ok) | T | T | T | T | T | T | - |
| fsmentry (ok) | R | T | R | R | T | R | F-as-mut |
| statum (ok) | R | R | R | R | T | R | F-transition-with |
| state-machines (ok) | R | T | R | R | T | R | F-probing-data-mut |
| rust-fsm (ok) | T | T | T | T | T | T | F-from-state |

### lock

| impl | L1 | L2 | L3 | L4 | L5 | L6 | L7 | forge |
|---|---|---|---|---|---|---|---|---|
| implicit (ok) | T | T | T | A | A | A | A | - |
| enum (ok) | T | T | T | T | T | R | T | - |
| typestate (ok) | R | T | R | R | T | R | R | - |
| statig (ok) | T | T | T | T | T | R | T | - |
| smlang (ok) | T | T | T | T | T | R | T | - |
| sml (ok) | T | T | T | T | T | T | T | - |
| finny (ok) | T | T | T | T | T | A | T | - |
| fsmentry (ok) | A | T | R | T | T | R | R | F-as-mut |
| statum (ok) | R | T | R | R | T | R | R | F-transition-with |
| state-machines (ok) | R | T | R | R | T | R | R | F-checking-data-mut |
| rust-fsm (ok) | T | T | T | T | T | T | T | F-from-state |

### install

| impl | I1 | I2 | I3 | I4 | I5 | I6 | forge |
|---|---|---|---|---|---|---|---|
| implicit (ok) | T | T | T | A | T | A | - |
| enum (ok) | T | T | T | T | T | R | - |
| typestate (ok) | R | R | R | R | R | R | - |
| statig (ok) | T | T | T | T | T | R | - |
| smlang (ok) | T | T | T | T | T | R | - |
| sml (ok) | T | T | T | T | T | R | - |
| finny (ok) | T | T | T | T | T | A | - |
| fsmentry (ok) | R | R | R | R | R | R | F-as-mut |
| statum (ok) | R | R | R | R | R | R | F-transition-with |
| state-machines (ok) | R | R | R | R | R | R | F-with-prepared-data |
| rust-fsm (ok) | T | T | T | T | T | A | F-from-state |

### Totals (36 probes per implementation)

| impl | R (compile time) | T (run time) | A (accepted) | - (n/a) | targets with a forge case | trybuild |
|---|---|---|---|---|---|---|
| implicit | 0 | 21 | 15 | 0 | 0 | ok |
| enum | 7 | 29 | 0 | 0 | 0 | ok |
| typestate | 32 | 4 | 0 | 0 | 0 | ok |
| statig | 7 | 29 | 0 | 0 | 0 | ok |
| smlang | 7 | 29 | 0 | 0 | 0 | ok |
| sml | 5 | 31 | 0 | 0 | 0 | ok |
| finny | 0 | 32 | 4 | 0 | 0 | ok |
| fsmentry | 23 | 10 | 3 | 0 | 5 | ok |
| statum | 32 | 4 | 0 | 0 | 6 | ok |
| state-machines | 26 | 10 | 0 | 0 | 5 | ok |
| rust-fsm | 0 | 32 | 4 | 0 | 6 | ok |

## Lines of code

`raw/loc.json` (`sme.py loc`: non-blank, non-comment, without `#[cfg(test)]`; `cargo fmt --all --check` passed, `raw/fmt-check.txt`).

| impl | pointer | wrap | polkit | transfer | lock | install | shared | total | total / enum |
|---|---|---|---|---|---|---|---|---|---|
| implicit | 220 | 179 | 245 | 282 | 248 | 241 | 7 | 1422 | 1.19 |
| enum | 208 | 190 | 176 | 189 | 211 | 215 | 6 | 1195 | 1.00 |
| typestate | 409 | 383 | 341 | 304 | 313 | 383 | 6 | 2139 | 1.79 |
| statig | 274 | 229 | 221 | 259 | 275 | 318 | 20 | 1596 | 1.34 |
| smlang | 313 | 266 | 328 | 387 | 367 | 299 | 6 | 1966 | 1.65 |
| sml | 299 | 319 | 256 | 304 | 244 | 292 | 15 | 1729 | 1.45 |
| finny | 336 | 330 | 408 | 462 | 344 | 397 | 34 | 2311 | 1.93 |
| fsmentry | 212 | 195 | 220 | 293 | 271 | 290 | 6 | 1487 | 1.24 |
| statum | 418 | 390 | 368 | 361 | 363 | 413 | 15 | 2328 | 1.95 |
| state-machines | 318 | 331 | 336 | 416 | 361 | 385 | 15 | 2162 | 1.81 |
| rust-fsm | 245 | 282 | 240 | 234 | 236 | 253 | 6 | 1496 | 1.25 |

## Sizes

`size_of` of each machine type in bytes (`raw/sizes.tsv`, `bench --bin sizes`, release).

| impl | pointer | wrap | polkit | transfer | lock | install |
|---|---|---|---|---|---|---|
| implicit | 48 | 64 | 40 | 56 | 28 | 32 |
| enum | 28 | 32 | 28 | 40 | 28 | 24 |
| typestate | 32 | 64 | 28 | 48 | 32 | 24 |
| statig | 32 | 48 | 28 | 48 | 24 | 32 |
| smlang | 32 | 48 | 28 | 48 | 32 | 48 |
| sml | 80 | 48 | 72 | 48 | 36 | 40 |
| finny | 216 | 128 | 224 | 112 | 464 | 352 |
| fsmentry | 28 | 32 | 28 | 40 | 36 | 24 |
| statum | 32 | 64 | 28 | 48 | 24 | 24 |
| state-machines | 168 | 176 | 184 | 296 | 256 | 336 |
| rust-fsm | 28 | 32 | 24 | 40 | 20 | 32 |
| (spec Event) | 24 | 6 | 8 | 16 | 8 | 32 |
| (spec Effect) | 16 | 6 | 8 | 12 | 8 | 24 |

## Compile time

`raw/compile.jsonl` (`sme.py compile`, 3 repetitions per profile, interleaved dev/release, under the
benchmark lock, `CARGO_BUILD_JOBS=8`, unpinned; dev builds without debuginfo, as the wrapper sets).
clean: the crate and all its dependencies from an empty target dir; crate: `cargo clean -p` the crate,
then build it; incr: touch `src/lib.rs`, build (`CARGO_INCREMENTAL=1`). Median of 3, seconds,
with [min-max].

### dev

| impl | n | crate only | incremental | clean (with deps) |
|---|---|---|---|---|
| implicit | 3 | 0.24 [0.24-0.25] | 0.06 [0.05-0.06] | 0.29 [0.28-0.29] |
| enum | 3 | 0.25 [0.25-0.26] | 0.06 [0.06-0.06] | 0.29 [0.29-0.30] |
| typestate | 3 | 0.30 [0.30-0.31] | 0.07 [0.06-0.07] | 0.34 [0.34-0.35] |
| statig | 3 | 0.36 [0.36-0.38] | 0.21 [0.21-0.21] | 2.92 [2.85-2.93] |
| smlang | 3 | 0.52 [0.51-0.52] | 0.28 [0.27-0.28] | 2.67 [2.62-2.79] |
| sml | 3 | 0.68 [0.66-0.79] | 0.37 [0.35-0.43] | 4.36 [4.26-4.53] |
| finny | 3 | 0.94 [0.89-0.98] | 0.24 [0.23-0.42] | 4.03 [3.80-4.12] |
| fsmentry | 3 | 0.37 [0.37-0.38] | 0.11 [0.11-0.11] | 2.19 [2.09-2.21] |
| statum | 3 | 1.01 [1.01-1.01] | 0.52 [0.51-0.62] | 3.87 [3.83-3.92] |
| state-machines | 3 | 1.11 [1.10-1.15] | 0.29 [0.28-0.31] | 3.70 [3.60-3.71] |
| rust-fsm | 3 | 0.31 [0.30-0.31] | 0.07 [0.07-0.08] | 1.65 [1.61-1.67] |

### release

| impl | n | crate only | incremental | clean (with deps) |
|---|---|---|---|---|
| implicit | 3 | 0.29 [0.29-0.29] | 0.05 [0.05-0.06] | 0.33 [0.33-0.34] |
| enum | 3 | 0.30 [0.28-0.30] | 0.06 [0.06-0.06] | 0.34 [0.32-0.35] |
| typestate | 3 | 0.34 [0.33-0.34] | 0.07 [0.07-0.07] | 0.39 [0.36-0.40] |
| statig | 3 | 0.45 [0.45-0.47] | 0.29 [0.29-0.29] | 2.96 [2.90-3.00] |
| smlang | 3 | 0.57 [0.57-0.58] | 0.30 [0.30-0.30] | 2.66 [2.64-2.74] |
| sml | 3 | 0.76 [0.75-0.79] | 0.38 [0.38-0.40] | 4.39 [4.36-4.43] |
| finny | 3 | 1.03 [1.03-1.05] | 0.23 [0.22-0.24] | 4.03 [4.00-4.13] |
| fsmentry | 3 | 0.42 [0.41-0.42] | 0.11 [0.10-0.11] | 2.14 [2.12-2.18] |
| statum | 3 | 1.06 [1.06-1.07] | 0.48 [0.48-0.48] | 3.91 [3.83-3.96] |
| state-machines | 3 | 1.56 [1.52-1.63] | 0.29 [0.27-0.31] | 4.08 [4.00-4.09] |
| rust-fsm | 3 | 0.34 [0.32-0.34] | 0.08 [0.07-0.08] | 1.71 [1.62-1.72] |

## Instruction counts, the primary number

Instructions (callgrind `Ir`, gungraun 0.20) of one run of each workload, from `raw/instructions.txt`,
minus the harness's fixed overhead, then divided by the workload's size: per event, and per printed
cell (a `Put` of the reference) for wrap-80/200. Workloads: 20 000 pointer events; 201 227 and 208 057
printed cells (208 510 and 213 282 events) at 80 and 200 columns; one replay set of every trace
(`raw/counts.tsv`, `raw/smoke-small.txt`). The overhead is what the crate slots report in a build with
every crate slot off (`raw/instructions-stub.txt`, all eight identical): pointer-stream 190, wrap-80 190, wrap-200 190, replay-pointer 1397, replay-wrap 984, replay-polkit 1316, replay-transfer 1415, replay-lock 1342, replay-install 1086.

### Instructions per event (per printed cell for wrap-80/200)

| impl | pointer-stream /ev | wrap-80 /cell | wrap-200 /cell | replay-pointer /ev | replay-wrap /ev | replay-polkit /ev | replay-transfer /ev | replay-lock /ev | replay-install /ev |
|---|---|---|---|---|---|---|---|---|---|
| implicit (bugs kept) | 107.5 | 80.6 | 79.6 | 150.1 | 95.4 | 99.3 | 170.3 | 89.7 | 89.6 |
| enum | 122.3 | 64.7 | 64.0 | 152.2 | 78.9 | 107.4 | 157.2 | 69.9 | 75.6 |
| typestate | 132.4 | 140.0 | 139.2 | 167.6 | 150.0 | 125.7 | 173.5 | 104.6 | 89.5 |
| statig | 153.0 | 127.2 | 125.2 | 244.1 | 156.9 | 330.6 | 217.8 | 282.3 | 195.3 |
| smlang | 160.0 | 115.8 | 116.2 | 194.4 | 123.8 | 112.7 | 200.3 | 117.7 | 157.7 |
| sml | 138.2 | 159.3 | 157.5 | 217.0 | 149.1 | 164.1 | 216.9 | 206.8 | 148.6 |
| finny | 245.8 | 179.9 | 177.9 | 338.0 | 190.7 | 287.3 | 271.3 | 327.3 | 219.6 |
| fsmentry | 151.5 | 66.0 | 65.1 | 192.7 | 81.0 | 132.0 | 190.3 | 91.7 | 114.1 |
| statum | 180.0 | 155.0 | 152.7 | 211.9 | 173.1 | 168.4 | 238.7 | 131.1 | 89.2 |
| state-machines | 283.2 | 355.1 | 350.3 | 348.4 | 344.8 | 301.8 | 493.8 | 332.8 | 303.2 |
| rust-fsm | 152.8 | 126.4 | 124.5 | 180.2 | 146.1 | 139.8 | 194.8 | 118.0 | 126.5 |

### Ratio to enum

| impl | pointer-stream | wrap-80 | wrap-200 | replay-pointer | replay-wrap | replay-polkit | replay-transfer | replay-lock | replay-install |
|---|---|---|---|---|---|---|---|---|---|
| implicit (bugs kept) | 0.88 | 1.24 | 1.24 | 0.99 | 1.21 | 0.92 | 1.08 | 1.28 | 1.19 |
| enum | 1.00 | 1.00 | 1.00 | 1.00 | 1.00 | 1.00 | 1.00 | 1.00 | 1.00 |
| typestate | 1.08 | 2.16 | 2.17 | 1.10 | 1.90 | 1.17 | 1.10 | 1.50 | 1.18 |
| statig | 1.25 | 1.96 | 1.96 | 1.60 | 1.99 | 3.08 | 1.39 | 4.04 | 2.58 |
| smlang | 1.31 | 1.79 | 1.82 | 1.28 | 1.57 | 1.05 | 1.27 | 1.68 | 2.09 |
| sml | 1.13 | 2.46 | 2.46 | 1.43 | 1.89 | 1.53 | 1.38 | 2.96 | 1.97 |
| finny | 2.01 | 2.78 | 2.78 | 2.22 | 2.42 | 2.67 | 1.73 | 4.68 | 2.90 |
| fsmentry | 1.24 | 1.02 | 1.02 | 1.27 | 1.03 | 1.23 | 1.21 | 1.31 | 1.51 |
| statum | 1.47 | 2.39 | 2.39 | 1.39 | 2.19 | 1.57 | 1.52 | 1.88 | 1.18 |
| state-machines | 2.32 | 5.49 | 5.47 | 2.29 | 4.37 | 2.81 | 3.14 | 4.76 | 4.01 |
| rust-fsm | 1.25 | 1.95 | 1.94 | 1.18 | 1.85 | 1.30 | 1.24 | 1.69 | 1.67 |

### Build sensitivity

The same benchmarks compiled into differently composed bench crates: `all` is the measurement
above (every slot); `solo` has the three baselines plus one crate slot (`raw/instructions-solo-<slot>.txt`);
`stub` has the baselines only. Each cell: ratio to enum in the `all` build / ratio to enum in that
slot's `solo` build (enum measured in the same build).

| slot | pointer-stream | wrap-80 | wrap-200 | replay-pointer | replay-wrap | replay-polkit | replay-transfer | replay-lock | replay-install |
|---|---|---|---|---|---|---|---|---|---|
| statig | 1.25 / 1.62 | 1.96 / 1.82 | 1.96 / 1.81 | 1.60 / 1.86 | 1.99 / 1.91 | 3.08 / 3.86 | 1.39 / 1.38 | 4.04 / 4.07 | 2.58 / 2.59 |
| smlang | 1.31 / 1.31 | 1.79 / 1.76 | 1.82 / 1.78 | 1.28 / 1.28 | 1.57 / 1.55 | 1.05 / 1.05 | 1.27 / 1.27 | 1.68 / 1.48 | 2.09 / 2.09 |
| sml | 1.13 / 1.13 | 2.46 / 2.46 | 2.46 / 2.46 | 1.43 / 1.43 | 1.89 / 1.89 | 1.53 / 1.53 | 1.38 / 1.38 | 2.96 / 2.96 | 1.97 / 1.97 |
| finny | 2.01 / 2.01 | 2.78 / 2.78 | 2.78 / 2.78 | 2.22 / 2.22 | 2.42 / 2.42 | 2.67 / 2.67 | 1.73 / 1.76 | 4.68 / 4.68 | 2.90 / 3.12 |
| fsmentry | 1.24 / 1.51 | 1.02 / 1.02 | 1.02 / 1.02 | 1.27 / 1.35 | 1.03 / 1.03 | 1.23 / 1.23 | 1.21 / 1.33 | 1.31 / 1.31 | 1.51 / 1.51 |
| statum | 1.47 / 1.47 | 2.39 / 2.39 | 2.39 / 2.38 | 1.39 / 1.39 | 2.19 / 2.17 | 1.57 / 1.57 | 1.52 / 1.54 | 1.88 / 2.73 | 1.18 / 1.18 |
| state-machines | 2.32 / 2.80 | 5.49 / 5.49 | 5.47 / 5.47 | 2.29 / 2.50 | 4.37 / 4.37 | 2.81 / 2.67 | 3.14 / 3.14 | 4.76 / 5.71 | 4.01 / 4.01 |
| rust-fsm | 1.25 / 1.47 | 1.95 / 1.95 | 1.94 / 1.94 | 1.18 / 1.32 | 1.85 / 1.85 | 1.30 / 1.18 | 1.24 / 1.24 | 1.69 / 2.03 | 1.67 / 1.67 |

A slot's own count, solo build against the all build (solo / all - 1):

| slot | pointer-stream | wrap-80 | wrap-200 | replay-pointer | replay-wrap | replay-polkit | replay-transfer | replay-lock | replay-install |
|---|---|---|---|---|---|---|---|---|---|
| statig | +0% | -7% | -7% | +1% | -4% | +4% | +0% | +1% | +0% |
| smlang | +0% | -2% | -2% | +0% | -1% | +0% | +0% | -12% | +0% |
| sml | +0% | +0% | +0% | +0% | +0% | +0% | +0% | +0% | +0% |
| finny | +0% | +0% | +0% | +0% | +0% | +0% | +2% | +0% | +8% |
| fsmentry | -6% | +0% | +0% | -7% | +0% | +0% | +0% | +0% | +0% |
| statum | +0% | -0% | -0% | +0% | -1% | +0% | -8% | +21% | +0% |
| state-machines | -6% | +0% | +0% | -5% | +0% | -5% | -0% | +0% | +0% |
| rust-fsm | -9% | +0% | +0% | -3% | +0% | -9% | +0% | +0% | +0% |

The baselines' counts per unit across all builds (all, stub and every solo build): min-max.

| baseline | pointer-stream | wrap-80 | wrap-200 | replay-pointer | replay-wrap | replay-polkit | replay-transfer | replay-lock | replay-install |
|---|---|---|---|---|---|---|---|---|---|
| implicit | 79.2-107.5 | 61.9-80.6 | 61.2-79.6 | 140.3-150.1 | 77.1-95.4 | 99.3-99.3 | 149.4-170.5 | 89.7-96.0 | 89.6-89.6 |
| enum | 94.7-122.3 | 64.7-64.7 | 64.0-64.0 | 132.2-152.3 | 78.9-79.0 | 88.7-107.4 | 143.4-157.6 | 58.2-69.9 | 75.6-75.6 |
| typestate | 101.1-132.4 | 140.0-140.0 | 139.1-139.2 | 139.9-167.6 | 149.9-150.0 | 96.4-125.7 | 155.9-173.7 | 104.6-104.6 | 89.5-89.5 |

### With Hotaru's release settings (thin LTO, one codegen unit)

The same all-slot benchmark built with `CARGO_PROFILE_BENCH_LTO=thin CARGO_PROFILE_BENCH_CODEGEN_UNITS=1`,
the settings of Sekka's `apps/hotaru/Cargo.toml` (`raw/instructions-lto-all.txt`; overhead from
`raw/instructions-lto-stub.txt`: pointer-stream 173, wrap-80 173, wrap-200 173, replay-pointer 1200, replay-wrap 850, replay-polkit 1180, replay-transfer 1247, replay-lock 1190, replay-install 950). Per unit, then the ratio to enum.

| impl | pointer-stream | wrap-80 | wrap-200 | replay-pointer | replay-wrap | replay-polkit | replay-transfer | replay-lock | replay-install |
|---|---|---|---|---|---|---|---|---|---|
| implicit (bugs kept) | 75.6 (0.88) | 61.9 (0.96) | 61.2 (0.96) | 127.1 (1.03) | 76.5 (0.98) | 71.0 (0.86) | 146.1 (1.04) | 89.7 (1.54) | 63.8 (1.21) |
| enum | 85.6 (1.00) | 64.7 (1.00) | 64.0 (1.00) | 123.7 (1.00) | 78.3 (1.00) | 82.1 (1.00) | 140.5 (1.00) | 58.2 (1.00) | 52.8 (1.00) |
| typestate | 101.1 (1.18) | 135.8 (2.10) | 135.0 (2.11) | 139.9 (1.13) | 144.3 (1.84) | 97.1 (1.18) | 153.2 (1.09) | 65.3 (1.12) | 51.6 (0.98) |
| statig | 120.0 (1.40) | 118.0 (1.82) | 116.1 (1.81) | 207.9 (1.68) | 150.1 (1.92) | 316.1 (3.85) | 201.4 (1.43) | 222.9 (3.83) | 163.5 (3.10) |
| smlang | 148.9 (1.74) | 113.8 (1.76) | 114.0 (1.78) | 162.3 (1.31) | 121.7 (1.55) | 107.2 (1.31) | 164.1 (1.17) | 77.6 (1.33) | 98.1 (1.86) |
| sml | 117.3 (1.37) | 63.3 (0.98) | 62.8 (0.98) | 192.9 (1.56) | 77.0 (0.98) | 133.1 (1.62) | 164.1 (1.17) | 134.3 (2.31) | 75.6 (1.43) |
| finny | 189.5 (2.21) | 168.3 (2.60) | 166.4 (2.60) | 277.2 (2.24) | 176.7 (2.26) | 222.8 (2.71) | 202.9 (1.44) | 267.3 (4.59) | 184.0 (3.48) |
| fsmentry | 111.9 (1.31) | 64.7 (1.00) | 64.0 (1.00) | 147.1 (1.19) | 77.6 (0.99) | 93.4 (1.14) | 152.8 (1.09) | 63.4 (1.09) | 59.1 (1.12) |
| statum | 123.5 (1.44) | 154.8 (2.39) | 152.6 (2.38) | 168.0 (1.36) | 169.4 (2.16) | 108.2 (1.32) | 180.5 (1.28) | 114.1 (1.96) | 50.7 (0.96) |
| state-machines | 255.0 (2.98) | 357.5 (5.52) | 352.9 (5.51) | 313.0 (2.53) | 341.8 (4.37) | 269.6 (3.28) | 439.4 (3.13) | 297.8 (5.12) | 272.5 (5.16) |
| rust-fsm | 139.0 (1.62) | 126.1 (1.95) | 124.3 (1.94) | 174.0 (1.41) | 142.1 (1.81) | 132.5 (1.61) | 156.2 (1.11) | 100.6 (1.73) | 102.2 (1.93) |

## Wall-clock (criterion), the secondary number

Median time per iteration from `raw/criterion-run<N>.jsonl` (each `<group>/<impl>/new/estimates.json`; criterion's
bootstrap 95% confidence interval of the median), divided by the workload's events (printed cells for
wrap-80/200: 1M pointer events; wrap streams of ~10M cells; one replay set of every trace). Pinned to
CPU 3 under the benchmark lock; loads in `raw/criterion-run<N>-load.log`. Ratios are to enum within
the same run. `!` marks a result whose two runs differ by more than 10%.

### Summary: median ns per unit (run 1), and the ratio to enum in run 1 / run 2

| impl | pointer-stream | wrap-80 | wrap-200 | replay-pointer | replay-wrap | replay-polkit | replay-transfer | replay-lock | replay-install |
|---|---|---|---|---|---|---|---|---|---|
| implicit (bugs kept) | 4.61 (0.67 / 0.69) | 4.05 (1.01 / 0.95) | 4.06 (0.96 / 0.98) | 7.23 (0.56 / 0.87) | 4.62 (1.04 / 1.10) | 4.73 (0.84 / 0.86) | 7.58 (0.98 / 1.03) | 4.06 (1.25 / 1.37) | 4.68 (1.19 / 1.33) |
| enum | 6.85 (1.00 / 1.00) | 4.01 (1.00 / 1.00) | 4.21 (1.00 / 1.00) | 12.89 (1.00 / 1.00) | 4.42 (1.00 / 1.00) | 5.62 (1.00 / 1.00) | 7.73 (1.00 / 1.00) | 3.25 (1.00 / 1.00) | 3.94 (1.00 / 1.00) |
| typestate | 10.65 (1.55 / 1.54) | 11.17 (2.79 / 2.64) | 11.59 (2.75 / 2.67) | 14.55 (1.13 / 1.83) | 11.50 (2.60 / 2.74) | 8.98 (1.60 / 1.66) | 11.87 (1.53 / 1.77) | 3.88 (1.19 / 1.25) | 4.13 (1.05 / 1.01) |
| statig | 11.44 (1.67 / 1.66) | 7.95 (1.98 / 1.98) | 8.43 (2.00 / 1.88) | 15.13 (1.17 / 2.09) | 7.64 (1.73 / 2.59) | 17.36 (3.09 / 3.29) | 13.56 (1.75 / 2.55) | 12.93 (3.98 / 4.61) | 8.81 (2.23 / 2.79) |
| smlang | 8.17 (1.19 / 1.19) | 4.70 (1.17 / 1.12) | 5.40 (1.28 / 1.09) | 9.91 (0.77 / 1.54) | 4.92 (1.11 / 1.25) | 4.80 (0.85 / 0.89) | 8.50 (1.10 / 1.15) | 4.52 (1.39 / 1.54) | 6.43 (1.63 / 1.92) |
| sml | 7.79 (1.14 / 1.30) | 7.18 (1.79 / 1.76) | 8.16 (1.94 / 1.51) | 20.21 (1.57 / 2.17) | 6.14 (1.39 / 1.19) | 10.08 (1.79 / 1.85) | 10.07 (1.30 / 1.61) | 8.24 (2.53 / 2.04) | 6.55 (1.66 / 2.01) |
| finny | 12.02 (1.75 / 1.80) | 9.48 (2.37 / 2.29) | 11.28 (2.68 / 2.25) | 20.74 (1.61 / 2.92) | 9.08 (2.05 / 1.67) | 15.66 (2.79 / 2.92) | 12.67 (1.64 / 1.88) | 13.01 (4.00 / 2.68) | 11.44 (2.90 / 3.11) |
| fsmentry | 6.88 (1.00 / 1.02) | 4.28 (1.07 / 1.08) | 5.17 (1.23 / 0.95) | 14.13 (1.10 / 1.33) | 4.59 (1.04 / 0.87) | 5.88 (1.05 / 1.10) | 9.26 (1.20 / 1.10) | 4.26 (1.31 / 0.90) | 5.98 (1.52 / 1.62) |
| statum | 4.91 (0.72 / 0.72) | 11.33 (2.83 / 2.67) | 11.25 (2.67 / 2.47) | 6.54 (0.51 / 0.87) | 12.51 (2.83 / 2.35) | 10.21 (1.82 / 1.88) | 14.87 (1.92 / 1.36) | 6.52 (2.01 / 1.37) | 6.15 (1.56 / 1.20) |
| state-machines | 15.21 (2.22 / 2.22) | 25.60 (6.39 / 5.68) | 25.91 (6.15 / 5.92) | 34.91 (2.71 / 2.36) | 21.05 (4.76 / 4.07) | 15.80 (2.81 / 2.92) | 40.98 (5.30 / 5.24) | 14.50 (4.46 / 3.32) | 19.69 (4.99 / 4.21) |
| rust-fsm | 8.51 (1.24 / 1.25) | 7.92 (1.98 / 1.55) | 8.05 (1.91 / 1.69) | 8.39 (0.65 / 1.89) | 7.55 (1.71 / 1.37) | 6.37 (1.13 / 1.20) | 10.85 (1.40 / 1.44) | 4.86 (1.50 / 1.71) | 9.24 (2.34 / 1.72) |

### pointer-stream (ns/event; 1 000 000 events per iteration)

| impl | run 1 median [95% CI] | / enum | run 2 median [95% CI] | / enum | run 2 vs 1 |
|---|---|---|---|---|---|
| implicit | 4.610 [4.606, 4.615] | 0.67 | 4.774 [4.680, 5.380] | 0.69 | +3.6% |
| enum | 6.851 [6.794, 6.868] | 1.00 | 6.925 [6.912, 6.940] | 1.00 | +1.1% |
| typestate | 10.653 [10.649, 10.659] | 1.55 | 10.697 [10.682, 10.713] | 1.54 | +0.4% |
| statig | 11.444 [11.440, 11.446] | 1.67 | 11.483 [11.459, 11.507] | 1.66 | +0.3% |
| smlang | 8.170 [8.156, 8.193] | 1.19 | 8.266 [8.246, 8.292] | 1.19 | +1.2% |
| sml | 7.786 [7.775, 7.795] | 1.14 | 9.008 [8.155, 9.749] | 1.30 | +15.7% ! |
| finny | 12.016 [12.006, 12.045] | 1.75 | 12.469 [12.325, 12.531] | 1.80 | +3.8% |
| fsmentry | 6.877 [6.874, 6.882] | 1.00 | 7.094 [7.062, 7.125] | 1.02 | +3.2% |
| statum | 4.913 [4.912, 4.915] | 0.72 | 5.018 [5.010, 5.034] | 0.72 | +2.1% |
| state-machines | 15.210 [15.128, 15.339] | 2.22 | 15.401 [15.348, 15.442] | 2.22 | +1.3% |
| rust-fsm | 8.507 [8.492, 8.519] | 1.24 | 8.624 [8.613, 8.647] | 1.25 | +1.4% |

### wrap-80 (ns/cell; 10 001 175 cells per iteration)

| impl | run 1 median [95% CI] | / enum | run 2 median [95% CI] | / enum | run 2 vs 1 |
|---|---|---|---|---|---|
| implicit | 4.047 [4.035, 4.119] | 1.01 | 4.097 [4.066, 4.108] | 0.95 | +1.2% |
| enum | 4.008 [4.000, 4.023] | 1.00 | 4.317 [4.105, 4.718] | 1.00 | +7.7% |
| typestate | 11.173 [11.169, 11.185] | 2.79 | 11.414 [11.304, 11.597] | 2.64 | +2.2% |
| statig | 7.948 [7.946, 7.952] | 1.98 | 8.527 [8.195, 9.287] | 1.98 | +7.3% |
| smlang | 4.704 [4.614, 4.802] | 1.17 | 4.815 [4.691, 4.993] | 1.12 | +2.3% |
| sml | 7.180 [6.484, 7.562] | 1.79 | 7.584 [6.925, 8.218] | 1.76 | +5.6% |
| finny | 9.481 [9.431, 9.528] | 2.37 | 9.869 [9.610, 10.309] | 2.29 | +4.1% |
| fsmentry | 4.279 [4.145, 4.481] | 1.07 | 4.669 [4.461, 4.834] | 1.08 | +9.1% |
| statum | 11.335 [11.276, 11.591] | 2.83 | 11.522 [11.419, 11.729] | 2.67 | +1.7% |
| state-machines | 25.604 [25.016, 26.013] | 6.39 | 24.530 [24.197, 25.675] | 5.68 | -4.2% |
| rust-fsm | 7.923 [7.364, 8.685] | 1.98 | 6.670 [6.565, 7.301] | 1.55 | -15.8% ! |

### wrap-200 (ns/cell; 10 014 434 cells per iteration)

| impl | run 1 median [95% CI] | / enum | run 2 median [95% CI] | / enum | run 2 vs 1 |
|---|---|---|---|---|---|
| implicit | 4.058 [4.021, 4.096] | 0.96 | 4.117 [4.065, 4.315] | 0.98 | +1.4% |
| enum | 4.212 [4.127, 4.428] | 1.00 | 4.213 [4.104, 4.463] | 1.00 | +0.0% |
| typestate | 11.589 [11.506, 11.683] | 2.75 | 11.268 [11.260, 11.281] | 2.67 | -2.8% |
| statig | 8.435 [8.323, 8.534] | 2.00 | 7.927 [7.917, 7.954] | 1.88 | -6.0% |
| smlang | 5.397 [5.086, 5.788] | 1.28 | 4.605 [4.594, 4.617] | 1.09 | -14.7% ! |
| sml | 8.160 [7.758, 9.027] | 1.94 | 6.365 [6.345, 6.377] | 1.51 | -22.0% ! |
| finny | 11.280 [10.862, 11.437] | 2.68 | 9.481 [9.438, 9.494] | 2.25 | -16.0% ! |
| fsmentry | 5.171 [4.572, 5.533] | 1.23 | 3.988 [3.977, 4.009] | 0.95 | -22.9% ! |
| statum | 11.248 [11.130, 11.294] | 2.67 | 10.408 [10.402, 10.424] | 2.47 | -7.5% |
| state-machines | 25.905 [25.205, 26.088] | 6.15 | 24.960 [24.663, 25.455] | 5.92 | -3.6% |
| rust-fsm | 8.049 [7.064, 8.882] | 1.91 | 7.121 [6.777, 7.450] | 1.69 | -11.5% ! |

### replay-pointer (ns/event; 96 events per iteration)

| impl | run 1 median [95% CI] | / enum | run 2 median [95% CI] | / enum | run 2 vs 1 |
|---|---|---|---|---|---|
| implicit | 7.232 [6.705, 8.805] | 0.56 | 6.404 [6.375, 6.465] | 0.87 | -11.5% ! |
| enum | 12.890 [12.662, 13.069] | 1.00 | 7.355 [7.307, 7.387] | 1.00 | -42.9% ! |
| typestate | 14.553 [13.476, 16.254] | 1.13 | 13.445 [13.258, 14.028] | 1.83 | -7.6% |
| statig | 15.130 [15.054, 15.220] | 1.17 | 15.394 [15.242, 15.993] | 2.09 | +1.7% |
| smlang | 9.906 [9.778, 10.916] | 0.77 | 11.312 [10.655, 12.025] | 1.54 | +14.2% ! |
| sml | 20.214 [17.797, 22.407] | 1.57 | 15.931 [15.507, 16.639] | 2.17 | -21.2% ! |
| finny | 20.738 [19.005, 23.866] | 1.61 | 21.493 [20.528, 22.478] | 2.92 | +3.6% |
| fsmentry | 14.134 [13.537, 14.651] | 1.10 | 9.747 [9.657, 9.874] | 1.33 | -31.0% ! |
| statum | 6.542 [6.356, 6.574] | 0.51 | 6.397 [6.311, 6.450] | 0.87 | -2.2% |
| state-machines | 34.914 [32.014, 36.317] | 2.71 | 17.374 [17.211, 17.707] | 2.36 | -50.2% ! |
| rust-fsm | 8.386 [8.371, 8.402] | 0.65 | 13.906 [11.262, 17.507] | 1.89 | +65.8% ! |

### replay-wrap (ns/event; 144 events per iteration)

| impl | run 1 median [95% CI] | / enum | run 2 median [95% CI] | / enum | run 2 vs 1 |
|---|---|---|---|---|---|
| implicit | 4.619 [4.616, 4.622] | 1.04 | 5.900 [5.758, 5.911] | 1.10 | +27.7% ! |
| enum | 4.422 [4.348, 4.443] | 1.00 | 5.374 [5.278, 5.724] | 1.00 | +21.5% ! |
| typestate | 11.497 [11.495, 11.499] | 2.60 | 14.712 [14.097, 15.327] | 2.74 | +28.0% ! |
| statig | 7.641 [7.638, 7.665] | 1.73 | 13.908 [12.265, 14.228] | 2.59 | +82.0% ! |
| smlang | 4.920 [4.918, 4.924] | 1.11 | 6.694 [6.013, 7.535] | 1.25 | +36.0% ! |
| sml | 6.136 [6.131, 6.160] | 1.39 | 6.369 [6.343, 6.423] | 1.19 | +3.8% |
| finny | 9.083 [9.006, 9.126] | 2.05 | 8.991 [8.961, 9.008] | 1.67 | -1.0% |
| fsmentry | 4.593 [4.592, 4.594] | 1.04 | 4.660 [4.651, 4.669] | 0.87 | +1.5% |
| statum | 12.506 [12.503, 12.510] | 2.83 | 12.604 [12.587, 12.630] | 2.35 | +0.8% |
| state-machines | 21.047 [21.031, 21.107] | 4.76 | 21.849 [21.730, 22.003] | 4.07 | +3.8% |
| rust-fsm | 7.551 [7.309, 8.041] | 1.71 | 7.351 [7.332, 7.378] | 1.37 | -2.7% |

### replay-polkit (ns/event; 94 events per iteration)

| impl | run 1 median [95% CI] | / enum | run 2 median [95% CI] | / enum | run 2 vs 1 |
|---|---|---|---|---|---|
| implicit | 4.731 [4.701, 4.803] | 0.84 | 4.641 [4.631, 4.646] | 0.86 | -1.9% |
| enum | 5.617 [5.533, 5.677] | 1.00 | 5.422 [5.418, 5.429] | 1.00 | -3.5% |
| typestate | 8.982 [8.951, 8.988] | 1.60 | 8.994 [8.985, 9.007] | 1.66 | +0.1% |
| statig | 17.365 [17.290, 17.421] | 3.09 | 17.832 [17.664, 18.017] | 3.29 | +2.7% |
| smlang | 4.796 [4.780, 4.821] | 0.85 | 4.834 [4.819, 4.869] | 0.89 | +0.8% |
| sml | 10.079 [10.033, 10.155] | 1.79 | 10.026 [9.979, 10.071] | 1.85 | -0.5% |
| finny | 15.662 [15.599, 15.735] | 2.79 | 15.840 [15.736, 15.881] | 2.92 | +1.1% |
| fsmentry | 5.876 [5.862, 5.897] | 1.05 | 5.948 [5.927, 5.973] | 1.10 | +1.2% |
| statum | 10.209 [10.188, 10.243] | 1.82 | 10.180 [10.148, 10.197] | 1.88 | -0.3% |
| state-machines | 15.805 [15.778, 15.830] | 2.81 | 15.825 [15.743, 15.976] | 2.92 | +0.1% |
| rust-fsm | 6.367 [6.348, 6.406] | 1.13 | 6.500 [6.482, 6.535] | 1.20 | +2.1% |

### replay-transfer (ns/event; 121 events per iteration)

| impl | run 1 median [95% CI] | / enum | run 2 median [95% CI] | / enum | run 2 vs 1 |
|---|---|---|---|---|---|
| implicit | 7.580 [7.550, 7.608] | 0.98 | 7.683 [7.562, 7.865] | 1.03 | +1.4% |
| enum | 7.734 [7.612, 7.857] | 1.00 | 7.469 [7.427, 7.593] | 1.00 | -3.4% |
| typestate | 11.870 [11.787, 11.957] | 1.53 | 13.225 [12.453, 14.177] | 1.77 | +11.4% ! |
| statig | 13.562 [13.179, 14.216] | 1.75 | 19.019 [16.100, 21.617] | 2.55 | +40.2% ! |
| smlang | 8.504 [8.386, 8.628] | 1.10 | 8.626 [8.515, 10.969] | 1.15 | +1.4% |
| sml | 10.073 [10.029, 10.116] | 1.30 | 11.998 [10.354, 14.243] | 1.61 | +19.1% ! |
| finny | 12.670 [12.609, 12.722] | 1.64 | 14.040 [13.534, 14.692] | 1.88 | +10.8% ! |
| fsmentry | 9.258 [9.193, 9.320] | 1.20 | 8.184 [8.152, 8.236] | 1.10 | -11.6% ! |
| statum | 14.870 [14.545, 14.922] | 1.92 | 10.185 [10.165, 10.211] | 1.36 | -31.5% ! |
| state-machines | 40.984 [40.057, 41.226] | 5.30 | 39.159 [38.450, 40.167] | 5.24 | -4.5% |
| rust-fsm | 10.846 [10.762, 10.900] | 1.40 | 10.786 [10.747, 10.796] | 1.44 | -0.6% |

### replay-lock (ns/event; 132 events per iteration)

| impl | run 1 median [95% CI] | / enum | run 2 median [95% CI] | / enum | run 2 vs 1 |
|---|---|---|---|---|---|
| implicit | 4.064 [4.048, 4.081] | 1.25 | 6.478 [6.432, 6.522] | 1.37 | +59.4% ! |
| enum | 3.251 [3.241, 3.260] | 1.00 | 4.733 [4.710, 4.744] | 1.00 | +45.6% ! |
| typestate | 3.877 [3.872, 3.885] | 1.19 | 5.934 [5.904, 5.951] | 1.25 | +53.1% ! |
| statig | 12.928 [12.891, 12.979] | 3.98 | 21.800 [21.648, 21.954] | 4.61 | +68.6% ! |
| smlang | 4.523 [4.506, 4.546] | 1.39 | 7.279 [7.265, 7.291] | 1.54 | +60.9% ! |
| sml | 8.241 [8.231, 8.264] | 2.53 | 9.653 [8.728, 10.316] | 2.04 | +17.1% ! |
| finny | 13.007 [12.967, 13.073] | 4.00 | 12.681 [12.631, 12.710] | 2.68 | -2.5% |
| fsmentry | 4.263 [4.246, 4.280] | 1.31 | 4.266 [4.246, 4.278] | 0.90 | +0.1% |
| statum | 6.523 [6.464, 6.567] | 2.01 | 6.474 [6.453, 6.508] | 1.37 | -0.7% |
| state-machines | 14.496 [14.467, 14.531] | 4.46 | 15.724 [14.806, 16.886] | 3.32 | +8.5% |
| rust-fsm | 4.864 [4.844, 4.880] | 1.50 | 8.089 [8.019, 8.159] | 1.71 | +66.3% ! |

### replay-install (ns/event; 122 events per iteration)

| impl | run 1 median [95% CI] | / enum | run 2 median [95% CI] | / enum | run 2 vs 1 |
|---|---|---|---|---|---|
| implicit | 4.682 [4.654, 4.720] | 1.19 | 7.315 [7.254, 7.360] | 1.33 | +56.2% ! |
| enum | 3.943 [3.923, 3.988] | 1.00 | 5.491 [5.476, 5.519] | 1.00 | +39.3% ! |
| typestate | 4.134 [4.120, 4.160] | 1.05 | 5.572 [5.543, 5.602] | 1.01 | +34.8% ! |
| statig | 8.807 [8.765, 8.875] | 2.23 | 15.344 [15.239, 15.494] | 2.79 | +74.2% ! |
| smlang | 6.434 [6.403, 6.477] | 1.63 | 10.539 [10.517, 10.596] | 1.92 | +63.8% ! |
| sml | 6.555 [6.536, 6.584] | 1.66 | 11.063 [10.997, 11.110] | 2.01 | +68.8% ! |
| finny | 11.445 [11.324, 11.533] | 2.90 | 17.098 [17.033, 17.153] | 3.11 | +49.4% ! |
| fsmentry | 5.985 [5.805, 6.869] | 1.52 | 8.878 [8.842, 9.084] | 1.62 | +48.3% ! |
| statum | 6.153 [4.773, 6.369] | 1.56 | 6.588 [6.542, 6.635] | 1.20 | +7.1% |
| state-machines | 19.693 [19.542, 19.824] | 4.99 | 23.120 [23.057, 23.259] | 4.21 | +17.4% ! |
| rust-fsm | 9.244 [7.375, 10.991] | 2.34 | 9.447 [9.421, 9.501] | 1.72 | +2.2% |

### With Hotaru's release settings (thin LTO, one codegen unit), one run

`raw/criterion-lto.jsonl` (built with `CARGO_PROFILE_BENCH_LTO=thin CARGO_PROFILE_BENCH_CODEGEN_UNITS=1`,
run once, pinned and locked like the others; loads in `raw/criterion-lto-load.log`). Median ns per
unit (ratio to enum in this run).

| impl | pointer-stream | wrap-80 | wrap-200 | replay-pointer | replay-wrap | replay-polkit | replay-transfer | replay-lock | replay-install |
|---|---|---|---|---|---|---|---|---|---|
| implicit (bugs kept) | 4.51 (0.86) | 4.08 (1.02) | 4.02 (1.02) | 5.94 (0.65) | 4.47 (0.74) | 3.32 (1.02) | 6.87 (0.67) | 3.56 (1.56) | 2.89 (1.21) |
| enum | 5.22 (1.00) | 4.02 (1.00) | 3.95 (1.00) | 9.14 (1.00) | 6.03 (1.00) | 3.27 (1.00) | 10.29 (1.00) | 2.27 (1.00) | 2.39 (1.00) |
| typestate | 6.19 (1.19) | 10.97 (2.73) | 9.46 (2.40) | 8.77 (0.96) | 13.83 (2.29) | 4.25 (1.30) | 10.06 (0.98) | 2.48 (1.09) | 2.71 (1.14) |
| statig | 11.21 (2.15) | 7.92 (1.97) | 7.80 (1.98) | 13.75 (1.50) | 8.09 (1.34) | 25.42 (7.78) | 12.74 (1.24) | 10.61 (4.67) | 7.86 (3.29) |
| smlang | 7.65 (1.47) | 4.45 (1.11) | 4.40 (1.11) | 8.61 (0.94) | 5.00 (0.83) | 5.50 (1.68) | 7.69 (0.75) | 4.50 (1.98) | 3.76 (1.57) |
| sml | 6.86 (1.32) | 4.15 (1.03) | 4.11 (1.04) | 13.52 (1.48) | 3.93 (0.65) | 6.79 (2.08) | 7.73 (0.75) | 4.69 (2.06) | 2.85 (1.19) |
| finny | 8.59 (1.65) | 9.23 (2.30) | 9.90 (2.51) | 14.36 (1.57) | 8.78 (1.46) | 17.50 (5.35) | 16.03 (1.56) | 12.35 (5.43) | 8.23 (3.44) |
| fsmentry | 5.40 (1.03) | 4.01 (1.00) | 3.99 (1.01) | 8.16 (0.89) | 4.35 (0.72) | 4.99 (1.53) | 6.79 (0.66) | 2.29 (1.01) | 2.53 (1.06) |
| statum | 4.87 (0.93) | 11.15 (2.77) | 10.41 (2.64) | 5.72 (0.63) | 12.15 (2.01) | 10.62 (3.25) | 7.25 (0.70) | 3.50 (1.54) | 2.44 (1.02) |
| state-machines | 13.79 (2.64) | 24.60 (6.12) | 23.64 (5.99) | 14.77 (1.62) | 21.85 (3.62) | 14.49 (4.43) | 37.31 (3.63) | 12.86 (5.66) | 17.66 (7.39) |
| rust-fsm | 6.81 (1.31) | 5.98 (1.49) | 5.81 (1.47) | 8.01 (0.88) | 6.80 (1.13) | 5.66 (1.73) | 7.26 (0.71) | 6.75 (2.97) | 4.76 (1.99) |

### Run-to-run differences above 10%

| group | impl | run 1 | run 2 |
|---|---|---|---|
| pointer-stream | sml | 7.786 | 9.008 |
| wrap-80 | rust-fsm | 7.923 | 6.670 |
| wrap-200 | smlang | 5.397 | 4.605 |
| wrap-200 | sml | 8.160 | 6.365 |
| wrap-200 | finny | 11.280 | 9.481 |
| wrap-200 | fsmentry | 5.171 | 3.988 |
| wrap-200 | rust-fsm | 8.049 | 7.121 |
| replay-pointer | implicit | 7.232 | 6.404 |
| replay-pointer | enum | 12.890 | 7.355 |
| replay-pointer | smlang | 9.906 | 11.312 |
| replay-pointer | sml | 20.214 | 15.931 |
| replay-pointer | fsmentry | 14.134 | 9.747 |
| replay-pointer | state-machines | 34.914 | 17.374 |
| replay-pointer | rust-fsm | 8.386 | 13.906 |
| replay-wrap | implicit | 4.619 | 5.900 |
| replay-wrap | enum | 4.422 | 5.374 |
| replay-wrap | typestate | 11.497 | 14.712 |
| replay-wrap | statig | 7.641 | 13.908 |
| replay-wrap | smlang | 4.920 | 6.694 |
| replay-transfer | typestate | 11.870 | 13.225 |
| replay-transfer | statig | 13.562 | 19.019 |
| replay-transfer | sml | 10.073 | 11.998 |
| replay-transfer | finny | 12.670 | 14.040 |
| replay-transfer | fsmentry | 9.258 | 8.184 |
| replay-transfer | statum | 14.870 | 10.185 |
| replay-lock | implicit | 4.064 | 6.478 |
| replay-lock | enum | 3.251 | 4.733 |
| replay-lock | typestate | 3.877 | 5.934 |
| replay-lock | statig | 12.928 | 21.800 |
| replay-lock | smlang | 4.523 | 7.279 |
| replay-lock | sml | 8.241 | 9.653 |
| replay-lock | rust-fsm | 4.864 | 8.089 |
| replay-install | implicit | 4.682 | 7.315 |
| replay-install | enum | 3.943 | 5.491 |
| replay-install | typestate | 4.134 | 5.572 |
| replay-install | statig | 8.807 | 15.344 |
| replay-install | smlang | 6.434 | 10.539 |
| replay-install | sml | 6.555 | 11.063 |
| replay-install | finny | 11.445 | 17.098 |
| replay-install | fsmentry | 5.985 | 8.878 |
| replay-install | state-machines | 19.693 | 23.120 |

## Cross-check: instructions in the timed binaries

criterion times a different binary (`benches/machines.rs`) from the one callgrind counts
(`benches/instructions.rs`), and the two inline differently. `raw/perf-check.tsv` counts
user-space instructions (`perf stat -e instructions:u`) of one run of each stream in the criterion
binaries (criterion's test mode, minus a run whose filter matches nothing in the group: the workload
generation). Per unit: callgrind in the instruction binary / perf in the default criterion binary /
perf in the LTO criterion binary; then instructions per ns in the default binary (perf count over the
run-1 median).

### pointer-stream (per event)

| impl | callgrind (instructions bench) | perf, default criterion binary | perf, LTO criterion binary | Ir per ns (default) |
|---|---|---|---|---|
| implicit (bugs kept) | 107.5 | 79.5 | 76.0 | 17.2 |
| enum | 122.3 | 123.4 | 86.3 | 18.0 |
| typestate | 132.4 | 133.5 | 101.7 | 12.5 |
| statig | 153.0 | 154.5 | 121.5 | 13.5 |
| smlang | 160.0 | 156.9 | 149.3 | 19.2 |
| sml | 138.2 | 139.6 | 118.7 | 17.9 |
| finny | 245.8 | 226.8 | 190.7 | 18.9 |
| fsmentry | 151.5 | 143.5 | 112.6 | 20.9 |
| statum | 180.0 | 132.8 | 124.1 | 27.0 |
| state-machines | 283.2 | 285.0 | 256.6 | 18.7 |
| rust-fsm | 152.8 | 153.4 | 139.6 | 18.0 |

### wrap-80 (per cell)

| impl | callgrind (instructions bench) | perf, default criterion binary | perf, LTO criterion binary | Ir per ns (default) |
|---|---|---|---|---|
| implicit (bugs kept) | 80.6 | 80.4 | 61.8 | 19.9 |
| enum | 64.7 | 64.6 | 64.6 | 16.1 |
| typestate | 140.0 | 139.0 | 134.8 | 12.4 |
| statig | 127.2 | 126.5 | 117.4 | 15.9 |
| smlang | 115.8 | 113.6 | 112.1 | 24.2 |
| sml | 159.3 | 160.1 | 63.2 | 22.3 |
| finny | 179.9 | 179.3 | 167.8 | 18.9 |
| fsmentry | 66.0 | 65.8 | 64.6 | 15.4 |
| statum | 155.0 | 155.5 | 155.3 | 13.7 |
| state-machines | 355.1 | 356.5 | 358.8 | 13.9 |
| rust-fsm | 126.4 | 125.8 | 125.6 | 15.9 |

### wrap-200 (per cell)

| impl | callgrind (instructions bench) | perf, default criterion binary | perf, LTO criterion binary | Ir per ns (default) |
|---|---|---|---|---|
| implicit (bugs kept) | 79.6 | 79.7 | 61.2 | 19.6 |
| enum | 64.0 | 64.1 | 64.1 | 15.2 |
| typestate | 139.2 | 139.4 | 135.2 | 12.0 |
| statig | 125.2 | 125.4 | 116.2 | 14.9 |
| smlang | 116.2 | 116.7 | 114.4 | 21.6 |
| sml | 157.5 | 157.3 | 62.8 | 19.3 |
| finny | 177.9 | 178.0 | 166.5 | 15.8 |
| fsmentry | 65.1 | 65.2 | 64.0 | 12.6 |
| statum | 152.7 | 152.7 | 152.6 | 13.6 |
| state-machines | 350.3 | 350.1 | 352.7 | 13.5 |
| rust-fsm | 124.5 | 124.6 | 124.4 | 15.5 |

## Load during the measurements

1-minute load average before and after each locked step, and the busy share of CPU 3 (the pinned
benchmark) and CPU 11 (its SMT sibling) between the two, from `/proc/stat` (`raw/*-load.log`; the
logs also hold the top 10 processes by CPU).

| step | start | end | load before | load after | CPU 3 busy | CPU 11 busy |
|---|---|---|---|---|---|---|
| compile dev 1 | 17:08:14 | 17:08:50 | 1.71 | 2.06 | 15% | 9% |
| compile release 1 | 17:08:50 | 17:09:27 | 2.06 | 1.96 | 13% | 11% |
| compile dev 2 | 17:09:27 | 17:10:02 | 1.96 | 1.88 | 11% | 7% |
| compile release 2 | 17:10:02 | 17:10:38 | 1.88 | 1.69 | 12% | 9% |
| compile dev 3 | 17:10:38 | 17:11:13 | 1.69 | 1.46 | 12% | 8% |
| compile release 3 | 17:11:13 | 17:11:50 | 1.46 | 2.03 | 16% | 12% |
| smoke | 17:11:53 | 17:11:57 | 2.03 | 1.95 | 100% | 3% |
| smoke-small | 17:11:57 | 17:11:57 | 1.95 | 1.95 | 86% | 13% |
| instructions | 17:12:12 | 17:12:31 | 3.43 | 2.74 | 7% | 3% |
| instructions-stub | 17:12:43 | 17:12:58 | 2.86 | 2.31 | 6% | 4% |
| instructions solo statig | 17:18:34 | 17:19:05 | 5.65 | 7.37 | 44% | 71% |
| instructions solo smlang | 17:22:18 | 17:22:43 | 8.75 | 8.32 | 41% | 78% |
| instructions solo sml | 17:24:23 | 17:24:55 | 7.01 | 7.31 | 35% | 69% |
| instructions lto all | 17:24:55 | 17:25:57 | 7.31 | 6.28 | 34% | 43% |
| instructions solo finny | 17:27:44 | 17:28:15 | 3.34 | 3.71 | 31% | 28% |
| instructions lto stub | 17:28:15 | 17:29:04 | 3.71 | 4.35 | 27% | 70% |
| instructions solo fsmentry | 17:30:45 | 17:31:09 | 3.61 | 5.30 | 35% | 66% |
| instructions solo statum | 17:32:57 | 17:33:26 | 7.27 | 5.88 | 23% | 41% |
| instructions solo state-machines | 17:35:11 | 17:35:37 | 3.60 | 4.36 | 34% | 65% |
| instructions solo rust-fsm | 17:37:18 | 17:37:43 | 6.87 | 8.01 | 48% | 85% |
| run 1 pointer-stream | 17:13:04 | 17:14:54 | 2.84 | 1.44 | 100% | 1% |
| run 1 wrap-80 | 17:14:54 | 17:18:34 | 1.44 | 5.65 | 100% | 20% |
| run 1 wrap-200 | 17:19:05 | 17:22:18 | 7.37 | 8.75 | 100% | 54% |
| run 1 replay-pointer | 17:22:43 | 17:24:23 | 8.32 | 7.01 | 100% | 56% |
| run 1 replay-wrap | 17:25:57 | 17:27:44 | 6.28 | 3.34 | 100% | 6% |
| run 1 replay-polkit | 17:29:04 | 17:30:45 | 4.35 | 3.61 | 100% | 17% |
| run 1 replay-transfer | 17:31:09 | 17:32:57 | 5.30 | 7.27 | 100% | 33% |
| run 1 replay-lock | 17:33:26 | 17:35:11 | 5.88 | 3.60 | 100% | 8% |
| run 1 replay-install | 17:35:37 | 17:37:18 | 4.36 | 6.87 | 100% | 34% |
| run 2 pointer-stream | 17:40:18 | 17:42:10 | 2.03 | 4.23 | 100% | 15% |
| run 2 wrap-80 | 17:42:10 | 17:45:36 | 4.23 | 4.99 | 100% | 30% |
| run 2 wrap-200 | 17:45:36 | 17:49:10 | 4.99 | 6.17 | 100% | 18% |
| run 2 replay-pointer | 17:49:10 | 17:50:57 | 6.17 | 7.01 | 100% | 35% |
| run 2 replay-wrap | 17:50:57 | 17:52:40 | 7.01 | 5.04 | 100% | 35% |
| run 2 replay-polkit | 17:52:40 | 17:54:21 | 5.04 | 4.85 | 100% | 9% |
| run 2 replay-transfer | 17:54:21 | 17:56:03 | 4.85 | 5.94 | 100% | 39% |
| run 2 replay-lock | 17:56:03 | 17:57:52 | 5.94 | 13.13 | 100% | 61% |
| run 2 replay-install | 17:57:52 | 17:59:43 | 13.13 | 22.97 | 100% | 100% |
| criterion lto build | 17:59:56 | 18:00:34 | 25.91 | 30.67 | 100% | 100% |
| lto pointer-stream | 18:12:34 | 18:14:23 | 1.70 | 2.56 | 100% | 5% |
| lto wrap-80 | 18:14:23 | 18:18:08 | 2.56 | 3.06 | 100% | 3% |
| lto wrap-200 | 18:18:08 | 18:21:52 | 3.06 | 3.89 | 100% | 10% |
| lto replay-pointer | 18:21:52 | 18:23:34 | 3.89 | 5.78 | 100% | 32% |
| lto replay-wrap | 18:23:34 | 18:25:16 | 5.78 | 4.72 | 100% | 29% |
| lto replay-polkit | 18:25:16 | 18:27:00 | 4.72 | 9.98 | 100% | 48% |
| lto replay-transfer | 18:27:00 | 18:28:43 | 9.98 | 7.26 | 100% | 38% |
| lto replay-lock | 18:28:43 | 18:30:31 | 7.26 | 7.01 | 100% | 36% |
| lto replay-install | 18:30:31 | 18:32:16 | 7.01 | 4.89 | 100% | 26% |


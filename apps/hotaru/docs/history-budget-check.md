# History budget check in the feed loop

After the [stream parser fix](stream-parser.md), styled text was about 4 %
slower than at the clipboard checkpoint. Profiling it on `wx-workstation`
showed where that went, and found a larger cost that both builds shared: the
history budget check after every byte the parser handles.

That check's common case is now inline. Styled text is 21 % faster than the
stream parser build and 17 % faster than the clipboard checkpoint. The screens
are the same, and history is pruned at the same points as before.

## Where the stream parser's cost went

- **Extra instructions, not code layout.** On the styled corpus the stream
  parser build retired 5.6 % more user instructions than its parent, at the
  same instructions per cycle. Branch and instruction-cache misses were
  negligible in both.
- **Per escape sequence.** On 8 MiB inputs that each isolate one kind of work,
  the extra instructions were:
  - 11 % for `ESC[0m` repeated, and 13 % for `ESC[5;7H`;
  - 1 % for plain lines, and 2 % for `CR LF`.

  Plain text takes the ASCII fast path. Everything the byte-at-a-time parser
  handles got dearer.
- **Callgrind's exact counts** come from `x86-64-v3` builds, which show the
  same deltas. For each `ESC[0m` there were about 53 extra instructions:
  - **+29 in `csi()`:** the new parameter-count and explicit-zero checks, and
    the extra register moves they cause;
  - **+15 in `feed_inner`'s byte loop:** lines callgrind can't attribute,
    from the larger state machine inlined there;
  - **+7 in the parser:** mostly the colon check before dispatch.
- **Removing them didn't win the time back.** Calling `sgr()` first in `csi()`
  brought styled text back to its parent's instruction count with identical
  screens. Yet it ran 1–3 % slower than the stream parser build in three
  timing runs. A variant with an unchanged instruction count also moved by
  2–3 %. At this size, code placement swamps the difference, so neither
  variant was adopted.

## The budget check

`Terminal::feed_inner` calls `enforce_history_budget()` after every byte the
parser handles, and that checks both screens.

- **Before:** each check was an out-of-line call to `Screen::enforce_budget`.
  The call loaded the screen's budget signal and returned at once when nothing
  had changed, which is almost always: an escape sequence's bytes don't touch
  row storage.
- **The cost:** the two calls per byte took about 15 instructions, which came
  to 60 for each `ESC[0m` and 90 for each `ESC[5;7H`. They cost more time than
  that count suggests; see the tables below.
- **Now:** `Screen::enforce_budget` tests the signal's dirty flag inline. It
  calls the out-of-line `apply_budget_change` only when the flag is set.
- **Behaviour is unchanged.** The check runs at every point it did before, so
  history is pruned at the same moments with the same peak. `history_budget.rs`
  still checks that pruning doesn't depend on how input is split into chunks.

**Checking only in ground state was rejected.** That variant called the budget
check only once the parser was back in ground state:

- It was nearly as fast on escape-heavy input, but slower on ASCII and
  `CR LF`.
- It lets the budget peak build up across a sequence, so history could be
  pruned at different points.

### Feed times

Six alternating rounds of 20 feeds per build on CPU 8, at a load of 2–3, with
the rest of CPU 8's core complex idle. Each entry is the median of 120 feeds.
The "inline" binary is byte-identical to the candidate below.

| Pattern | Parent | Stream parser | Ground only | Inline |
| --- | ---: | ---: | ---: | ---: |
| Styled text | 27.62 ms | 29.02 ms | 23.58 ms | **23.05 ms** |
| Cursor/erase | 92.14 ms | 93.29 ms | 88.53 ms | **87.95 ms** |
| ASCII | 5.59 ms | 5.59 ms | 5.74 ms | **5.32 ms** |
| Unicode | 34.20 ms | 33.10 ms | 31.81 ms | **31.25 ms** |

### Instructions

Change from the parent, from `perf stat -e instructions:u` over ten feeds:

| Input | Stream parser | Ground only | Inline |
| --- | ---: | ---: | ---: |
| `ESC[0m` repeated | +11.2 % | +3.8 % | +5.5 % |
| `ESC[5;7H` repeated | +12.7 % | +2.6 % | +6.1 % |
| `CR LF` repeated | +1.9 % | +4.6 % | −1.1 % |
| Plain lines | +1.0 % | +2.4 % | −0.7 % |
| Styled text | +5.6 % | −1.1 % | +1.0 % |
| Cursor/erase | +2.8 % | +1.0 % | +1.4 % |
| ASCII | +0.9 % | +2.3 % | −0.6 % |
| Unicode | −2.9 % | −2.6 % | −3.5 % |

The calls cost more time than their instruction count suggests. The inline
check removes 4.3 % of styled text's instructions and 21 % of its time.

## Against Ghostty

**The reviewed corpus:** `tools/benchmark.py` against the pinned
libghostty-vt, at a load of 1.1–1.4. Screens matched before timing. The
reports are `upstream/benchmarks/history-budget-check-*.json`.

| Pattern | Ghostty | Hotaru | Hotaru ÷ Ghostty | Stream parser build |
| --- | ---: | ---: | ---: | ---: |
| ASCII | 6.36 ms | 5.37 ms | 0.844 | 0.840 |
| Styled text | 32.53 ms | 22.88 ms | **0.703** | 0.908 |
| Unicode | 30.60 ms | 31.18 ms | 1.019 | 1.023 |
| Cursor/erase | 757.7 ms | 87.69 ms | 0.116 | 0.125 |

**Ghostty's own suite inputs.** `suite-feed.py` regenerates each 64 MiB input
from the [2026-10-30 comparison](ghostty-comparison-2026-10-30.md) and checks
that its screen matches the library's. It then times only the feed, through
both engines' in-process adapters with zero history: four rounds of five
feeds, alternating, on CPU 8.

- All seven inputs matched, so random UTF-8 and random OSC are timed here for
  the first time.
- The ASCII and OSC inputs are fresh, because their generators take
  time-based seeds.
- Load rose to about 25 as the last input finished. Each input's fastest and
  median feeds still agree within a few percent; Ghostty's OSC runs had one
  slow outlier.

| Input | Ghostty | Hotaru | Hotaru ÷ Ghostty | `2f2f73e0` |
| --- | ---: | ---: | ---: | ---: |
| ASCII stream | 26.35 ms | 18.57 ms | 0.705 | 0.75 |
| Styled: plain lines | 60.35 ms | 67.86 ms | 1.124 | 1.17 |
| Styled: heavy styling | 580.8 ms | 327.9 ms | 0.565 | 0.62 |
| Styled: Unicode-heavy | 479.4 ms | 573.0 ms | 1.195 | 1.23 |
| Styled: mixed, hyperlinks | 1,355.1 ms | 881.2 ms | 0.650 | 0.68 |
| Random UTF-8 | 434.5 ms | 629.0 ms | **1.448** | screens differed |
| Random OSC | 1,670.0 ms | 1,467.8 ms | 0.879 | screens differed |

Random UTF-8 is now the widest gap, ahead of Unicode-heavy lines and plain
random lines.

## Evidence

The candidate is `52ec3c4e…`
([`upstream/candidate-history-budget-check-build.json`](../upstream/candidate-history-budget-check-build.json)).
The records are in
[`upstream/history-budget-check/`](../upstream/history-budget-check/).

- **Oracle:** all 855 cases match the pinned library at chunk sizes of 1, 7
  and 65,536 bytes, 2,565 comparisons.
- **Ghostty's suite inputs:** all seven 64 MiB generated inputs leave the
  library's screen.
- **CSI sweep:** identical to the stream parser's, with the same four named
  differences.
- **Fuzzing:** 60,000 fresh cases (seeds 301–306) found one minimized
  difference. 88c434c9's build gives the same screen, so it predates this
  change:
  - With a wrap pending at the right margin, ED and EL with a parameter other
    than 0–2 clear the wrap in Hotaru but not in the reference. The parameters
    tried were ED 3, 4, 6, 9 and 22, EL 3, 6 and 9, and `?6J` and `?6K`.
  - The reference also implements ED 22.
  - The sweep missed both because its probes never start with a wrap pending.
  - Both are fixed in the next commit, with back tab (CBT), which a new
    pending-wrap sweep turned up; see [stream-parser.md](stream-parser.md).
- **Tests:** the Hotaru workspace's tests pass, including the history budget
  tests in release. rustfmt is clean, and Clippy is clean on `hotaru-core` and
  `hotaru-replay`.
  - The exception is `hotaru-io`'s
    `spawned_child_has_controlling_terminal_and_requested_size`, which fails
    inside the aizome dev shell. That shell exports `HOTARU_TERMINFO_DIR`, so
    the child sees `TERM=xterm-hotaru`. The test passes with that variable
    unset, and neither it nor `hotaru-io` changes here.
- **Packages:** the focused package
  `/nix/store/dyx8i0s3s8ms5nlgnp5igsxssdr0fk0m-hotaru-0.1.0` passed its check
  phase, which runs aizome-hotaru's 55 unit and 34 native tests. All eleven
  private-Sway suites passed against it. The Aizome package built with its
  check phase, as `/nix/store/hs0m49wb…-aizome-0.1.0`.

## Profiling on wx

- **Tools:** `perf` 7.2.10 from Nix, with `perf_event_paranoid` at 1.
- **Exact per-line counts come from callgrind.** On the Zen 5 CPU, sampled
  `instructions:u` lands almost every sample on one instruction. IBS op
  sampling (`ibs_op/swfilt=1,cnt_ctl=1/u`) is precise, but it aliases with
  fixed-length loops.
- **Callgrind needs AVX2 builds.** Valgrind can't decode AVX-512, so it ran on
  `-C target-cpu=x86-64-v3` builds. Their `perf stat` instruction deltas match
  the native builds'.
- **Timing needs a quiet machine; instruction counts don't.** Instruction
  counts are deterministic, so they hold under load from other work. Timing
  doesn't: an earlier run at load 36–99 gave bimodal results.

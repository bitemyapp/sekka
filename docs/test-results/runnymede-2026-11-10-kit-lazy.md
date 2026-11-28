# runnymede, 2026-11-10, 01:30: aizome-kit with lazy elements (Sekka PR 379)

The scenes of `runnymede-2026-11-09-kit.md` again, with the kit of [bitemyapp/sekka#379](https://github.com/bitemyapp/sekka/pull/379), which lets a view say that a subtree is as it was (`lazy`). Measured on the same desktop between 01:30 and 01:41. The kit is the pull request's head (`0da3562a`) with two commits on it that were not yet pushed (`ba18204c`: the focus ring's redraw past its clip, and an empty file removed); neither is on a path these scenes take.

Four kits and modes:

- **PR 373**: the kit of the evening before (`f89eb41d`), from `runnymede-2026-11-09-kit.md`. The decorated scenes and the 10,000-row scenes were run again with that same binary at 01:35 as a control.
- **PR 379, built outright**: the new kit with the adapter as it was: `view` builds every item again.
- **Every item lazy** (`BENCH_KIT_LAZY=items`): each item is a lazy element over a shared value that is replaced when its node or anything under it changes.
- **Lazy groups** (`BENCH_KIT_LAZY=groups`): the same, and the wide topology's run of siblings in lazy groups of a hundred. Only the wide scenes differ from `items`, so only those were run.

GPUI-CE's and zgui's numbers are the afternoon of 2026-11-09's, from `runnymede-2026-11-09.md`; neither was run again.

All times are milliseconds: the headline passes' median frame interval, then the diagnostic passes' median CPU work for a frame. A frame of 4.16 means the work fit in one of the display's (240 Hz). The kit draws once a display frame at most, so its frame interval stops at 4.16 however little it had to do; zgui's does not. Rows and the 1,024- and 64-node scenes are two repetitions, 8,192-node scenes one.

The adapter was built on another machine (wx-workstation) in this repository's dev shell, which is the same store path there and here, and copied over: nothing was compiled on the desktop.

## One row of a live column changes (`rows`)

| Rows, change | PR 373 | PR 379, built outright | every item lazy | lazy groups | GPUI-CE | zgui |
|---|---|---|---|---|---|---|
| 1,000, label | 4.15 / 0.55 | 4.17 / 0.57 | 4.16 / 0.14 | 4.16 / 0.06 | 4.16 / 2.44 | 0.16 / 0.06 |
| 1,000, fill | 4.16 / 0.53 | 4.16 / 0.54 | 4.16 / 0.10 | 4.16 / 0.03 | 4.16 / 2.42 | 0.13 / 0.01 |
| 1,000, size | 4.17 / 0.53 | 4.16 / 0.54 | 4.16 / 0.11 | 4.17 / 0.03 | 4.16 / 2.43 | 0.32 / 0.01 |
| 3,000, label | 4.16 / 1.55 | 4.16 / 1.51 | 4.17 / 0.29 | 4.17 / 0.07 | 8.38 / 6.77 | 0.24 / 0.12 |
| 3,000, fill | 4.16 / 1.51 | 4.16 / 1.49 | 4.16 / 0.26 | 4.17 / 0.03 | 8.40 / 6.75 | 0.23 / 0.01 |
| 3,000, size | 4.17 / 1.53 | 4.17 / 1.53 | 4.17 / 0.26 | 4.17 / 0.03 | 8.43 / 6.92 | 0.80 / 0.01 |
| 10,000, label | 4.96 / 4.87 | 4.96 / 4.90 | 4.16 / 0.83 | 4.16 / 0.08 | 29.6 / 23.9 | 0.49 / 0.33 |
| 10,000, fill | 4.91 / 4.92 | 4.99 / 4.87 | 4.16 / 0.80 | 4.16 / 0.05 | 29.8 / 23.7 | 0.60 / 0.01 |
| 10,000, size | 4.96 / 4.84 | 4.89 / 4.88 | 4.17 / 0.79 | 4.17 / 0.05 | 29.6 / 24.0 | 0.17 / 0.01 |

At 10,000 rows the 95th-percentile frame went from 8.19 (label, built outright) to 4.25 in both lazy modes: no frame of the display is missed any more. The control run of PR 373's binary at 01:35 gave 5.63 / 5.27, 5.83 / 5.02 and 5.75 / 4.84 for the three 10,000-row scenes, a little above the evening's.

## 1,024 and 64 nodes

| Scene | PR 373 | PR 379, built outright | every item lazy | lazy groups | GPUI-CE | zgui |
|---|---|---|---|---|---|---|
| 1,024 text, wide, 1% | 4.16 / 0.76 | 4.17 / 0.75 | 4.16 / 0.30 | 4.17 / 0.23 | 4.17 / 2.65 | 0.24 / 0.16 |
| 1,024 text, wide, 25% | 4.16 / 1.39 | 4.16 / 1.41 | 4.17 / 1.07 | 4.16 / 1.05 | 5.49 / 4.71 | 2.79 / 1.88 |
| 1,024 text, wide, 100% | 4.16 / 3.67 | 4.17 / 3.62 | 4.17 / 3.74 | 4.16 / 3.76 | 8.82 / 8.03 | 11.8 / 7.79 |
| 1,024 decorated, 100% † | 8.33 / 6.88 | 8.33 / 6.97 | 8.33 / 6.91 | | 12.4 / 11.6 | 8.33 / 2.90 |
| 64 decorated, 100% † | 8.33 / 5.02 | 8.33 / 5.15 | 8.33 / 5.05 | | 4.16 / 0.90 | 8.33 / 0.89 |

† PR 373's column is the 01:35 control here. The evening before, the same binary gave 6.59 / 6.84 and 5.27 / 4.90: the same work, and frames that came between one display frame and two. This morning every decorated scene's frames came two display frames apart, with PR 373's kit as with PR 379's, so that is the desktop this morning and not the kit.

## 8,192 nodes

| Scene | PR 373 | PR 379, built outright | every item lazy | lazy groups | GPUI-CE | zgui |
|---|---|---|---|---|---|---|
| Paint, flat, 1% | 4.16 / 3.88 | 4.16 / 3.92 | 4.16 / 0.69 | | 27.9 / 23.3 | 17.9 / 17.7 |
| Layout, balanced, 1% | 4.55 / 4.51 | 4.43 / 4.68 | 4.17 / 0.11 | | 179 / 174 | 1.21 / 0.92 |
| Text, wide, 1% | 4.45 / 4.49 | 4.42 / 4.46 | 4.17 / 1.00 | 4.17 / 0.40 | 25.1 / 20.0 | 1.23 / 0.98 |
| Paint, flat, 25% | 4.16 / 4.10 | 4.34 / 4.21 | 4.16 / 1.71 | | 27.8 / 23.4 | 23.0 / 21.6 |
| Layout, balanced, 25% | 4.62 / 4.67 | 4.56 / 4.57 | 4.16 / 1.58 | | 186 / 179 | 17.3 / 15.5 |
| Text, wide, 25% | 10.8 / 10.7 | 11.0 / 11.3 | 7.89 / 7.92 | 7.39 / 7.38 | 42.1 / 36.7 | 20.9 / 17.4 |
| Paint, flat, 100% | 5.34 / 6.07 | 5.53 / 6.48 | 5.64 / 6.48 | | 27.9 / 23.4 | 20.9 / 17.6 |
| Layout, balanced, 100% | 4.55 / 4.69 | 4.54 / 4.60 | 4.80 / 4.94 | | 177 / 172 | 68.6 / 63.1 |
| Text, wide, 100% | 29.2 / 29.7 | 30.6 / 31.3 | 28.5 / 32.7 | 28.7 / 29.2 | 72.8 / 66.9 | 83.8 / 71.0 |
| Structural, wide, 10% | 4.17 / 3.98 | 4.16 / 4.14 | 4.17 / 1.27 | 4.16 / 0.74 | 25.8 / 21.3 | 12.1 / 8.67 |
| Paint, decorated, 100% † | 8.33 / 11.6 | 8.33 / 11.6 | 8.33 / 11.8 | | 180 / 174 | 20.8 / 17.0 |
| Input latency, decorated, 1% | 4.41 / 4.40 | 4.39 / 4.30 | 4.17 / 0.14 | | 180 / 174 | 0.81 / 0.51 |
| Resize step, decorated | 7.80 / 11.7 | 8.39 / 11.0 | 8.34 / 7.01 | | 184 / 175 | did not resize |
| Resize storm, decorated | 8.93 / 10.6 | 9.24 / 11.0 | 7.70 / 10.0 | | 198 / 179 | 48.4 / 46.5 |

† The 01:35 control again; the evening's was 7.29 / 11.4.

What the tables say:

- **Unused, the new kit is the old one.** PR 379 built outright is within 7% of PR 373's work in every scene, above it in some and below in others.
- **Where little changes, the work follows the change.** One row of 10,000 takes 0.8 ms with every row lazy and under 0.1 ms with the rows in lazy hundreds, where building outright takes 4.9. At 8,192 nodes the 1% scenes take 0.11 to 1.0 ms where they took 3.9 to 4.5.
- **A long run of lazy siblings still costs a comparison each.** That is the 0.8 ms at 10,000 rows, and the 0.69 ms of the flat scene, whose 8,192 siblings wrap and can't be grouped. Groups take a wide scene's sparse updates down further: by a quarter at 1,024 nodes, and by 10 to 17 times at 10,000 rows.
- **Where everything changes there is nothing to keep**, and the lazy elements are so many more nodes: the balanced layout scene at 100% is 7% dearer (4.60 to 4.94), the others within the run's noise.
- **In a resize the kept items are laid out again and none is built**: the storm's work is 10.0 against 11.0.

## First frame and memory

| Scene | | PR 373 | PR 379, built outright | every item lazy | lazy groups |
|---|---|---|---|---|---|
| 10,000 rows | first frame, ms | 72 | 72 | 67 | 66 |
| 8,192 nodes, text, 1% | first frame, ms | 61 | 66 | 60 | 59 |
| 10,000 rows | peak PSS, MiB | 91 | 90 | 90 | 91 |
| 8,192 nodes, text, 100% | peak PSS, MiB | 105 | 105 | 82 | 85 |
| 8,192 nodes, structural | peak PSS, MiB | 79 | 87 | 94 | 97 |
| 8,192 nodes, resize step | peak PSS, MiB | 93 | 104 | 93 | |
| 8,192 nodes, resize storm | peak PSS, MiB | 286 | 164 | 787 | |

**The resize storm took four times the memory with lazy items**: 787 MiB at its peak in the headline pass and 773 in the diagnostic one, against 164 and 292 built outright. An earlier build of the adapter, whose values changed at every tick of a resize workload so that every item was built again each frame, peaked at 998 and 843. The same frames spent 1.95 ms presenting where the built-outright ones spent 0.34. The structural scene's memory rose by a tenth; no other scene's rose, and most fell. Both were found and fixed the same night: see below.

## The two memory findings, found and fixed (02:40)

**The resize storm was the window's buffers, not the tree.** Sidestep's render thread handled the main thread's messages for as long as there were any, and with lazy items the main thread is ahead of it, so there always were: the compositor's events went unread, among them the releases of the buffers each frame replaced. The storm gives the window a new size at every frame, each of which needs a buffer of that size, and none could reuse a released one; the shared-memory pool doubled until it held them all. In a headless sway the pool was 128 MiB with lazy items where it is 16 built outright. The render thread now reads the compositor's events between frames (Sekka branch `fix/render-thread-compositor-events`, on `main`, so the kit built outright gains too).

**The structural scene was dead nodes.** A tree that keeps its nodes kept the dead ones, widgets and all, until they outnumbered the living; the scene hides and shows a tenth of its rows on alternate ticks, so the dead piled up to about as many as the living between drops. A dead node now gives up its widget at once, and dead nodes at the end of the tree are dropped at once (Sekka PR 379, `58bcf54d`).

The same scenes again at 02:40 with both, peak PSS in MiB in the headline pass, and the frame and work in milliseconds:

| Scene | Mode | PSS before | PSS after | frame / work before | frame / work after |
|---|---|---|---|---|---|
| Resize storm, 8,192 decorated | built outright | 164 | 94 | 9.24 / 11.0 | 9.24 / 11.5 |
| Resize storm, 8,192 decorated | every item lazy | 787 | 93 | 7.70 / 10.0 | 6.28 / 8.74 |
| Structural, 8,192 wide, 10% | built outright | 87 | 87 | 4.16 / 4.14 | 4.17 / 4.17 |
| Structural, 8,192 wide, 10% | every item lazy | 94 | 70 | 4.17 / 1.27 | 4.16 / 1.10 |
| Structural, 8,192 wide, 10% | lazy groups | 97 | 72 | 4.16 / 0.74 | 4.16 / 0.43 |

The lazy storm's present went from 1.95 ms to 0.32 at the median. Every other scene of the tables above was run again with both fixes and came out within the run's noise of what it was, in time and in memory. The decorated scenes' frames came 5.4 to 7.4 ms apart again at 02:41, and so did PR 373's unchanged binary at 02:45 (6.58 and 5.22 for the 1,024- and 64-node scenes): the two display frames of 01:30 were the desktop then.

## What was checked, and what was not

- The three modes draw the same still scene: screenshots of a wide, a flat, a balanced and a decorated scene in a headless compositor are the same file in each mode.
- A scene as it changes was not compared across modes here. The kit's own tests do that for its lazy elements (`tests/lazy.rs`, 21 tests, each frame made of damage against the window drawn whole).
- GPUI-CE and zgui were not run again, so their columns are a day old and from another session of the desktop.

## How to run them

```
cargo build --profile bench-run -p bench-aizome-kit --features bench-aizome-kit/lazy
BENCH_KIT_LAZY=items cargo run -p gui-bench -- run --no-build --diagnostic --framework aizome-kit --repetitions 2 --preset rows
BENCH_KIT_LAZY=groups cargo run -p gui-bench -- run --no-build --diagnostic --framework aizome-kit --repetitions 1 --preset standard --scenario n8192-wide
```

with `../sekka` on a revision that has `lazy`. Unset, or `off`, the adapter builds every item outright. The raw documents (`runnymede-lz-*.json`, `runnymede-lz2-*.json`, `runnymede-ctl373-*.json`) are not in the repository.

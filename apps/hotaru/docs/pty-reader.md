# A reader thread for each pane's PTY

The [GPU comparison](gpu-comparison-2026-10-30.md) found Hotaru slower than
Ghostty at bulk output in the full application. On wx, Hotaru's core parses
ASCII at about 1,490 MiB/s, but the application consumed it at 432 MiB/s,
where Ghostty consumed 715 MiB/s. A reader thread for each pane closes most of
that gap.

## Where the time went

`perf` sampled the application on wx's NVIDIA GPU while 64 MiB of the ASCII
corpus went through its PTY.

- **One thread did both jobs.** The pane's thread (`hotaru-pty-N`) spent 30 %
  of all samples in the kernel reading the PTY and 20 % in Hotaru's own code
  parsing.
- **It wasn't idle.** `strace -c` counted 14,232 reads, about 4.7 KiB each,
  with almost no `EAGAIN` and only 108 polls. The thread wasn't waiting on
  wakeups; it read and parsed back to back.
- **Ghostty splits the work across threads.** It made as many reads (15,825),
  but on its own `io-gather` thread, almost all kernel time, while `io-reader`
  parses and `renderer` draws. Reading overlaps parsing.

A PTY hands over a few KiB per read, so reading costs about as much as parsing
plain text. Doing both in sequence on one thread left Hotaru's throughput at the
sum of the two.

## The change

Each pane now has a reader thread, `hotaru-read-N`, in
`desktop/aizome/apps/hotaru/src/session.rs`.

- **What it reads:** a duplicate of the PTY descriptor. It reads until the PTY
  has nothing more for now, or until 64 KiB has gathered.
- **What it hands over:** the chunk, through a channel bounded to four chunks,
  waking the pane's thread with its existing eventfd. Buffers return for reuse.
- **Backpressure:** while the pane's thread isn't taking output (its reply
  budget is full), the channel fills and the reader blocks. The PTY's buffer
  then fills, and the program writing to it waits, as before.
- **The end:** EOF (Linux's `EIO` once the last slave closes) and read errors
  follow the last output through the channel. The pane reports its program's
  exit only after that output has been parsed.
- **What stays on the pane's thread:** writes, replies, resizes, hangup
  detection and the terminal itself. The pane no longer polls the PTY for
  input.
- **Stopping:** when the pane's thread ends, a stop eventfd wakes the reader
  and it exits, closing its descriptor.

The test `bulk_output_arrives_whole_before_the_pane_ends` writes 100,000
numbered lines, far more than the reader's queue holds, and then an end marker.
It checks that the final frame shows the last lines in order, and then the
marker.

## Results

The A/B uses the packaged builds:

- main's Hotaru, with only the `cargo-output-hashes.nix` fix, as `a3rbsc14…`;
- this change, as `0xsfbcp0x…`.

Each ran three alternating rounds on wx's GPU compositor, with 64 MiB of each
corpus. Medians, with Ghostty's from the comparison:

| Corpus | main | Reader thread | Gain | Ghostty |
| --- | ---: | ---: | ---: | ---: |
| ASCII | 146 ms (439 MiB/s) | **80 ms (805 MiB/s)** | 1.83× | 91 ms (715 MiB/s) |
| Styled | 291 ms (220 MiB/s) | **214 ms (300 MiB/s)** | 1.36× | 301 ms (213 MiB/s) |
| Unicode | 478 ms (134 MiB/s) | 396 ms (162 MiB/s) | 1.21× | **351 ms (183 MiB/s)** |
| Cursor/erase | 652 ms (98 MiB/s) | **583 ms (110 MiB/s)** | 1.12× | 3,148 ms (20 MiB/s) |

On the laptop's Intel GPU, main, the reader thread and Ghostty rotated through
four rounds with the same corpora, while the desktop stayed in use (load average
2.6–4.0). Two of Ghostty's four runs timed out on the harness's startup wait for
the first marker frame, before any output was written, and are left out.
Medians:

| Corpus | main | Reader thread | Gain | Ghostty |
| --- | ---: | ---: | ---: | ---: |
| ASCII | 239 ms (268 MiB/s) | **197 ms (325 MiB/s)** | 1.21× | 224 ms (286 MiB/s) |
| Styled | 412 ms (155 MiB/s) | **247 ms (259 MiB/s)** | 1.67× | 264 ms (242 MiB/s) |
| Unicode | 543 ms (118 MiB/s) | 352 ms (182 MiB/s) | 1.54× | **331 ms (193 MiB/s)** |
| Cursor/erase | 803 ms (80 MiB/s) | **592 ms (108 MiB/s)** | 1.36× | 3,128 ms (20 MiB/s) |

The laptop's ASCII result depends on its load. In an earlier A/B of the two
Hotaru builds, with the load average at about 5.4, ASCII didn't change
(224 ms against 225 ms), though the other corpora gained as above. That run
doesn't show why, and the laptop's `perf` can't see kernel time.

**Typing latency** didn't change, measured with 400 presses per build on wx:

| | p50 | p95 | p99 | max |
| --- | ---: | ---: | ---: | ---: |
| main | 28.4 ms | 36.0 ms | 37.6 ms | 38.3 ms |
| Reader thread | 28.7 ms | 36.0 ms | 37.5 ms | 38.4 ms |

Every 30 and 60 Hz update stream delivered all 120 updates with both builds.

## What's left

After this change, Unicode text is still slower than in Ghostty, and that gap
is in the core, not the pipeline. Profiling the Unicode corpus on wx, the
pane's thread took 61 % of the application's samples, with only 0.3 % in the
kernel. Almost all of its time goes to printing in Unicode width mode
(DEC 2027), which panes use. As shares of all samples:

- `print_utf8`, 16 %, and `Handler::print`, 6 %;
- grapheme clustering: `Row::combine`, 6 %, and `CompactString::push`, 4 %;
- scalar UTF-8 validation of runs under 64 bytes: `from_utf8`, 6 %. The
  corpus's short lines hit this.

The [short-run validation experiment](short-utf8.md) was rejected for its cost
on malformed input. Mode 2027's print path is in next-work.

## The packages' git hashes

Building the focused package for this A/B showed that `packages.hotaru` had
stopped evaluating on main. Shiori added git dependencies (hayro, moxcms,
zune-jpeg) to the Aizome workspace's `Cargo.lock`. `package.nix` listed their
hashes, but `hotaru.nix` didn't. Both packages now import
`desktop/aizome/nix/cargo-output-hashes.nix`.

## Records

[`upstream/pty-reader/`](../upstream/pty-reader/) holds the records, from the
GPU-mode [frame benchmark](frame-benchmark.md):

- `wx/`:
  - the scripts;
  - the A/B and latency results (`ab.tar.gz`, `latency.tar.gz`);
  - each private-Sway suite's status;
  - the `strace -c` counts for main's Hotaru and Ghostty on ASCII;
  - `perf` thread and symbol summaries.
- `laptop/`: the rotation and the earlier, more loaded A/B, each with its
  script and results.

The private-Sway suites (selections, settings, smoke, fonts, search, shell,
links, automatic links, cursor, pixel scroll, sprites) pass with the reader
thread's package. So does its `cargo test` check phase, which includes the
bulk-output test. `cargo fmt` and clippy are clean for the change.

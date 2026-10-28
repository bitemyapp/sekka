# Private Wayland frame benchmark harness

`tools/frame-benchmark.py` is the first application-frame measurement harness
from the [renderer audit](renderer-audit.md). It is separate from the historical
DSR probes, which remain unchanged. The current evidence qualifies the tools
and matched application geometry, not either application's performance. The
functional calibration used the combined packaged candidate and exact reference
binary; unrelated Uta builds were active, so all its timestamps are excluded
from comparative conclusions.

The subsequent [sprite-package GUI pilot](gui-pilot-sprites.md) records the first
accepted matched timing window: four startup pairs and one sustained pair. Its
software-rendering scope, raw observations and remaining scrolling gap are
separate from this earlier harness/calibration qualification.

## What the timestamp means

A persistent C client uses `zwlr_screencopy_manager_v1` to copy the private
compositor's next output into one reusable SHM buffer. There is at most one
capture in flight. It records request time, receipt of `ready`, and completion
of marker decoding with `CLOCK_MONOTONIC`. The protocol's compositor timestamp
is retained as its original seconds/nanoseconds triple. Its arbitrary clock
offset is not assumed to match the observer's clock. No per-sample `grim`
process, DSR query, application-specific diagnostic hook or screenshot OCR is
used. One final `grim` capture is saved outside the measurements.

The reported latency ends at **observer receipt of a matching compositor
capture**, not physical scanout. A headless output has a synthetic refresh
clock. This harness does not obtain per-client presentation feedback or measure
keyboard-to-photon latency. Capture requests can themselves cause output work;
their effects must remain visible in the reports rather than being subtracted
as a guessed constant.

Each VT update starts with a top-row marker, performs the workload, then writes
a bottom-row marker. Each marker has six prefix guard cells, 32 sequence bits,
32 CRC bits and six suffix guard cells. CRC-32 covers the big-endian sequence
and run nonce. The observer validates every bit and both guard sequences,
checks five opaque interior samples per cell, validates the CRC, then requires
equal top/bottom serials. Wrong CRCs, torn marker cells, old/new row mixtures
and an incorrect run nonce fail decoding. Marker geometry is discovered once
from the pixels, then reused until explicitly reset after window calibration.

This validates a complete sequence marker, **not a checksum of every glyph in
the viewport**. Each exact VT write is separately hashed. The workload uses
common ASCII text, truecolor backgrounds and ordinary scrolling; unsupported
graphics or different Unicode rendering cannot be credited as faster work.
The paired markers change two rows on every update and have measurable cost.
A marker-only stream exposes that cost separately from sparse and scroll work.

## Independent events and controls

The raw PTY fixture records ready, input-read, write-start and write-end times
through an out-of-band socket. It precomputes timed stream/key buffers and
keeps terminal responses out of the measurement. The persistent observer also
owns a private virtual keyboard with a US keymap. Each keyboard trial sends an
actual Wayland `KEY_A` press/release and records transport flush completion.
An incomplete flush fails the latency trial rather than being treated as
already delivered input.

The report retains:

- Launch to fixture readiness and launch to the first valid captured marker.
  Startup resize repaints are explicit fixture events. Initial grid equality
  is checked separately from later calibration; mismatched startup grids do
  not form an accepted startup comparison.
- A modest default 32 paced keyboard trials with raw times, p50/p95/p99/max,
  input-to-fixture-read and final-write-to-capture intervals. This count tests
  the harness and is insufficient for strong tail-latency conclusions.
- Marker-only, sparse-cell and one-row-scroll streams at 30 Hz, 60 Hz and an
  unpaced producer. Unique observed serials, unobserved/coalesced serials,
  bytes, final-frame delay and CPU/memory samples remain separate. Observed
  serial throughput is a sampling lower bound, not a count of all app commits.
  A complete serial going backwards within a stream fails the run.
- Alternating static observer-off/on/off/on controls, including application,
  fixture, compositor and observer CPU ticks, RSS and capture/decode counts.
  The ordinary three-second control is exploratory; use longer controls when
  CPU counter resolution matters. A default one-millisecond request interval
  means the next eligible capture is requested promptly; one outstanding
  request and the actual compositor refresh still bound capture frequency.

App CPUs 8–11, compositor CPUs 12–13, observer CPU 14, controller CPU 15 and
fixture CPU 16 are disjoint by default. Overrides must be available and
disjoint. Sidestep raster workers and llvmpipe are explicitly configured for
four threads. These are process-local test settings; the harness changes no
host governor, driver, device permission or real desktop configuration.

## Geometry and environment qualification

Every run uses a temporary home/config/cache, a private D-Bus without service
activation and a private 1920×1440 Sway pixman output requested at 60 Hz. Mesa
llvmpipe is explicit for the GL reference. Both applications use the same
hashed IBM Plex Mono and packaged fallback directories, opaque backgrounds,
disabled cursor blink, hidden VT cursor and zero scrollback. Compiler/protocol
versions, actual executable hashes, configurations, CPU sets, environment
allowlist, output refresh information and original-resolution capture are saved.

First make a calibration run for each app. It adjusts the outer window until
the PTY reports the requested grid and records the independently observed
physical cell width/height. Then supply that run's `--window-width` and
`--window-height` for fresh startup trials. Require equal cell geometry with
`--cell-width` and `--cell-height`; a mismatch fails. Ghostty's supported
`--ghostty-cell-height` metric adjustment is available if needed, and its exact
value is preserved in the configuration. Different content dimensions may not
be silently normalized into an efficiency claim. Outer chrome dimensions are
reported independently, since the two native interfaces differ.

Both font-size settings are points: the native grid explicitly multiplies by
`96/72` before applying scale. At 14 points, the qualified native package uses
11×24-pixel cells; pinned Ghostty defaults to 11×25. The supported Ghostty
`adjust-cell-height=-1` gives the required 11×24 cells without changing its font
size. The initial fixture sets matching terminal default foreground/background
through OSC 10/11; application chrome retains its normal design.

Ghostty needs **`scrollback-limit-bytes=0`** to set its `Screen.no_scrollback`
flag. `scrollback-limit-lines=0` alone still retains history at page granularity,
as documented by the pinned source and its explicit zero-line tests. The new
harness sets both to zero. The old GUI probe's unlimited-byte/zero-line setting
is an additional reason its historical results cannot establish matched
zero-history parity; those reports and tools remain unchanged.

## Tool qualification

`tools/frame-harness-check.py OUTPUT` acquires the shared workstation lock,
builds the observer with `-Wall -Wextra -Werror`, runs synthetic tests against
the **actual C decoder**, exercises the fixture through a real PTY, then runs
an empty disposable software compositor. It launches neither terminal app.

The current [qualification report](../upstream/frame-harness/check-3/result.json)
passes 656 decoder cases across XRGB/ARGB/XBGR/ABGR, Y inversion, CRC/nonce
mismatch, torn cells, row mismatch and bounds; 30 PTY writes including real
key reads and startup resize repaint; eight controller cases requiring the
requested column count and agreement between fixture/decoded row counts; and
seven persistent captures across stop/restart plus virtual-keyboard transport.
Raw event logs and protocol, compiler, source and executable hashes accompany it.

The [application calibration report](../upstream/frame-harness/calibration/summary.json)
then passed the full controller, one actual virtual-keyboard trial and all nine
stream/control variants for both applications. It records exact configurations,
matching font-file hashes, executable identities and original 1920×1440 captures:

| Application | Package / source identity | Outer requested window | Content grid |
| --- | --- | --- | --- |
| Hotaru | `26jmmbqz0m6j903y1vm3c1d9si3b7j41-hotaru-0.1.0`, source `bsxc055gdzr4p5kfsp4fwsrll7s2cqqw-hotaru-source` | 1349×1077 | 120×40, 11×24 pixels/cell |
| Ghostty | `6ba144158577d033ccb6ed4b574e4013957c9161`, runtime ELF `ea356b4e5f33e0d0d55af99bb978afe419eed2a933edd31e457554db8970f61f` | 1334×964 | 120×40, 11×24 pixels/cell |

Both first captured frames also satisfy the requested grid. No malformed-config
or unknown-option warnings occurred. Ghostty's expected private-bus accessibility,
GTK color-change and CPU-presentation fallback warnings remain in its logs.
Rejected runs are retained: an unsupported runtime `for_window` command was
moved into the private Sway configuration; an initial Hotaru grid was unequal;
and unadjusted Ghostty cell height failed the physical-geometry gate. None is a
performance sample. `--purpose qualification` is the default; measurement mode
refuses to run when an unrelated Uta build process is detected.

The observer uses generated protocol bindings from installed, pinned Cargo
registry XML files, whose paths and hashes are recorded. The protocol's MIT
notice remains in its generated source. New observer code uses bounded
dimension arithmetic, at most 64 MiB of mapped capture memory, and one capture
owner. Screencopy `ready` completes the compositor write; unlike an attached
surface buffer, this protocol does not require a `wl_buffer.release` event
before reuse. Empty-compositor qualification explicitly exercises that reuse.

Example after a combined candidate is available (use a new output directory):

```sh
ssh wx-workstation python3 \
  /home/callen/work/hotaru-frame-harness-20261028/tools/frame-benchmark.py \
  hotaru /absolute/packaged/bin/hotaru \
  --font-dir /nix/store/b9vp4qd3gz9vmsl20grvd80hniirn5n5-aizome-fonts \
  --fallback-dir /nix/store/1rv68ixfzh8ki0jmanx7y846h2qdvzbg-hotaru-fonts \
  --window-width 1349 --window-height 1077 --cell-width 11 --cell-height 24 \
  --output /home/callen/work/hotaru-frame-harness-20261028/hotaru-calibration
```

Use `ghostty` and the recorded reference binary for its run, with matching Nix
Mesa supplied by `--mesa`, window 1334×964 and `--ghostty-cell-height=-1`.
The tool acquires the shared workstation lock itself;
do not wrap it in another acquisition of that same lock. The default workload
is a first qualification slice. At least 30 alternating fresh processes,
longer idle controls and 500 or more paced keyboard samples belong to a later
statistical comparison, after geometry and full-controller correctness pass.

## Alternating matrix controller

`tools/frame-matrix.py` runs calibrated Hotaru/Ghostty pairs in alternating
order. It snapshots its controller and observer sources, calibration receipts,
configuration and font identities into each output directory. Startup mode
uses fresh application processes; steady mode uses 500 keyboard samples,
120-frame streams and 30-second controls. `--rounds` selects the number of
pairs. No statistical comparison has been accepted merely because this
controller's qualification passes.

The outer controller owns the shared benchmark lock; children use a separate
private lock. Role-specific CPU sets are configurable and must be available
and disjoint. The activity monitor includes their SMT siblings. One-second
preflight observations reject substantial CPU or I/O activity and observed
compilers. Each measurement must have actual adjacent activity samples before
and after it, with gaps no greater than 2.5 seconds. A final synchronous sample
closes the interval after child cleanup. Recorded samples cannot prove
exclusive host use or detect every short-lived competing process.

Every trial checks both wrapper and runtime ELF identities, font hashes,
configuration and startup/final physical geometry against its calibration.
Failures and interference remain in the output. `--preflight-only` collects
the gate evidence without launching an application.

The Linux controller is a process-local child subreaper. On timeout it first
asks the inner harness to clean up. It then identifies remaining owned
descendants, opens pidfds, rechecks ancestry and terminates those children;
independent sessions cannot escape cleanup. Requiring this fallback rejects
the trial. It neither adopts nor signals unrelated host jobs. The
`--self-test` exercises activity arithmetic, missing/irregular interval
coverage and cleanup of a real orphan child in its own session. The
[qualification receipt](../upstream/frame-harness/matrix-check/result.json)
identifies the independently reviewed source.

Local Intel measurements use the separate
`~/.cache/hotaru-local-benchmark.lock`. CPU topology identifies P-cores 0–3
and E-cores 4–7; the current local GUI role assignment is application 3,
compositor 2, observer 4, controller 5 and fixture 6. The exact Nix application
closures can run in a task-owned rooted store and private mount namespace
when host-daemon import is unavailable. Record this environment separately
from workstation measurements; matching executable bytes do not make the
machines or driver stacks equivalent. The initial local preflight rejected
saturated CPUs before launching either application.

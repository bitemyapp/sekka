# Performance

The budget is 4 ms of CPU from an event to its frame, for a typical
interaction (see `docs/architecture.md` and the kit's guide). Each app
measures its own typical interactions here: headlessly through the kit's
`Harness` (update, view, layout, diff and rasterising the damage through
the offscreen renderer), and live with `SIDESTEP_FRAME_STATS=1` (Sidestep's
per-window summary every five seconds: record, rasterise, present). Numbers
are from this laptop, debug builds unless said (dependencies at
`opt-level = 2`, the workspace's own crates unoptimised), so they're upper
bounds. Exceptions to the budget are written up with their reasons.

## At a glance

Every part below was measured again on 18 October 2026 between 00:00 and
00:21, release builds (thin LTO) of commit `fd62197`, with Aizome's fonts
(`AIZOME_FONT_DIR`), on this laptop (Intel Core Ultra 7 365, 8 threads;
Mesa Intel graphics for GLES). Another agent's builds and tests shared the
machine: the one-minute load average was 4–9 during the harness runs and
the compositor's bench, 0.2–0.5 during the live Yukimi and Shashin runs
and 9–15 during the live shell runs; an earlier pass at load 21–25 was
thrown away (six of its budget tests failed).

Rows marked † were measured again on 18–19 October, after the
frame-budget work, the recovery overlay, screen recording and sharing,
live reload (the compositor is now a core and a reloadable logic, built
without LTO) and the compositor's polish: interleaved before-and-after
runs of the same binaries, at the load each note gives (other agents'
builds shared the machine, load 4–34). The split costs GLES 0.04–0.08 ms
a frame at the median, half of it from losing LTO; pixman is within
noise (`docs/reload.md`). Two kinds of number:

- **Harness**: each part's own test (the command is in its section below)
  times one frame from the event through update, view, layout and diff to
  rasterising the damage at 2× through the offscreen renderer. These are
  upper bounds: some apps rasterise the damage's bounding box rather than
  its rectangles, and a frame that covers the window also pays for the
  harness's offscreen buffer, about 1 ms (p95 up to 6.7 ms; the kit's
  `perf` test measures it), which a window on screen doesn't.
- **Live**: the compositor's CPU per output frame from
  `crates/aizome-comp/examples/frame_bench.rs` (headless, 2880 × 1800 at
  scale 2, GLES on the render node and pixman); and, where the table says
  so, Sidestep's frame statistics (`SIDESTEP_FRAME_STATS=1`: record,
  rasterise and present, summed per frame and summarised every five
  seconds) from a release app on Aizome's headless compositor at
  2880 × 1800, scale 2, driven with `aizomectl input`.

| Part | Typical interaction | p50 ms | p95 ms | 4 ms |
|---|---|---|---|---|
| Compositor, GLES | Moving a window | 0.46 | 0.61 | within |
| | A client drawing every frame (800 × 500) | 0.37 | 0.56 | within |
| | Switching Desks (slides) | 0.45 | 0.75 | within |
| | The Desks overview opening and closing † | 0.60–0.75 | 1.4–1.8 | within |
| | Ten windows, one moving | 0.76 | 0.95 | within |
| | A full-screen client drawing every frame | 0.44 | 0.60 | within |
| | A frosted menu (Full: blurred) over a moving window † | 0.70–0.85 | 0.93–1.6 | within |
| | A frosted menu over a client drawing every frame † | 0.44–0.67 | 0.59–0.96 | within |
| | The Ctrl+Alt+Del overlay opening, first time included † | 0.6 | | within |
| | Its list updating each second † | | ≤ 0.8 | within |
| | While a screencopy client records 60 fps † | 0.28 | 0.72 | within |
| Compositor, pixman | Moving a window | 0.45 | 0.69 | within |
| | A client drawing every frame (800 × 500) | 0.85 | 1.00 | within |
| | Switching Desks (slides) | 0.57 | 1.04 | within |
| | The Desks overview opening and closing † | 1.8–2.9 | 4.5–4.6 | **at** (1) |
| | The Ctrl+Alt+Del overlay opening † | 2.1 | | within |
| | Ten windows, one moving | 0.75 | 0.96 | within |
| | A full-screen client drawing every frame | 1.9–2.3 | 2.0–3.7 | within (2) |
| Shell | The clock's tick (bar and Deskbar) | 0.11 | 0.13 | within |
| | A CPU meter's bar moving | 0.05 | 0.06 | within |
| | A window opening or closing (the Deskbar and the bar) | 0.56 | 1.72 | within |
| | A key typed in the palette | 0.74 | 1.80 | within |
| | ↑ or ↓ in the palette | 0.23 | 0.29 | within |
| | The desktop's first frame (once per output) † | 3.1 (median; 0.8–9.1) | | **once** (4) |
| | The recording indicator's tick † | 0.085 | 0.1–0.2 | within |
| | The palette's first opening (live) † | 0.77 (median; 0.65–1.25) | | within (4) |
| Kit | Toggling a switch | 0.04 | 0.04 | within |
| | A character typed in a text field | 0.38 | 0.41 | within |
| | A character typed in a 2,000-line editor | 0.29 | 0.51 | within |
| | Scrolling a 10,000-row list a notch | 0.15 | 0.20 | within |
| | Moving a 10,000-row list's selection | 0.11 | 0.14 | within |
| Settei | Flipping a switch | 0.20 | 0.23 | within |
| | A key in the sidebar's search | 0.22 | 0.94 | within |
| | Another pane | 3.05 | 3.76 | within |
| | Network: the Wi-Fi switch | 1.88 | 3.67 | within |
| | Displays: dragging a display | 0.24 | 0.51 | within |
| | Sound: the input meter's tick | 0.12 | 0.12 | within |
| | Bluetooth: a device's options | 1.48 | 2.84 | within |
| Yukimi | Scrolling Installed | 3.20 | 3.57 | within |
| | Typing in Discover's search | 0.42 | 0.94 | within |
| | Opening a generation in History | 1.26 | 1.83 | within |
| | A frame of falling snow | 0.97 | 1.08 | within |
| | Switching pages (harness) † | 1.46–1.48 | 2.76–4.80 | **at** (6) |
| | Switching pages (live) † | 0.62–0.99 | 0.96–2.11 | within |
| Hako | ↓ in a column of 10,000 | 2.28 | 2.44 | within |
| | Scrolling that column | 0.40 | 0.42 | within |
| | ↓ in a list of 10,000 | 1.21 | 1.27 | within |
| Sumi | Typing in the board's document | 0.75 | 0.88 | within |
| | Scrolling it | 0.78 | 0.91 | within |
| | Typing at the end of a 10 MB file | 2.03 | (max 7.6) | within |
| | ❖↓ to the end of a 10 MB file † | 2.7 (median; 1.3–9.9) | | within at the median (7) |
| Fusen | Typing in a note | 0.08 | 0.10 | within |
| Koyomi | Next or previous month | 2.48 | 2.71 | within |
| | Next or previous week | 2.46 | 2.92 | within |
| | A step of dragging an event | 0.001 | 0.14 | within |
| | Dropping it | 0.30 | 0.53 | within |
| Tokei | The world clock's tick | 0.04 | 0.06 | within |
| | A stopwatch frame | 0.03 | 0.08 | within |
| Soroban | A key, Basic | 0.12 | 1.20 | within |
| | A key, Scientific | 0.31 | 0.69 | within |
| | A key, Programmer | 0.58 | 2.55 | within |
| Tenki | Choosing another place | 1.95 | 3.46 | within |
| | Hovering, dragging places, scrolling the hours, the minute's tick | 0.08–0.24 | 0.08–0.27 | within |
| Tansu | A click on the partition map | 1.30 | 1.53 | within |
| | ← → along the map | 0.62 | 0.65 | within |
| | Queueing an operation and undoing it | 1.44 | 2.85 | within |
| | Switching drives † | 1.32–2.71 | 1.46–3.03 | within |
| Kura | Moving over the rings | 0.92 | 1.41 | within |
| | Arrow keys in the folder's list | 1.30 | 2.38 | within |
| | Going into a folder and back up † | 1.06–1.39 | 1.77–2.58 | within |
| | Opening or closing a hint's paths † | 1.32–1.47 | 2.05–2.36 | within |
| | Ticking a path | 0.18 | 0.39 | within |
| | Picking a hint † | 1.33–1.36 | 1.71–1.88 | within |
| Myaku | A new reading (600 processes) | 2.61 | 2.99 | within |
| | A click on a row | 0.20 | 0.30 | within |
| | Scrolling the table | 0.29 | 0.34 | within |
| Shashin | Showing a fitted 3000 × 2000 photo | 3.30 | 3.68 | within (10) |
| | Panning at 100% | 3.25 | 3.56 | within (10) |
| | Pinching † | 2.81–2.84 | 3.22–3.65 | within (10) |
| | A zoom step, ❖+ ❖− (live, 6000 × 4000 JPEG) | 1.2–1.5 | 1.6–2.2 | within (10) |
| | The next picture † | 2.48–3.25 | 2.70–3.73 | within |
| Annai | A frame of a scene | 0.43 | 0.57 | within |
| | Turning a page | 1.00 | 1.09 | within |
| Greeter (login screen) | Typing a password | 0.06 | 0.07 | within |
| | Moving through the people | 0.11 | 0.36 | within |
| | A wrong password's shake | 0.10 | 0.23 | within |

Ranges are over several runs or five-second summaries. A live p95 over a
summary of six or seven frames is in effect its largest frame.

### Every exception

1. **The Desks overview under pixman** (compositor): opening and closing
   it, p50 1.8–2.9 ms, p95 4.5–4.6 ms, p99 4.7–5.9 ms in the final
   interleaved runs at load 6–11 (p95 2.9–3.5 ms at load 18–19 in
   `--only overview` runs; 5.8–7.6 ms inside the full bench at load
   4.6–14), against p95 14.9–19.5 ms before in the same runs. Its fade
   frames used to blend every piece over the whole output; see-through
   pieces on a flat colour are now filled as the colour they make, and
   thumbnails are kept at their size and scaled again only where their
   window drew. What's left are the opening and closing frames, which
   repaint the whole output as a full-screen client's frame does.
   *At the budget, accepted*: pixman is the fallback without a usable GPU,
   and GLES does the same frames in 0.60–0.75/1.4–1.8 ms.
2. **Full-output damage under pixman** (compositor): a full-screen client
   redrawing every frame, p50 1.9–2.3 ms, p95 2.0–3.7 ms. *Within*, but
   close: a full-screen video or game without a GPU sits near the budget.
3. **First frames of new labels** (compositor): a frame that first shows
   a title or caption rasterises it offscreen; the overview's cold first
   opening under GLES is the p99 tail (about 5 ms). *Accepted*: once per
   label, which is cached. The recovery overlay warms its own labels
   offscreen, so even its first opening is 0.6 ms (GLES), 2.1 ms (pixman).
4. **First frames of new surfaces** (shell). The memory of a screen's bar,
   Deskbar and desktop (21.6 MB at 2880 × 1800, 2×) is faulted in ahead
   (`Cx::reserve_buffers`), so their first frames no longer take thousands
   of page faults. Live, release, load 15–34, interleaved with the 17
   October build: the desktop's first frame median 3.1 ms (0.8–9.1; 13.6
   ms median before). *Accepted*: once per output. The palette's first
   opening, which rasterised its whole 1446 × 966-pixel surface (a median
   11.5 ms at load 15–34 on 18 October), is *fixed*: its window is kept
   between openings and drawn ahead at idle, offscreen in six bands of
   about a millisecond each (a second after the shell warms its text, a
   second after it closes, and a second after what it would open with
   changes; `Cx::prepare_window`), so opening it copies that picture in
   (the window keeps it while closed: 5.6 MB) and draws only what changed.
   On 19 October, release, Aizome's headless compositor at 2880 × 1800,
   scale 2, ❖Space five seconds after start, twelve runs each interleaved
   with main (`2e15129`) at load 0.1–2.9: the first opening median
   0.77 ms (0.65–1.25, of which the copy, rasterising, 0.62–1.20) against
   5.51 ms (5.13–8.34) before; opened again 0.83 ms (0.66–1.65) against
   4.45 ms (4.28–7.26). After `aizomectl reload --shell` 0.73 ms, after
   `reload --logic` at most 1.2 ms, and screenshots of the opened palette
   are the same pixels as before. In the harness (`tests/perf.rs`,
   `the_palettes_first_opening_after_warming`, ten runs each): the first
   opening median 2.40 ms (2.16–2.90, most of it the harness's own new
   buffer) against 3.78 ms (3.37–7.25), again 0.64 against 2.78 ms;
   drawing ahead 3.5–5.9 ms of the main thread at idle, a band a turn.
5. **The desktop's wallpaper frame** (shell): a 2880 × 1800 copy, 4.7 ms
   under load, once per output size and theme. *Accepted*.
6. **Switching pages in Yukimi**: harness p50 1.46–1.48 ms, p95 2.76–4.80
   ms at load 6 (4.78/5.95 before), now timed as a window draws its damage
   (the harness used to draw the damage's bounding box into a fresh image);
   live p50 0.62–0.99 ms, p95 0.96–2.11 ms at load 3–5 (3.8–5.0 ms before).
   Icons no longer hold a frame on one raster thread. *Within* live, *at*
   the budget in the harness under load.
7. **❖↓ to the end of a 10 MB file in Sumi**: median 2.7 ms over nine runs
   at load 29 (1.3–9.9 ms; 8.7 ms before). A jump no longer colours lines
   it can't reach; the rest of the colouring comes over the next frames
   (exact about 65 ms later), and a screen of new lines is laid out
   together on Sidestep's text threads. The tail is those threads not
   being scheduled on an oversubscribed machine. *Within* at the median.
8. **Switching drives in Tansu**: *fixed*, p50 1.32–2.71 ms, p95
   1.46–3.03 ms (4.45 ms before): the SD card's hatched free space is drawn
   once and copied after.
9. **Kura's rings**: *fixed*. Going into a folder and back up p95
   1.77–2.58 ms (4.56), opening a hint's paths p95 2.05–2.36 ms (5.50),
   picking a hint p50 1.33–1.36, p95 1.71–1.88 ms (5.09/11.4): the
   antialiased rings are drawn on a worker and copied after; a frame whose
   rings aren't ready draws them plainly, as moving rings already were.
10. **Shashin**: *fixed*. Pinching p50 2.81–2.84, p95 3.22–3.65 ms at load
    9 (4.25–4.44/5.95–6.83 at load 24: full-window frames are memory-bound);
    the next picture p95 2.70–3.73 ms (3.98 before; leaving a 3000 × 2000
    photo used to free its 24 MB of pixels on the window's thread). One
    frame of forty, the first pinch step below 100%, takes about 22 ms, not
    yet explained.

Within the budget now, though written up below as exceptions earlier:
Hako's preview column (↓ p95 2.44 ms), Tenki's choosing a place (p95
3.46 ms) and the shell's window opening and closing (p95 1.72 ms).

## The kit

`aizome-kit`'s own measures (the full tables, and how they're made, are
at the end of `docs/kit.md`), release builds at 2880 × 1800 (scale 2).
Sidestep repaints a whole window at 2× in about a millisecond now (5–7 ms
before), and the kit scrolls by moving pixels (Sidestep's
`Window::scroll`), drawing only the strips scrolled into view.

| Interaction | p50 | p95 | Where |
|---|---|---|---|
| Toggle a switch | 0.05 ms | 0.06 ms | harness |
| Type a character in a 2,000-line editor | 0.36 ms | 0.54 ms | harness |
| Scroll a 10,000-row list filling the window one notch | 0.15 ms | 0.21 ms | harness (pixels moved live, below) |
| A switch, live | 50–58 µs | 111–121 µs | headless sway |
| A list's and a table's selection, live | 205–253 µs | 394–466 µs | headless sway |
| The gallery's list and 10,000-row table scrolling two rows a frame, live | 1.5–2.1 ms | 1.8–3.0 ms | headless sway (drawn again: 2.5–3.6 and 2.9–4.8 ms) |
| The editor scrolling a line a frame, live | 1.0–2.0 ms | 2.0–3.6 ms | headless sway (drawn again: 4.4–5.2 and 8.5–13.6 ms) |

## Soroban (Calculator)

A key press, typed or clicked: the calculator's update, the view (up to 66
keys in Programmer mode), layout and diff, and rasterising what changed
(the display and the key that flashes, 10% of the window; with ↩ also the
tape).

| Mode | p50 | p95 | max | |
|---|---|---|---|---|
| Basic | 0.25 ms | 0.61 ms | 0.66 ms | `tests/soroban.rs`, `a_key_press_fits_the_frame_budget`, at 2×, 252 presses |
| Scientific | 0.55 ms | 0.98 ms | 1.04 ms | ″ |
| Programmer | 0.85 ms | 1.35 ms | 1.65 ms | ″ (the bit toggles redraw when the value changes) |

Live, under headless sway (pixman) at 1×, typing with `wtype`, Sidestep's
frame stats: total p50 0.43–0.48 ms, p95 0.62–0.94 ms, max 1.6 ms. The
average damage there (64%) is `wtype`'s doing: each run of it brings a new
virtual keyboard, so the window loses and regains the keyboard, and the
widget that holds the focus for typing spans the window. A real keyboard
redraws only the display and the key.

No exceptions.

## The compositor

The compositor's budget is **4 ms of CPU per output frame**: collecting
the scene's elements, rendering what's damaged and submitting it.

### Method

- The compositor times every frame it draws on every output, from the
  start of element collection to the end of submission (the damage
  tracker's render, or DrmCompositor's render and queue). Redraws that find
  nothing damaged are counted as skipped and draw nothing. Under headless
  GLES it also waits on the frame's fence and records that wait as the GPU
  time.
- The last 4096 frames per output are kept; `aizomectl stats` (IPC
  `stats`) prints p50, p95, p99, max and mean, frames rendered and frames
  skipped, and `--reset` starts over.
- `crates/aizome-comp/examples/frame_bench.rs` starts a headless
  compositor with one 2880 × 1800 output at scale 2 (the laptop panel),
  opens sidestep-ui windows (`examples/sidestep_client.rs`) and runs the
  scenarios below, resetting the statistics around each:

  ```sh
  nix develop --command cargo build --release -p aizome-comp --bin aizome --examples
  nix develop --command target/release/examples/frame_bench --renderer gles
  nix develop --command target/release/examples/frame_bench --renderer pixman
  ```

- Release build (thin LTO), Intel Core Ultra 7 365 (8 threads), Mesa
  Intel graphics (Panther Lake) through EGL on the render node; pixman on
  the same CPU. Effects tier Lean (hard shadows, 80 ms motion).

### Results (2026-10-18, 00:01, load 4–7)

Commit `fd62197`, the same bench on a quieter machine (another agent's
builds still running), milliseconds of compositor CPU per rendered frame:

| Scenario | GLES p50 | p95 | p99 | max | pixman p50 | p95 | p99 | max |
|---|---|---|---|---|---|---|---|---|
| Idle, 2 s | – | – | – | – | – | – | – | – |
| Moving a 320 × 200 window, 240 steps | 0.46 | 0.61 | 0.71 | 1.16 | 0.45 | 0.69 | 0.79 | 1.19 |
| A client drawing every frame (800 × 500) | 0.37 | 0.56 | 0.59 | 0.74 | 0.85 | 1.00 | 1.63 | 17.3 |
| Switching Desks (slides), 12 | 0.45 | 0.75 | 1.03 | 1.03 | 0.57 | 1.04 | 1.12 | 1.12 |
| Overview open and close, 8 | 1.34 | 2.73 | 5.87 | 5.87 | 4.84 | 19.1 | 21.4 | 21.4 |
| 10 windows, one moving | 0.76 | 0.95 | 0.99 | 1.00 | 0.75 | 0.96 | 1.01 | 1.05 |
| A full-screen client drawing every frame | 0.44 | 0.60 | 0.70 | 0.83 | 2.31 | 3.48 | 3.79 | 3.95 |

Two more pixman runs at load 0.2–0.3: the overview p50 5.2–6.9, p95
19.5–19.7, max 20–30 ms; the full-screen client p50 1.9, p95 2.0–3.7 ms.
GLES's GPU time (the fence) was p50 0.8–2.5 ms, p99 1.0–4.0 ms. The
overview under pixman is slower than in the evening run below; the
compositor built from that run's commit (`d4dcb61`) measured p95 21–29 ms
under pixman the same night, so it's the machine (or the Sidestep both
build against), not the overview's code.

### Results (2026-10-17, evening run)

Milliseconds of compositor CPU per rendered frame. Other agents were
building on the machine during the run (load average 6–12), so the tails
are pessimistic.

GLES:

| Scenario | Frames | p50 | p95 | p99 | max | GPU p50 / p99 |
|---|---|---|---|---|---|---|
| Idle, 2 s | 0 | – | – | – | – | – |
| Moving a 320 × 200 window, 240 steps | 220 | 0.34 | 0.67 | 0.98 | 1.44 | 0.68 / 0.93 |
| A client drawing every frame (800 × 500) | 166 | 0.39 | 0.60 | 0.73 | 0.75 | 0.97 / 1.34 |
| Switching Desks (slides), 12 | 26 | 0.36 | 0.84 | 1.23 | 1.23 | 0.63 / 0.82 |
| Overview open and close, 8 | 32 | 0.92 | 2.33 | 8.16 | 8.16 | 1.89 / 3.25 |
| 10 windows, one moving | 109 | 0.51 | 0.99 | 1.37 | 2.17 | 0.67 / 0.91 |
| A full-screen client drawing every frame | 153 | 0.38 | 0.58 | 0.81 | 1.13 | 2.37 / 3.05 |

pixman:

| Scenario | Frames | p50 | p95 | p99 | max |
|---|---|---|---|---|---|
| Idle, 2 s | 0 | – | – | – | – |
| Moving a 320 × 200 window, 240 steps | 226 | 0.54 | 0.84 | 1.26 | 1.29 |
| A client drawing every frame (800 × 500) | 165 | 1.59 | 1.97 | 2.22 | 2.35 |
| Switching Desks (slides), 12 | 26 | 0.36 | 0.89 | 0.98 | 0.98 |
| Overview open and close, 8 | 32 | 3.77 | 14.8 | 15.2 | 15.2 |
| 10 windows, one moving | 112 | 0.77 | 0.99 | 1.26 | 1.55 |
| A full-screen client drawing every frame | 155 | 2.53 | 3.55 | 3.79 | 4.02 |

Under GLES a client's wl_shm frame is now uploaded into its texture when
it commits (so the buffer can go straight back to the client), not while
the output renders: the frames above no longer include those uploads,
which is why a client drawing every frame costs 0.4 ms here against
0.8–1.8 ms in the morning run. The upload (a few hundred microseconds
for a full 800 × 500 frame) is still the compositor's work, in the
commit handler. Pixman reads client memory as it composites, so its
numbers include it as before.

(With Archivo and IBM Plex Mono installed through `AIZOME_FONT_DIR`, as in
a session. An earlier run without them, before fonts were loaded at
start-up, had a 16 ms first frame when a window first moved: loading the
font.)

What these show:

- **Idle means no frames**: nothing changes, nothing is drawn, and no
  frame callbacks fire.
- **Moving a window composites**: the window's buffers are reused and only
  the old and new rectangles are redrawn; the client is never asked to
  redraw. Tabs and frames are cached solid-colour elements with stable
  identities, and labels are cached textures, so they cost nothing until
  they change.
- Everything ordinary stays well inside the budget on both renderers,
  pixman included. (A Desk slide here is two or three drawn frames: the
  80 ms ease-out carries a window off the output within a few frames,
  after which nothing visible changes.)

### Exceptions

1. **Full-output damage under pixman at 2880 × 1800** (a full-screen
   client redrawing every frame: p50 2.5–3.5 ms, p95 3.6–4.4 ms, max 4–5.6 ms;
   an earlier run on a busier machine had p95 11.6 ms; on 18 October p50
   1.9–2.3 ms, p95 2.0–3.7 ms, inside the budget). Each frame composites
   5.2 million pixels on the CPU; the cost is memory traffic, not the
   scene. GLES does the same frame in 0.4 ms (1.8 ms in the morning run,
   when the client's upload was part of the frame). Full-screen video or games
   without a usable GPU sit at or over the budget; ordinary windows
   (partial damage) don't.
2. **The Desks overview under pixman** (p50 3.1–3.8 ms, p95 12.8–14.8 ms,
   max 15–18.5 ms; up to 57 ms in the earliest run; on 18 October p50
   4.8–6.9 ms, p95 19.1–19.7 ms, max 20–30 ms). Opening and closing it
   damages the whole output, and its thumbnails are window buffers scaled
   down at composition, which pixman filters on the CPU. It redraws only
   when opened, closed, filtered or dragged in, never continuously, so the
   cost is a handful of frames per use.
3. **First frames of new labels.** A frame that first shows a label (a new
   title, the overview's captions) rasterizes it with Sidestep's offscreen
   canvas and uploads it: the 8–13 ms overview maximum under GLES is its
   first opening, setting a dozen captions (5.9 ms on 18 October). Labels are cached per text,
   style and scale (and dropped half a minute after they were last
   drawn), so these are one-off costs; fonts load at start-up.
4. **GPU time** under headless GLES is the wait for the frame's fence,
   which includes queueing behind other GPU work; its p99 spikes don't
   hold the compositor's CPU.

On the udev backend frames are paced by vblank: a frame waits for the
previous one's scan-out rather than being dropped, and the CPU cost per
frame is the same as above (DrmCompositor also puts suitable buffers on
planes for direct scan-out, which only lowers it).

## Tokei (Clock)

A clock's tick is the typical interaction: once a second the program
reads the time, views the window again and diffs it, and only the digits
that changed redraw, because every character of a clock is its own
keyed element in tabular IBM Plex Mono (`widgets::digits`): 21:42:08 →
21:42:09 damages one 31 × 69pt cell, and a new minute the big clock's
last four cells and each city's minute (under 8% of the window; checked
by `a_tick_redraws_only_the_digits_that_changed`).

| Interaction | p50 | p95 | max | |
|---|---|---|---|---|
| World clock tick (4 cities) | 0.22 ms | 0.41 ms | 0.63 ms | `tests/tokei.rs`, `a_tick_fits_the_frame_budget`, at 2×, 117 ticks |
| Stopwatch frame (25 a second, while it runs and shows) | 0.16 ms | 0.28 ms | 0.32 ms | ″, 97 frames |

Live, under headless sway at 2× with `SIDESTEP_FRAME_STATS=1` (driven with
`wtype`: ❖4, Space, L): the stopwatch running, 133 frames in five seconds,
total p50 0.08 ms, p95 0.14 ms, max 1.8 ms, redrawing 3% of the window
per frame on average; the world clock idling, a frame a second, total p50
0.14 ms (the first frame, the whole window, is opening, not a tick).

The timer's ring is drawn in a thousand steps, so it redraws when a step
passes rather than on every tick. No exceptions.

## Yukimi (Software)

Headlessly, `apps/yukimi/tests/budget.rs` (`cargo test --release -p
aizome-yukimi --test budget -- --nocapture`), on the sample system, from
the event to the damage rasterised at 2× (the damage's bounding box, so
these are upper bounds). Release build, with Aizome's fonts:

| Interaction | p50 | p95 | max | drawn |
|---|---|---|---|---|
| Scrolling Installed with the wheel | 1.48 ms | 1.55 ms | 1.57 ms | 76% |
| Typing in Discover's search | 0.50 ms | 0.74 ms | 1.15 ms | 23% |
| Opening a generation in History | 1.36 ms | 1.90 ms | 2.17 ms | 61% |
| Hovering cards and rows | 0.00 ms | 0.01 ms | 0.01 ms | 0% (no view) |
| A frame of falling snow (Overview) | 0.94 ms | 1.04 ms | 1.12 ms | 19% |
| Switching pages | 4.92 ms | 5.82 ms | 7.38 ms | 97% |

Searching itself runs on a thread of its own (the window only shows the
answer), as does reading the system and the store, working out what a
generation changed, checking for updates and making the package index;
none of it is on the interface thread.

Live, the release build under headless sway (pixman) at 2× on this
system's real store (36,000 paths), Sidestep's frame stats: with the
Overview's snow falling, 184 frames in 5 s, total p50 0.93 ms, p95
2.4 ms; while switching pages every 1.2 s (through the instance socket),
p50 1.7–2.1 ms, p95 6.8–13 ms.

Again on 18 October (commit `fd62197`, load 4–9), after Sidestep's faster
repaint: the harness's page switch p50 4.78 ms, p95 5.95 ms, max 7.26 ms
(97% drawn, the harness's own full-window buffer included); scrolling
3.20/3.57, typing a search 0.42/0.94, a generation 1.26/1.83, snow
0.97/1.08 ms. Live, the release build with `--sample` on Aizome's headless
compositor (GLES) at 2880 × 1800, scale 2, ❖2 to ❖6 through `aizomectl
input` every 1.2 s for 30 s at load 0.3: total p50 1.7–2.4 ms, p95 3.8–5.0
ms per five-second summary (six or seven frames each, so the p95 is the
largest page), damage 55–64%, almost all of it rasterising.

Exception: **switching pages**. A new page repaints the whole content
area (and the toolbar and the sidebar's selection) at 2×. Yukimi's part,
the update, the view and the layout, is 0.15–0.65 ms of it (Discover's
catalog, 47 cards, is the most); the rest is rasterising about 3.4
million pixels of text and cards: 3.5–4.5 ms before Sidestep's faster
repaint, now 1.6–2.3 ms at the median live and 3.7–4.9 ms for the
largest pages.
Switching pages is a deliberate act a few times a minute, not something
repeated under the hand, so it is left at that rather than made to draw
less than the page that's new.

`YUKIMI_STALLS=50` prints, on standard error, every update or view that
takes longer than that many milliseconds (as the GTK Yukimi does for its
interface thread).

## Tenki (Weather)

Headlessly, `apps/tenki/tests/tenki.rs` (`cargo test --release -p
aizome-tenki --test tenki interactions -- --nocapture`), on the recorded
forecasts for Tokyo, Chicago and Reykjavík, from the event to the damage
rasterised at 2× (the damage's bounding box: upper bounds). Release build,
with Aizome's fonts, the machine busy with other builds (load 13–20):

| Interaction | p50 | p95 | max | drawn |
|---|---|---|---|---|
| Choosing another place | 2.22 ms | 3.86 ms | 9.9 ms | 91% |
| Hovering the places | 0.08 ms | 0.09 ms | 0.15 ms | 5% |
| Dragging a place along the list | 0.12 ms | 0.12 ms | 0.16 ms | 5% |
| Scrolling the hours sideways | 0.27 ms | 0.32 ms | 0.83 ms | 14% |
| The minute's tick ("Updated 13 min ago") | 0.10 ms | 0.13 ms | 0.19 ms | 0–1% |

Fetching, searching and asking GeoClue run on background threads, and
files (the cache, the places, the state) are written by a thread of their
own: replacing a file waited half a second on the busy disk when it was
done on the window's thread, which the first measurements caught.

Live, a debug build (the kit unoptimised) under Aizome's headless
compositor (GLES) at 2×, with `SIDESTEP_FRAME_STATS=1`, driven with
`aizomectl input` (❖N, typing "tokyo", Return, then ❖[ and ❖] every
0.3 s): switching places, total p50 4.2 ms and p95 6.5 ms per frame
(rasterising p50 3.0 ms; 52% of the window drawn on average over those
frames); otherwise p50 0.12–0.15 ms, p95 0.16–0.4 ms. The compositor's
own frames meanwhile: p50 0.5–1.1 ms, p95 1.2–1.9 ms.

Choosing a place is the one interaction that comes near the budget: it
replaces the whole main view (another place's every number), so it
redraws nine tenths of the window, and its worst frames are the first
time a place's strings are shaped. The release build keeps its p95 under
4 ms; the test holds it under 8 ms (as it runs beside other builds) and
everything else under 4 ms.

## Kura (Disk Usage)

### Scanning

The scanner (`apps/kura/src/scan.rs`) reads folders on a thread per CPU
(8 here) into one arena. On this laptop's home folder, 908,849 files and
113,202 folders (345 GB), with the cache warm, release build:

| | |
|---|---|
| The scan, 8 threads | 0.5 s |
| The scan, 1 thread | 1.9 s |
| `aizome-kura scan` in all: scan, hints (duplicates read and hashed), `index.json` and `index.md` written | 0.6–0.75 s |
| The arena after the scan | 67 MB for 981,366 nodes (48-byte nodes and the names), shrunk to fit |

The first `scan` on a Nix system also asks Nix for the store's size (`nix
path-info --all --json`, about 2.5 s here, in the background in the
window); it's cached against the store database's modification time.
Cold-cache numbers need root to drop the page cache, so there are none.

### The window

Headlessly, `apps/kura/tests/ui.rs`, `hovering_and_going_down_fit_the_frame_budget`
(`cargo test --release -p aizome-kura --test ui hovering_and_going -- --nocapture --test-threads 1`),
on a made-up home of 29,000 files (204 segments in the rings), from the
event to the damage rasterised at 2× (its bounding box, so upper bounds).
The machine was busy with other builds (load average 13–21 on 8 CPUs)
while these were taken:

| Interaction | p50 | p95 | max |
|---|---|---|---|
| Moving over the rings (a frame for each new segment under the pointer) | 0.96–1.16 ms | 1.5–2.2 ms | 2.3–8.7 ms |
| Arrow keys in the folder's list (the row and the ringed segment) | 1.7–3.2 ms | 3.0–7.8 ms | 3.3–9.5 ms |
| Going into a folder and back up (the click, then the rings' frames) | 2.5–4.2 ms | 6.7–11.7 ms | 10–12.7 ms |

The sunburst is the kit's `canvas`; what's hovered and selected are its
`parts`, so lighting a segment redraws the middle and that segment's
rectangles (one per eighth of a turn, hugging the arc) rather than the
rings. Segments are filled with half-point gaps between them instead of
stroked edges: stroking each segment was four fifths of a full drawing.
Painting a full set of rings (190 segments, a real home folder's) costs
about 4 ms at 2×, of which the antialiased fills are 3.5 ms; without
antialiasing 1.4–1.8 ms. (For comparison, the harness's offscreen
rendering of an empty area of the same size costs about 4 ms of its own,
so the headless numbers above are pessimistic.)

Live, the release build under headless sway (pixman) at 2× (2560 × 1600),
`SIDESTEP_FRAME_STATS=1`, on `~/work` (622,000 items), driven with
`wlrctl` (the pointer around the rings 160 times, into the largest folder
and out four times, a context menu), on the same busy machine: moving over
the rings, 128 frames, total p50 2.8 ms, p95 13 ms (9% of the window
redrawn on average); going in and out, total p50 3.2 ms, p95 11 ms; the
menu open, p50 1.7 ms.

Exceptions:

- **The rings moving** between levels (Aizome's motion time, 80 ms in
  Lean; none with reduced motion or Austere) redraw the whole sunburst on
  every frame. They're drawn without antialiasing (half the cost) and
  without labels; a frame is 2.5–3.5 ms at the median, more on a busy
  machine. It's five or six frames, a few times a minute.
- **Lighting the largest segments**: a segment spanning half a ring (a
  `target` folder that's half the disk) has rectangles covering much of
  the sunburst, and they're redrawn whole. That's the p95 above.
- **The first drawing after a scan**, and a resize: the whole window, as
  for any app.

### Suggestions

`suggestions_fit_the_frame_budget` in the same file, release build, the
made-up home with eight hints, the machine busier still (load average 24):

| Interaction | p50 | p95 | max |
|---|---|---|---|
| Opening or closing a hint's paths (the panel lays out again) | 1.8 ms | 6.8 ms | 9.2 ms |
| Ticking a path in or out | 0.15 ms | 0.30 ms | 0.42 ms |
| Picking a hint | 5.1 ms | 20 ms | 22 ms |

Exception: **picking a hint**. It moves the rings to the folder holding
the hint's paths and dims everything else, so the whole sunburst is drawn
again along with the panel (the rings alone are about 4 ms at 2×). It's a
deliberate click on a card, not something repeated; hovering and moving
through the list afterwards are the cheap paths above. Actions and the
rescans after them run on the kit's threads; the window only takes the
result.

Again on 18 October (release, commit `fd62197`, load 4–9, the same
tests): picking a hint p50 5.09 ms, p95 11.4 ms, max 15.5; opening or
closing a hint's paths 1.63/5.50 ms (the panel laying out again: over the
budget at the tail, a click on a card too); ticking a path 0.18/0.39 ms.
The rings: moving over them 0.92/1.41 ms, the list's arrows 1.30/2.38 ms,
going in and back up 1.62/4.56 ms (the rings moving, as above).

## Shell

The shell's surfaces (bar, Deskbar, desktop, palette, menus, panels, OSD,
lock) share one process and one render thread, so its budget is per
surface frame: 4 ms from the message to the frame. Each typical frame
redraws only what changed: the clock's tick repaints the clock's own
rectangle (2% of the bar), a meter only its bars, a key in the palette its
query and the rows that moved.

### Headless, through the kit's `Harness`

`crates/aizome-shell/tests/perf.rs` (`cargo test -p aizome-shell --release
--test perf -- --nocapture`): the message's update, every surface's view,
layout and diff, and rasterising the damage at 2× through the offscreen
renderer, over the boards' scene (five apps, three unread mails, three
Shelf items). Release build:

| Frame | p50 | p95 | max |
|---|---|---|---|
| Clock tick (bar and Deskbar) | 0.10 ms | 0.21 ms | 0.31 ms |
| A CPU meter bar moving | 0.05 ms | 0.07 ms | 0.11 ms |
| A window opening or closing (the Deskbar grows or shrinks, the bar's count) | 0.56 ms | 2.73 ms | 5.07 ms |
| A key typed in the palette (re-ranking every command, window, app, setting and action) | 0.87 ms | 3.23 ms | 3.65 ms |
| ↑ or ↓ in the palette | 0.32 ms | 2.87 ms | 4.70 ms |

### Live, under the headless compositor at scale 2

`aizome --backend headless --headless-output 2880x1800@2 --no-shell`, the
shell's release build with `SIDESTEP_FRAME_STATS=1`, driven with
`aizome-shell msg` and `aizomectl input` (record / rasterise / present,
total):

- The bar, steady (clock and meters): total p50 0.03–0.06 ms, p95 under
  0.25 ms, damage 2–9%.
- The palette, typing and moving: p50 about 1 ms.
- The Toolchest (a kit menu popup): its first frame 1.5 ms.
- Idle: no frames but the clock's each minute and a meter's when a bar
  visibly moves (the sampler only wakes the shell then); 2–4 clock ticks
  of CPU (20–40 ms) over 20 s, about 0.1–0.2%.

First frames (17 October, Sidestep's banded rasteriser, the machine at a
load of about 17 from other builds):

| Surface | Before | Now |
|---|---|---|
| The desktop, its colour | 89 ms | 2.8 ms |
| The desktop, with its wallpaper | 106 ms | 4.7 ms |
| The palette's first opening after start | 6.7–9.9 ms | 5.4 ms (record 0.7, rasterise 4.6) |
| The palette opened again | 2.3 ms | 0.9 ms |

The wallpaper is read, decoded and scaled to each output's size in pixels
on a worker thread (`wallpaper.rs`), once per output size and theme; the
desktop shows its colour until it's ready, and then the picture lands one
to one on the output's pixels, drawn without filtering: a copy. The
palette's text (its sizes and weights, and the rows it opens with) is laid
out once, two seconds after the shell starts, so the faces are loaded and
the shaping warm when it first opens: 4–14 ms of the main thread, once, at
idle. Headlessly (`tests/perf.rs`, `the_palettes_first_opening_after_warming`,
release, the whole surface rasterised at 2× on one thread): warming 4 ms,
the first opening 7.2–8.8 ms, again 5.3 ms.

### Memory

RSS with a bar, desktop and Deskbar on one 2880 × 1800 output: 77 MB with
the desktop colour, about 120 MB with a 2880 × 1800 wallpaper (the scaled
picture, 20 MB, and the desktop's raster; the decoded original lives only
on the worker while it's scaled), up to about 150 MB after the palette's
first use (text and glyph caches, its surface's buffers). Pictures for
another output size or the other theme are let go. The file index (20 MB
for a large home folder) is read only while the palette is open and
dropped when it closes; it's rebuilt in the background when older than ten
minutes, or when its folders or the ones it skips change.

### Exceptions

- **The palette's first opening**: *fixed* on 19 October. The palette's
  window is kept hidden between openings and drawn ahead at idle, in
  bands of about 1 ms, so opening it copies a ready picture: 0.77 ms
  median (0.65–1.25) live at load 0.1–2.9, against 5.51 ms before in the
  same runs; opened again 0.83 ms (4.45 before). See exception 4 above.
  What follows is the history.
- **The palette's first opening (before 19 October)** rasterised its whole surface (1446 × 966
  pixels at 2×: the panel, its shadow and some sixty runs of text) on the
  render thread: 5.4 ms on a loaded machine, against 0.9 ms once the
  surface exists. Laying its text out ahead removed the main thread's part
  (record 16.5 ms cold before, 0.7 ms now); what's left is pixels, which
  only a smaller palette would cut. On 18 October (release, commit
  `fd62197`, Aizome's headless compositor at 2880 × 1800, scale 2, ❖Space
  through `aizomectl input` four seconds after start, load 9): 6.4 ms
  (record 0.7, rasterise 5.1, present 0.65), opened again 1.7 ms; in the
  harness, warming 4.0 ms, the first opening 6.3 ms, again 3.65 ms. The
  same run's first frame of the desktop (its colour and icons, no
  wallpaper) was 8.9–11 ms, nearly all rasterising into the new
  2880 × 1800 surface, where 2.8 ms was written above: once per output,
  not investigated yet.

- **The desktop's wallpaper frame** copies 2880 × 1800 pixels (4.7 ms
  under load); it happens once per output size and theme, and the desktop
  redraws only an icon's rectangle after.

## Settei: Network, Bluetooth, Sound, Displays, Printers

Each pane's typical interaction, over the sample, headlessly at 2× (the
pane's update, the window's view, layout and diff, and rasterising the
damage): `apps/settei/tests/budget_hw.rs`, debug build, on a laptop
busy with other builds (a second run's p95s were up to twice these).

| Interaction | p50 | p95 | max | Redrawn |
|---|---|---|---|---|
| Network: the Wi-Fi switch (the networks go, or come back) | 2.94 ms | 3.56 ms | 4.05 ms | 70% |
| Displays: dragging a display, per pointer move | 0.27 ms | 0.39 ms | 1.20 ms | 5% |
| Sound: the input meter's tick (12.5 a second while the pane shows) | 0.58 ms | 0.64 ms | 0.95 ms | under 1% |
| Bluetooth: a device's options opened or closed | 1.91 ms | 2.20 ms | 2.26 ms | 51% |

A drag in the arrangement doesn't view the window again: the widget
moves the tile itself and redraws only the old and new places of the
tile and of the dashed outline where it will land (5% of the window
instead of the whole well, 18%, which took 1.5/4.4 ms p50/p95). The
input meter is one keyed element, so its tick redraws only itself.

Live, under Aizome's headless compositor at 2× (`aizome --backend
headless`), `aizome-settei --sample --pane displays` with
`SIDESTEP_FRAME_STATS=1`, a display dragged with `aizomectl input`:
total p50 2.2 ms, p95 3.5 ms, max 3.5 ms over the drag and the drop (the
drop redraws the pane, 31% damage on average).

No exceptions: the Wi-Fi switch and a device's options change much of
the page, and still fit.

## Settei (Settings)

Three typical interactions, measured headlessly by `tests/budget.rs`
(`typical_interactions_fit_the_frame_budget`) over the sample system: the
update (for a setting: reading `settings.toml` again, changing it, and
handing the text to the thread that writes it), the view of the whole
window, layout and diff, and rasterising the damage at 2×.

| Interaction | p50 | p95 | max | |
|---|---|---|---|---|
| Flipping a switch (Mouse › Natural scrolling) | 0.10 ms | 0.12 ms | 0.35 ms | `--release`, 37 flips |
| A key typed in the sidebar's search: the results, and the highlights in the pane | 0.24 ms | 1.16 ms | 1.92 ms | `--release`, 60 keys |
| Another pane from the sidebar: a whole new form laid out and drawn | 1.79 ms | 2.67 ms | 3.58 ms | `--release`, 30 moves |

Debug builds (Settei and the kit unoptimised) take several times
longer: a key in search p95 16 ms, another pane p95 27 ms.

Writing `settings.toml` happens on a thread of its own, in order: replacing
the file by renaming over it can wait on the disk (ext4 flushes the data of
a file renamed over another), and did, taking a switch's p95 to 12 ms when
the write was in the update. The window shows the change at once (and
rethemes itself through `Cx::set_appearance`); the compositor is asked to
`reload-config` once the file is written, once for a burst of changes.

Live, under the headless compositor (GLES) at 1×, Performance's read-out
of this window in a debug build: view and layout p95 1.3 ms, recording the
drawing p95 1.8 ms.

No exceptions.

## Sumi (Text Editor)

Measured headlessly through the kit's `Harness` at 2× (`tests/sumi.rs`,
`typing_and_scrolling_fit_the_frame_budget` and
`a_large_file_opens_quickly_and_stays_quick`): update, view, layout, diff
and rasterising the damage. The editor lays out only the lines that show
and redraws only the lines that changed; colouring re-tokenizes only the
lines an edit touched (and those after it whose state it carried over),
within 3 ms a frame.

| | p50 | p95 | max | |
|---|---|---|---|---|
| Typing in the board's document (Markdown) | 0.79 ms | 0.96 ms | 3.3 ms | release, n=185 |
| Scrolling it (30pt wheel steps) | 0.85 ms | 0.94 ms | 1.2 ms | release, n=60 |
| Typing at the end of a 10 MB file (151,809 lines of Rust) | 1.2 ms | | 6.2 ms | release, n=25 |
| Typing in the board's document | 1.8 ms | 5.6 ms | 15 ms | debug |
| Scrolling it | 1.8 ms | 7.2 ms | 8.0 ms | debug |

A 10 MB file opens in 23 ms (release; 112 ms debug): read, decoded and
made a rope on a thread of its own, so the window never waits. Live,
under headless sway (pixman, 1×, release, typing with `wtype`), Sidestep's
frame stats: total p50 0.11 ms, p95 0.96 ms, max 2.1 ms, average damage
24%.

Exception: ❖↓ to the end of a 10 MB file costs 8.9 ms once (release;
8.7 ms again on 18 October, the colours exact 15 frames and 44 ms later):
3 ms working out line colours (the rest are guessed, as if nothing were
open, and worked out over the next 16 frames, 47 ms in all), the rest
shaping a screen of lines nothing has shown yet. Debug builds of typing
and scrolling exceed the budget at p95; release builds keep well inside
it.

## Tansu (Disks)

Measured headlessly through the kit's `Harness` at 2× (`apps/tansu/tests/perf.rs`,
`typical_interactions_fit_the_frame_budget`, on the sample disks): the
update, view, layout and diff, and rasterising each damaged rectangle
(a selection changes strips at the top, middle and bottom of the window;
rasterising their union would mean the whole window). Release build, on
this laptop while other builds ran (load average about 10):

| | p50 | p95 | max | |
|---|---|---|---|---|
| A click on the partition map (selection moves) | 1.45 ms | 1.55 ms | 1.6 ms | n=50 |
| ← → along the map | 0.68 ms | 0.71 ms | 0.71 ms | n=20 |
| Queueing a deletion, and undoing it | 2.0 ms | 3.6 ms | 4.1 ms | n=40 |
| Switching drives | 1.8 ms | 5.2 ms | 5.4 ms | n=30 |

Debug builds: about twice as long. Two choices keep these down: the
details panel under the table has a fixed height and the pending strip
is always there (saying nothing is pending), so a new selection or the
first queued operation doesn't move the table and redraw it whole.

Exception: switching drives redraws the whole content area (the map,
legend, table and details all change), 1744 × 1300 pixels at 2×: under
2 ms at the median, up to 5.4 ms at the tail while the machine was busy.
It happens on a click in the sidebar, not while typing or dragging.

Again on 18 October (release, commit `fd62197`, load 4–9): a click on the
map 1.30/1.53 ms (p50/p95), ← → 0.62/0.65, queueing and undoing
1.44/2.85, switching drives 1.61/4.45 ms (max 4.72).

## Myaku (Activity)

The sampler reads the system on a thread of its own, once a second: a
reading of ~520 processes costs 7 ms in a release build (15 ms in a debug
one), about 0.7% of one core. It reads `stat` for every process, but
`status`, `io` and DRM's `fdinfo` only for processes that ran since the
last reading (one that didn't can't have woken, read or written; the rest
every fifth reading), memory maps (`smaps_rollup`, which walks page
tables) only when a process's resident size moved and at most 48 a
reading, the CPU sensor's one file rather than every sensor (reading an
NVMe drive's sensor takes milliseconds), and launcher entries only when
a group is new.

The window, with 600 processes (`apps/myaku/tests/perf.rs`, made-up
readings, 2×, rasterising each damaged rectangle), release:

| | p50 | p95 | max | |
|---|---|---|---|---|
| A new reading (most visible figures and the four graphs change) | 2.96 ms | 3.5 ms | 3.8 ms | n=60 |
| A click on a row | 0.21 ms | 0.29 ms | 11 ms | n=30 |
| Scrolling the table (48pt steps) | 2.7 ms | 3.0 ms | 3.1 ms | n=30 |

Of a reading, the update (re-sorting the groups and processes) takes
0.06 ms and the view and layout 0.9 ms; the table builds only the rows
that show. Two hundred readings later the process's memory is where it
was: a reading replaces the last, and the graphs keep two minutes in
fixed rings.

Live, under headless sway (pixman, 1×, release, this laptop's 520
processes), Sidestep's frame stats over 25 s: a frame a second, total
p50 0.9–2.0 ms, p95 1.2–2.8 ms; damage 87% (the figures of every visible
row change each second). Resident memory 36.6 MB.

No exceptions.

## Shashin (Images)

Headlessly, `apps/shashin/tests/budget.rs` (`cargo test --release -p
aizome-shashin --test budget -- --nocapture`), a 3000 × 2000 camera JPEG in
a 1440 × 900 window, from the event to the damage rasterised at 2× (the
picture fills 2880 × 1740 pixels of it). Release build, with Aizome's
fonts, on 17 October, before Sidestep's fast path for images landing one
to one (the numbers since are under the exception below):

| Interaction | frame p50 | p95 | max | Shashin's part p50 / p95 | drawn |
|---|---|---|---|---|---|
| Showing a fitted photo (its drawing for the fit, one to one) | 22.2 ms | 28.8 ms | 30.3 ms | 0.02 / 0.02 ms | 100% |
| Panning at 100% by dragging | 24.2 ms | 30.2 ms | 31.6 ms | 0.06 / 0.09 ms | 70% |
| Pinching (drawn from what's there, nearest pixel) | 30.3 ms | 38.8 ms | 45.7 ms | 0.11 / 0.27 ms | 100% |
| A zoom step (❖+, ❖−), settled | 27.5 ms | 31.1 ms | 31.6 ms | 0.02 / 0.03 ms | 100% |
| The next picture (read ahead) | 6.0 ms | 28.8 ms | 29.7 ms | 0.08 / 4.0 ms | 99% |

Shashin's part (the event, update, view, layout and diff) stays far
inside the budget: decoding, scaling, turning, SVG rendering, thumbnails
and the next and previous pictures all happen on the kit's worker threads,
a camera's EXIF thumbnail shows while a JPEG decodes, and every frame at
rest draws a picture already scaled to exactly the zoom, one device pixel
to one, with `BlendMode::Copy` for opaque pictures and the `sunk` ground
filled only around them. The next picture's p50 is low because most of
the test folder's pictures are small; a full-window photo costs what the
other rows do.

Live, the release build under headless sway (pixman) at 2× on a 2880 ×
1800 output, a 6000 × 4000 JPEG, Sidestep's frame stats: ❖+ and ❖− steps,
23 frames, rasterise p50 24.7 ms, p95 30.8 ms, record 23 µs, damage 95%.

Exception, now fixed: **a picture filling a window at 2×**. All of a
frame's cost was Sidestep rasterising the picture: it drew images through
tiny-skia's pattern pipeline at about 4 ns a device pixel whatever the
scale or filter (`apps/shashin/examples/drawbench.rs`: a one-to-one
2800 × 1800 blit took 21 ms, 18 ms with `Copy`; a solid fill of the same
area 0.3 ms), so any frame that redrew a full-window photo at 2× (opening
it, panning, zooming) took 18–30 ms. Sidestep now copies images whose
pixels land one to one on whole device pixels row by row (and composites
translucent ones in integers), which is how Shashin draws every frame at
rest.

Measured again on 18 October (release, commit `fd62197`, load 4–9), the
same test: showing a fitted photo p50 3.30 ms, p95 3.68 ms (22.2/28.8
before); panning at 100% 3.25/3.56 (24.2/30.2); pinching 4.23/4.57
(30.3/38.8); the next picture 3.27/3.98; Shashin's own part still
0.02–0.19 ms. Each of these frames covers the window and so also pays for
the harness's own 2880 × 1800 buffer (about 1 ms). The harness's zoom step
(the frame drawn from the old drawing, scaled to the new zoom, before the
drawing for it arrives) is 8.72/9.43 ms. Live, the release build on
Aizome's headless compositor (GLES) at 2880 × 1800, scale 2, a 6000 × 4000
JPEG, ❖+ ❖+ ❖− ❖− every 0.7 s for 30 s through `aizomectl input` at load
0.5: one frame per step, total p50 1.2–1.5 ms, p95 1.6–2.2 ms per
five-second summary (24.7/30.8 ms before), damage 93%. Pinching, at 4.6 ms
p95 in the harness (not measured live), is the one interaction left above
the budget.


## Annai (Welcome)

Headlessly, `apps/annai/tests/budget.rs` (`cargo test --release -p
aizome-annai --test budget -- --nocapture`), the 760 × 600 window, from the
clock's tick to the damage rasterised at 2×, release, with Aizome's fonts:

| Interaction | p50 | p95 | max | drawn |
|---|---|---|---|---|
| A frame of a scene (any of the seven, 30 a second) | 1.07 ms | 1.56 ms | 7.5 ms | 49% (the scene) |
| Turning a page | 2.14 ms | 3.31 ms | 5.1 ms | 99% |

A frame views the window again (the scene's moment is its canvas's
state), but only the scene changes, so only it is drawn: shapes and labels
from the theme, no images. Under reduced motion the scenes hold still and
nothing ticks.

Live, the release build under headless sway (pixman) at 2×, Sidestep's
frame stats over two five-second spans while scenes played: 154–156
frames each, total p50 0.33–0.53 ms, p95 2.1 ms, max 3.1 ms, damage 48%.

No exceptions.

## Hako (Files)

Hako's typical interactions are the keyboard walking a column (↓: the
selection moves, and with it the path bar's last crumb, the status line
and the preview column), the wheel scrolling a column, and ↓ in the list
view. `apps/hako/tests/perf.rs` measures each frame in a folder of
10,000 files: the event, update, view, layout and diff, and rasterising
the damage at 2× through the offscreen renderer. The views build nothing
per row (one widget paints the rows that show, and only those damaged),
so a column of 10,000 costs what one of twenty does.

| Interaction | build | p50 | p95 | max |
|---|---|---|---|---|
| ↓ in a column of 10,000 | release | 1.9 ms | 3.3 ms | 3.5 ms |
| scrolling it, 40 pt a notch | release | 0.76 ms | 0.92 ms | 1.8 ms |
| ↓ in a list of 10,000 | release | 1.8 ms | 3.3 ms | 6.1 ms |
| ↓ in a column of 10,000 | debug | 3.1 ms | 5.6 ms | 8.9 ms |
| scrolling it | debug | 0.93 ms | 1.1 ms | 1.5 ms |
| ↓ in a list of 10,000 | debug | 2.1 ms | 2.7 ms | 5.2 ms |

(115 frames each; the machine's load average was 17–19 from other
builds, which moves the tails.) Of a ↓ frame the program's part is small:
update p50 1 µs, view and layout p50 39 µs (p99 214 µs), recording the
paint p50 92 µs; the rest is rasterising.

Live, the debug build under headless sway (pixman) at 2×, `wtype` pressing
↓ 60 times and Page Down 30 in the same folder, Sidestep's frame stats:
176 frames, total p50 1.5 ms, p95 4.8 ms, max 6.2 ms, damage 25%.

### Exceptions

1. **The preview column** is redrawn whole when the selection moves to
   another file (its picture, name, facts and the start of its text all
   change): about a quarter of a 980 × 600 window at 2×. That is most of a
   ↓ frame's rasterising, and puts the tail (p95 4.8 ms live, debug, under
   load) at the budget. In the list and icon views, which have no
   preview, ↓ redraws two rows and the status line.
2. **Listings and thumbnails** are read and made in the background (a
   folder of 10,000 lists and sorts in 22 ms (release), 53 ms (debug); a
   2880 × 1800 PNG's two thumbnails take 170 ms; `perf.rs`,
   `listing_and_thumbnails_take_what_they_take`), never in a frame; their
   arrival is one frame of the rows that show.

## Fusen (Notes)

Typing in a note, measured headlessly at 2× (`tests/fusen.rs`,
`typing_fits_the_frame_budget`): update (the list's title and preview
follow the typing), view, layout, diff and rasterising the damage.

| | p50 | p95 | max | |
|---|---|---|---|---|
| Typing in a note | 0.08 ms | 0.11 ms | 0.20 ms | release, n=66 |
| Typing in a note | 0.40 ms | 3.1 ms | 14 ms | debug |

The notes folder is read again every 3 s (a few hundred small files cost
a few milliseconds), and stickies' places are asked of the compositor on
a thread of the kit's. No exceptions.

## Koyomi (Calendar)

`apps/koyomi/tests/perf.rs`, release build, at 2×, in the thread's CPU
time (the machine had a load average of 15–21 from parallel builds, which
stretches wall-clock time): from the key or pointer event through the
update, the view, layout and diff, to rasterising each damaged rectangle
through the offscreen renderer.

| Interaction | p50 | p95 | max | |
|---|---|---|---|---|
| Next or previous month (← →) | 2.9 ms | 3.2 ms | 3.4 ms | 76% of the window redraws: every day changes |
| Next or previous week | 1.5 ms | 1.7 ms | 1.8 ms | the grid, header and all-day strip redraw |
| A step of dragging an event across the week | 0.001 ms | 0.14 ms | 0.24 ms | 240 steps: no update or view, only the ghost's old and new places redraw (most steps don't move it a 15-minute snap) |
| Dropping it | 0.29 ms | 0.69 ms | 0.98 ms | the event moves; its old and new blocks, the status line and the sidebar's Up Next redraw; the file is written on a thread |

Live, under headless sway (pixman) at 2× (2240 × 1480), switching months
and weeks with `wtype`, Sidestep's frame stats: total p50 1.7–2.2 ms
(record 0.1 ms, rasterise 1.5–1.8 ms), p95 4–9 ms on the loaded machine,
where `present` alone reached 1–2 ms at p95 against 0.03 ms at p50.

How the week view keeps a drag cheap: the time grid is one widget that
draws the hours and columns, each event a child block it places, the
now-line another child. During a drag the grid keeps the drag in its own
state, draws the ghost over its children and invalidates only the ghost's
old and new rectangles; nothing is viewed or laid out again until the
drop. A month switch can't be cut much below one pass over the grid's
pixels, so the month's days fill no ground of their own (the window's is
already the surface).

No exceptions.

## The login screen (aizome-greeter)

Headlessly, `crates/aizome-greeter/tests/budget.rs` (`cargo test --release
-p aizome-greeter --test budget -- --nocapture`): the login surface at 1440
× 900 points over the design's night wallpaper at 2880 × 1800, three people
and two sessions, from the key or the tick to its damage rasterised at 2×,
release, on 20 October 2026:

| Interaction | p50 | p95 | max | drawn |
|---|---|---|---|---|
| Moving through the people (→) | 0.11 ms | 0.36 ms | 15.9 ms | 2.4% |
| Typing a password (and Backspace) | 0.06 ms | 0.07 ms | 0.24 ms | 0.8% |
| A wrong password's shake (each 30 ms step) | 0.10 ms | 0.23 ms | 1.67 ms | 2.3% |
| The clock's minute | 0.01 ms | 0.01 ms | 0.01 ms | 0.0% (same minute) |

The wallpaper is decoded and scaled once per display, on a thread, and
then only copied. A key redraws only what changed: the field, or the two
tiles whose selection moved. The shake moves the field's row.

### Exceptions

1. **The first move through the people** (15.9 ms, once): the first frame
   that draws the unselected names and pictures fills the kit's glyph and
   path caches. Every later frame takes 0.1–0.4 ms. *Accepted*: it happens
   once per greeter, and only with several people.

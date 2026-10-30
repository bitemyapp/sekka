# Hotaru against Ghostty on real GPUs, 2026-10-30

This compares both applications' shipped rendering paths on two real GPUs.

- **Ghostty** draws with OpenGL, and GTK presents its frames through GL.
- **Hotaru** has no GPU renderer yet. It draws on the CPU, and the
  compositor puts its buffers on screen with the GPU.

Earlier GUI comparisons ran everything in software. In the
[2026-10-30 comparison](ghostty-comparison-2026-10-30.md), Ghostty ran on
llvmpipe in a pixman compositor.

The applications are the same builds on both machines:

- **Ghostty:** the pinned reference `6ba14415`, built from the same derivation
  on each (`/nix/store/rvbnq9b2…-ghostty-1.4.1-dev+6ba14415…`).
- **Hotaru:** #142 at `86ca1374`
  (`/nix/store/pyy32rwyrpgnnpwlc5fmw5r88px0jrs6-hotaru-0.1.0`).

| | wx-workstation | ganymede (laptop) |
| --- | --- | --- |
| GPU | NVIDIA RTX PRO 6000 Blackwell Max-Q, driver 580.159.04 | Intel Panther Lake Xe, Mesa 26.2.5 |
| Compositor | Ubuntu's sway 1.9, GLES 3.2 | nixpkgs sway 1.12, GLES 3.2 |
| CPUs: apps; sway; observer, controller, fixture | 8–11; 12–13; 14, 15, 16 | 0–2 (performance cores); 3; 4, 5, 6 |

## Method

`tools/frame-benchmark.py --renderer gpu` runs each application in a private
headless sway that renders with GLES2 on the GPU. Everything else matches the
[frame benchmark](frame-benchmark.md):

- the same fonts;
- a 120×40 grid of 11×24-pixel cells;
- zero scrollback;
- a fresh home and configuration for every trial;
- a screencopy observer that times each frame up to its capture.

`tools/frame-matrix.py` alternates the applications and runs three phases:

- **Startup:** four pairs.
- **Sustained:** two pairs. Each trial has 500 key presses, four 30-second
  idle controls, and nine streams of 120 updates.
- **Throughput:** four pairs. Each trial writes four corpora of 64 MiB through
  the terminal.

Every trial on both machines was accepted.

**Ghostty's GPU path is checked in every trial.** A measurement stops if GTK
falls back to Cairo or Ghostty falls back to CPU presentation. The records
also hold:

- the compositor's GL renderer;
- the GL libraries each application had loaded;
- the DRM devices each application held open.

On wx, Ghostty runs on the host's NVIDIA driver through
[`wx-ghostty-env.json`](../upstream/gui-gpu-2026-10-30/wx/wx-ghostty-env.json):

- NVIDIA's EGL libraries;
- `libwayland-server` and `libdrm` from Ghostty's own closure, which NVIDIA's
  Wayland EGL platform needs;
- membership of the `render` and `video` groups.

Without that setup, GTK fell back to Cairo and Ghostty to CPU presentation.
Hotaru gets no GL environment: it draws on the CPU and holds no GPU device.

**Throughput** writes each corpus through the PTY as fast as the terminal reads
it, so the write time is the terminal's own speed.

- **The corpora** are the reviewed `ascii`, `styled`, `unicode` and `cursor`
  patterns, 8 whole repeats of each 8 MiB file.
- **Terminal defaults first:** the fixture turns autowrap and the cursor back
  on, then writes the corpus.
- **Then a closing frame:** the fixture paints a marker frame. Its capture is
  the time to the final frame.

## Results

Medians across trials. Each idle figure is the application's CPU over one
30-second idle control.

| | wx Hotaru | wx Ghostty | laptop Hotaru | laptop Ghostty |
| --- | ---: | ---: | ---: | ---: |
| Launch to PTY ready | 54.5 ms | 386.3 ms | 49.3 ms | 369.2 ms |
| Launch to first matching frame | 66.0 ms | 476.0 ms | 71.7 ms | 410.5 ms |
| Key to frame, p50 | 28.3 ms | 30.0 ms | 30.2 ms | 32.9 ms |
| p95 | 35.9 ms | 37.1 ms | 37.5 ms | 39.5 ms |
| p99 | 37.7 ms | 40.0 ms | 38.6 ms | 40.5 ms |
| max (1,000 presses) | 51.9 ms | 86.1 ms | 40.3 ms | 41.7 ms |
| Idle CPU per 30 s | 0 ms | 80–130 ms | 0–10 ms | 70–80 ms |
| Resident memory | 55–56 MiB | 201–206 MiB | 53 MiB | 199 MiB |

**Paced updates.** Both applications delivered all 120 updates of every
marker, sparse and scrolling stream, at 30 and at 60 Hz. The software pilot's
shortfall at 60 Hz scrolling (114 of 120) doesn't occur with a GPU compositor.
Per 120-update stream:

| | wx Hotaru | wx Ghostty | laptop Hotaru | laptop Ghostty |
| --- | ---: | ---: | ---: | ---: |
| Application CPU | 40–100 ms | 170–250 ms | 80–200 ms | 100–170 ms |
| Compositor CPU, 60 Hz scroll | 760–790 ms | 450–460 ms | 220–230 ms | 150–160 ms |
| Compositor CPU, other streams | 500–1,320 ms | 450–930 ms | 160–360 ms | 120–280 ms |

**Throughput,** 64 MiB per corpus. The cell is the time to consume it, and the
throughput.

| Corpus | wx Hotaru | wx Ghostty | laptop Hotaru | laptop Ghostty |
| --- | ---: | ---: | ---: | ---: |
| ASCII | 148 ms (432 MiB/s) | **91 ms (715 MiB/s)** | 223 ms (287 MiB/s) | **185 ms (347 MiB/s)** |
| Styled | **291 ms (220 MiB/s)** | 301 ms (213 MiB/s) | 408 ms (157 MiB/s) | **245 ms (261 MiB/s)** |
| Unicode | 470 ms (136 MiB/s) | **351 ms (183 MiB/s)** | 533 ms (120 MiB/s) | **332 ms (193 MiB/s)** |
| Cursor/erase | **660 ms (97 MiB/s)** | 3,148 ms (20 MiB/s) | **767 ms (83 MiB/s)** | 3,085 ms (21 MiB/s) |

The final frame followed 25–35 ms after the input was consumed, for both
applications. Both kept presenting at about the output's 60 Hz during a
flood. On wx, the observer captured 41–43 frames in Hotaru's 660 ms
cursor/erase run, and 195–198 in Ghostty's 3.1 s one.

## What it shows

- **Hotaru leads on startup, latency, idle cost and memory, on both GPUs.**
  - It starts six to seven times faster.
  - Its typing latency is 1.7–2.7 ms lower at the median, with a shorter tail.
  - It uses almost no CPU at rest.
  - It holds about a quarter of Ghostty's memory.
- **Hotaru costs the compositor more.**
  - Its CPU-drawn buffers are copied to the GPU on every update.
  - Ghostty's are already there.
  - Scrolling at 60 Hz costs sway about 70 % more with Hotaru on wx, and about
    45 % more on the laptop.
- **Ghostty leads bulk throughput everywhere except cursor/erase.**
  - On the laptop it's 1.2–1.7× faster. On wx it's 1.3–1.6× faster on ASCII
    and Unicode, and even on styled text.
  - Hotaru is 4–5× faster on cursor/erase, as in the core benchmark.
- **Hotaru's limit is not its parser.** Its core feed parses ASCII at about
  1,490 MiB/s on wx, but the full application reaches 432 MiB/s there.
  Ghostty's core runs at about 1,258 MiB/s and its application at 715 MiB/s.
  The path from the PTY to the screen costs Hotaru about twice what it costs
  Ghostty. The likely parts are reading the PTY, handing data to the terminal
  and drawing, but the run doesn't isolate them.

## Limits

- **Ghostty is on its GPU path; Hotaru isn't on one.** This compares what ships,
  not GPU renderer against GPU renderer. Hotaru's
  [native-scene GPU prototype](gpu-native-scene.md) isn't integrated.
- **No physical display.** The outputs are headless, so each time ends when the
  compositor's frame is captured, not at scanout. The observer's screencopy adds
  compositor work for both applications. Its 24-bit and 10-bit read formats
  were added for wx's NVIDIA compositor, and the laptop's startup and sustained
  runs used the earlier observer, which read XRGB.
- **The laptop was in interactive use.** Each trial waited for a quiet second
  on its CPUs, and Hotaru and Ghostty alternated, but the laptop's own activity
  is noise that wx's runs don't have.
- **Comparisons hold within a machine, not across them.** The two sway
  versions differ, and wx's compositor CPU is much higher for both
  applications.
- **What the checks cover.** Markers check the frame's sequence and the run's
  nonce, not every glyph pixel. Process memory in this zero-history fixture
  says nothing about retained history.

## Records

[`upstream/gui-gpu-2026-10-30/`](../upstream/gui-gpu-2026-10-30/) has one
folder per machine. Each holds:

- `summary.json`, from `tools/frame-summary.py`;
- the calibration;
- compressed raw trials for each phase, with the tools as they ran, but
  without the per-run observer binaries.

wx's folder also has its Ghostty environment and run scripts.
`tools/frame-bench-shell.nix` provides the tools the harness needs on NixOS.

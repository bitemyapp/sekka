# Linux GUI reference and exploratory probes

`tools/reference-gui.py` builds the pinned Ghostty `1.4.1-dev` Linux application
through its unchanged `nix/package.nix`, using Sekka's locked Nixpkgs source.
The external reference remains clean. This build does not change the existing
native terminal-only oracle or add GTK to Hotaru. The build command, exact
package and executable hashes, Nixpkgs NAR hash, loader dependencies and preserved
VT oracle hashes are in [`ghostty-gui-baseline.json`](../upstream/ghostty-gui-baseline.json).
Both application packages target baseline x86-64; the separately recorded
terminal-core benchmarks target the workstation's native CPU.

```sh
python3 tools/reference-gui.py --nixpkgs /nix/store/87lfqfz7mc5dm4w75qikxhd6lkr9jisr-source
```

The packaged reference uses Zig 0.17.0, ReleaseFast, GTK 4.22.4, libadwaita 1.9.3
and OpenGL. Host `ldd` can report false missing GLIBC symbols when inspecting
Nix binaries with its older loader. The build tool instead checks the ELF's
actual interpreter with `--list` and executes the packaged `--version` entrypoint.
On this non-NixOS workstation, the GL application also needs matching Nix Mesa;
the host EGL driver cannot load against this package's C library.

## October 28 checkpoint probe

`tools/gui-probe.py` runs a fresh application and PTY fixture in a private Sway
pixman session, with a private D-Bus that cannot activate desktop services.
Each run cleans up its processes and temporary directory. An out-of-band Unix
socket controls the fixture, so the fixture sleeps without polling during idle
measurements. The tool calibrates the window until `TIOCGWINSZ` reports exactly
120 columns and 40 rows. It uses the same Aizome font directory and disables
history. It records wrapper and running ELF hashes separately.

The six retained runs alternate Ghostty and Hotaru, three fresh processes each,
on CPUs 8–11 under the shared workstation lock. All use probe SHA-256
`bb2eff9f22760d35e3464391f83a2363a0bf2b809ee97a4cd621da118c31bf58`.
Their individual timings, process samples, exact commands, corpus hashes and
configurations are in [`upstream/gui-probes/`](../upstream/gui-probes/).
The Hotaru ELF is the earlier tested checkpoint, SHA-256
`27b62cc38cdb4b8f7d79a22446fb5bea2bc9a222aea5802f77f93fce46ea1917`;
these are **not measurements of the subsequent rich-copy implementation**.

| Median of three processes | Hotaru checkpoint | Ghostty reference |
| --- | ---: | ---: |
| Process launch to PTY fixture ready | 40.06 ms | 450.02 ms |
| Application CPU during three seconds idle | below 0.01 s counter resolution | 0.15 s |
| Application RSS before workload | 42,376 KiB | 379,192 KiB |
| ~8 MiB ASCII write to final DSR reply | 24.00 ms | 107.99 ms |
| ~8 MiB Unicode write to final DSR reply | 43.89 ms | 162.51 ms |
| ~8 MiB styled write to final DSR reply | 30.44 ms | 131.46 ms |

DSR proves that the parser reached the final cursor query; it does **not** prove
that the corresponding frame was displayed. The workload timer includes PTY
backpressure and parsing, plus any rendering contention during that interval.
It excludes reading the prepared corpus. RSS and CPU count the application
process, not the fixture or compositor. Startup includes cold application font
caches inside a fresh temporary home, but does not flush the kernel page cache.
All three workloads finish with the exact expected cursor reply.

These exploratory numbers do **not establish a performance advantage or parity**:

- Ghostty uses llvmpipe and reports failed DMA-BUF export followed by CPU
  presentation. Hotaru currently rasterizes with Sidestep's CPU backend. This
  comparison does not measure either application on a hardware GPU.
- Both use a 120×40 grid, but Ghostty's 14-point font and Hotaru's current
  14-unit font produce different window sizes: 1334×1004 and 1036×936 pixels.
  Font metrics and equal pixel workloads still need qualification.
- Visual inspection of the original 1920×1440 captures shows missing CJK glyphs
  with this restricted font bundle in both applications. Ghostty renders its
  embedded emoji; Hotaru displays missing-glyph boxes. Unicode rendering is not
  equivalent. The captures are evidence of a gap, not approved interface imagery.
- Hotaru still omits substantial Ghostty features, including parts of shaping,
  fallback, graphics and rendering. Faster incomplete work is not parity.
- Three samples are sufficient to exercise the harness, not to establish
  production latency distributions or a regression threshold.

Original captures and logs are retained on the workstation under
`/home/callen/work/hotaru-gui-probes/*-controlled-*` and locally under
`artifacts/hotaru-gui-probe/`. Earlier harness attempts are excluded from the
table: they exposed incompatible EGL drivers, an undersized private output,
and D-Bus portal activation during cleanup. The retained runs use matching
Mesa, the larger output and a bus without service activation.

Example reproduction (use a new output directory for every run):

```sh
taskset -c 8-11 python3 tools/gui-probe.py ghostty \
  /home/callen/.cache/hotaru-reference/gui-result/bin/ghostty \
  --font-dir /nix/store/b9vp4qd3gz9vmsl20grvd80hniirn5n5-aizome-fonts \
  --mesa /nix/store/457mqa9dsvw9cnmi6pr06zjy1ib0iic4-mesa-26.2.5 \
  --data /home/callen/work/hotaru-corpora/ascii.vt \
  --data /home/callen/work/hotaru-corpora/unicode.vt \
  --data /home/callen/work/hotaru-corpora/styled.vt \
  --output /home/callen/work/hotaru-gui-probes/ghostty-new
```

# Software frame-cadence attribution

The 434-sprite package's scroll gap is partly attributable to its SHM pixel
format in the private Sway/pixman environment. An existing format override on
the identical executable improved observed scroll updates from 114 to 119 of
120, with byte-identical final pixels. No scheduling or default-format change
has been made. This diagnostic does not establish the right format for a
hardware compositor or close the remaining cadence gap.

The earlier [untraced comparison](region-frame-comparison.md) remains unchanged.
These new runs enable application-only `WAYLAND_DEBUG=client` and
`SIDESTEP_TRACE_FRAMES=1`; their times and process CPU counts are diagnostic,
not replacements for uninstrumented application measurements. All execution
was on `wx-workstation` under the shared lock. Applications used CPUs 24–27,
the compositor 28–29, observer 30, controller 31 and fixture 32; activity on
their SMT siblings was also recorded.

## Callback and paint handoff

The first accepted balanced round reproduced the scroll result on the same
Hotaru ELF, with copying enabled and disabled, and unchanged Ghostty:

| Traced 60 Hz scroll | Updates observed | Callback interval median | Previous callback to next frame request, median |
| --- | ---: | ---: | ---: |
| Hotaru, copy | 114/120 | 17.709 ms | 0.300 ms |
| Hotaru, redraw | 114/120 | 17.703 ms | 0.566 ms |
| Ghostty | 120/120 | 16.574 ms | 4.746 ms |

A second Ghostty run observed 120/120 and a 16.580 ms callback median.
The subsequent redraw trial was rejected for seven samples of external
compiler overlap, and the final copy trial was not started. The accepted
Ghostty marker stream observed 119/120 in the first round; all other accepted
marker/sparse streams observed 120/120. These outcomes are retained.

Sidestep's UI waits for the preceding frame callback before recording another
damaged frame. During scrolling, the traced callback-to-request handoff was
already short. Copying reduced that work without reducing callback intervals.
This is evidence against application paint cost alone explaining the gap.

## Controlled format change

Hotaru normally submitted `XBGR8888` buffers (Wayland format 875709016), whereas
the reference submitted `ARGB8888` (0). The follow-up compared Hotaru's native
format with its existing `SIDESTEP_SHM_FORMAT=xrgb8888` control, which performs
the channel conversion in the application. Both retained pixel copying.
The copied diagnostic harness sets this control **after** sanitizing its
environment, records it, and verifies the actual protocol buffer format.

| Accepted traced scroll | Updates observed | Callback median | Application CPU | Compositor CPU |
| --- | ---: | ---: | ---: | ---: |
| Native XBGR | 114/120 | 17.709 ms | 100 ms | 230 ms |
| XRGB, first run | 119/120 | 16.873 ms | 140 ms | 150 ms |
| XRGB, second run | 119/120 | 16.871 ms | 140 ms | 140 ms |

CPU totals sum threads over approximately two seconds and have 10 ms
resolution. The application callback-to-request median increased to about
0.72 ms, while callback cadence improved. This controlled result attributes
much of the gap to the format-dependent transport/compositor path, without
identifying a particular compositor instruction as the sole cause.

The last reverse-order native trial was rejected for external compiler
overlap. All four trials reached the correct final sequence, and their full
1920×1440 RGBA screenshots were byte-identical:
`ecdc7fca54a6156146ea1785a90de47aa3ae98a985021c9f76429fc7183645fe`.
Thus the format override did not merely exchange color channels. The first
XRGB marker stream observed 119/120; other marker/sparse streams observed
120/120. The screenshot check covers the final frame, not every intermediate
glyph pixel; marker identity checks still apply throughout each stream.

## Compositor scope and evidence

The installed Sway is `1.9-1build2`, with wlroots `0.17.1-2.1build1` and a
pixman headless output. The matching
[wlroots source archive](https://archive.ubuntu.com/ubuntu/pool/universe/w/wlroots/wlroots_0.17.1.orig.tar.gz)
sets its headless output timer to `1000000 / refresh` milliseconds, or 16 ms
at 60, on output commit. Sway's
[frame handler](https://github.com/swaywm/sway/blob/1.9/sway/desktop/output.c)
repaints before sending surface frame callbacks when no render delay is
configured. Repaint work therefore contributes to this synthetic cadence.
The Ubuntu packaging patches do not alter these paths; source and packaging
URLs and hashes are retained. No physical refresh or hardware compositor
behavior is inferred from this arrangement.

The Hotaru ELF is
`1d4e649540eed06b5ec4b43e756e1d9b5b8bdbb9a715487903f3613963d0b262`;
Ghostty remains
`ea356b4e5f33e0d0d55af99bb978afe419eed2a933edd31e457554db8970f61f`.
The [machine-readable summary](../upstream/frame-cadence/summary.json),
analysis script, and compressed raw archive preserve controllers, exact
sources, activity, calibrations, every trial, logs, fixture/capture events
and screenshots. The initial controller-preparation attempt failed its ELF
hash assertion before launching an app and is retained separately.

Trace interval statistics use each client's own log clock. Phase assignment
uses compositor monotonic callback timestamps and excludes the first and
last 30 ms of each stream; it does not assume identical log clock origins.
The raw workstation root is
`/home/callen/work/hotaru-frame-cadence-20261028`.

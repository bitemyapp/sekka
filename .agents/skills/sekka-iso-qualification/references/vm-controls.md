# VM controls and evidence

Use packaged `respin-tools` where it fits. It creates `.work/RUN` with `target.raw`,
a private `sockets` link, firmware variables, QEMU/serial logs, and an
`artifacts/RUN` report. Its QMP helper expects a fixed 1280×800 display; check
that assumption before using its coordinates. The GUI install mode can wait
for external input, but `prepare-integrated` changes installer settings and the
installed configuration, assumes fixture credentials, and the graphical-login
check lacks Aizome. It powers off automatically; `boot-installed` attaches the
repository share. These are supplemental tests, not Q2/Q3 certification.
For the primary journey use [direct QEMU](direct-qemu.md).

Supplemental fixture entrypoints (consult `--help` for the current contract):

```sh
nix build .#tools --out-link result-tools
nix build .#vm-fixture --out-link result-fixture
result-tools/bin/respin-tools qemu-test /absolute/candidate.iso --firmware uefi --gui --filesystem ext4 --applications firefox
result-tools/bin/respin-tools qmp-input RUN screenshot
result-tools/bin/respin-tools qmp-input RUN key ctrl+alt+f3
result-tools/bin/respin-tools qmp-input RUN type 'test text'
result-tools/bin/respin-tools qmp-input RUN click 400 300
result-tools/bin/respin-tools guest-exec RUN 'systemctl --failed'
result-tools/bin/respin-tools boot-installed RUN
```

`qmp-input` requires a 40/80 GiB `target.raw`, an artifacts directory and the
harness socket link, and only types printable ASCII, without newlines. It cannot
drive a live-only harness run, a 100 GiB disk or arbitrary resolution. Use direct
QMP for these cases; do not resize an installed filesystem to appease a helper.

The tools may require QEMU, OVMF, basic commands and a fixture on PATH; use the
root flake's pinned package set and the `RESPIN_FIXTURE`, `OVMF_CODE`,
`OVMF_VARS` variables the implementation documents. Do not presume a helper
binary carries its runtime dependencies. A diagnostic helper must not be
installed into the released image just to make testing convenient.

For cases the harness cannot model, start QEMU explicitly and retain its exact
argument vector. Use KVM, a unique QMP UNIX socket, serial log, optional QGA
socket, disposable writable OVMF variables, USB keyboard/tablet, a disposable
virtio/NVMe disk and the unmodified ISO. Model USB media using read-only
`usb-storage`; model optical media separately. No host block-device passthrough.
Use virtio graphics appropriate to the desktop; record acceleration and
software fallback. Bind network forwarding only to loopback, if needed.

QMP: negotiate `qmp_capabilities`; `screendump` with a PNG filename captures the
actual display; `send-key` sends qcodes; `input-send-event` sends absolute
pointer coordinates scaled to 0–32767 and paired button press/release events.
Inspect each screenshot before acting; fixed delays and guessed clicks cannot
establish success. Capture before/after important actions, including failures.
Preserve full-resolution captures with timestamps and a human-readable ledger.

QGA/SSH can collect journals, mount tables, process environments and config
files. It cannot prove an interface was usable. Test text entry, selection,
scrolling, focus, drag/drop and keyboard-only navigation through real input.
For a hang, collect screenshot, serial output and QMP state first. A timeout is
a failure to investigate, not permission to silently retry until green.

Power off cleanly and prove QEMU exited. For disk-only boots, start a fresh
QEMU command with no ISO drive and no installer share, retaining the disk and
firmware variables. Capture `findmnt`, `lsblk`, `/proc/cmdline`, and QEMU's
arguments to prove independence from installation media.

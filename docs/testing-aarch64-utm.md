# Native aarch64 UTM validation — 2026-10-19

The image built from `f61e7cb214cf5300e1b94d81b9c3031a579fbfe1` on
`codex/aarch64-utm` installs and runs Plasma in UTM on Apple silicon with
accelerated graphics, automatic display resizing and working native Sekka apps.
Tatami also signs in, but its display does not automatically follow VM window
resizes. This is an experimental source build; no ARM nightly was published.

## Tested image and environment

| Item | Value |
| --- | --- |
| Local ISO | `artifacts/utm/sekka-aarch64-utm.iso` |
| Size | 5,082,458,112 bytes |
| SHA-256 | `892aa1ef504cc773378855057578c1a4fb8480de54e270d34205121225c92408` |
| Nix ISO output | `/nix/store/p1zdwly00mkgvibci3m4s36wlq9r0kpv-sekka-26.11.20261015.fe13242-aarch64-linux.iso` |
| Native builder | aarch64 Linux in the disposable OrbStack VM `sekka-arm-builder` |
| Mac | Apple M5 Max, macOS 27, 128 GB RAM |
| UTM | 5.0.8, QEMU 10 `virt`, CPU `host`, HVF, six cores, 12 GiB RAM |
| Display | `virtio-ramfb-gl`, Apple Core OpenGL renderer, auto resolution on, Retina off |
| Storage/network | One 80 GiB VirtIO disk; shared network, no inbound forwards |

The ISO was built from a clean commit through the root flake with the existing
dependency lock. Later changes to this report and the setup/limit documentation
do not change the tested image. The imported read-only UTM boot media was compared
byte for byte with that ISO. No host disks or shared folders were attached.

## Component and evaluation checks

| Check | Result |
| --- | --- |
| Native Genkan package | 93 tests passed, two ignored (root/loop-device matrix and external GeoIP) |
| Native Yukimi package | 95 tests passed, one ignored (network nightly lookup) |
| Native Tatami tool | 99 tests passed |
| ARM UTM profile check | Built successfully |
| ARM application catalog | All 31 supported package derivations and the complete selection evaluated |
| Generated installed templates | Plasma, Tatami and GNOME+Xfce evaluated as native `aarch64-linux`; backend validation, unchanged lock, architecture-specific audio and guest-agent assertions passed |
| Existing x86 profiles/cache | Live-profile and binary-cache check derivations evaluated |
| Formatting/release metadata | Touched Nix files formatted; `git diff --check` and two release metadata tests passed |

The template matrix is an evaluation check. GNOME+Xfce was not installed or
signed into in this run. The x86 checks above were evaluated, not rebuilt as
x86 images. Tests requiring real storage operations were not run on the builder
or Mac.

## Disposable UTM installation and disk boot

The final media cold-booted into Plasma. Genkan's welcome/disk/account screens
were inspected at the default UTM size. The complete installation then used the
normal packaged helper's reviewed-request → prepare → exact-disk confirmation
protocol through the QEMU guest agent, rather than driving every installer page.
The test harness was local test equipment and is absent from the image.

Before preparation and again before erasure, the harness required Linux root,
`aarch64`, QEMU, UEFI, mounted installation media, the exact packaged helper,
and exactly one eligible VirtIO disk: `/dev/vda`, 80 GiB, serial
`3ABB52DFF1BA4FDA8861`. It submitted the normal `ERASE /dev/vda` confirmation.
No production privilege or disk-selection guard was bypassed.

The installation selected Plasma (default), Tatami, Firefox, ext4, 12 GiB swap,
system tuning and proprietary software. It returned status zero with a complete
event. Preparation took 60.66 seconds; installation after confirmation took
65.59 seconds (126.25 seconds overall). Copying approximately 11.9 GB took
63.00 seconds, bootloader installation 0.66 seconds, and final flush 0.57 seconds.
These are observations from one VM, not installation-time guarantees.

The ISO drive was disconnected in UTM before booting the installed system.
The guest saw only `vda`: `/` on ext4 `vda2`, `/boot` on `vda1`, swap on `vda3`,
and no `/iso` mount. The installed dependency lock SHA-256 was
`4f20d61557b8d5f14ce91e1b56b07c5dc45e113415c86e1189025cdbeeeaf9e2`.
Two disk boots reached the graphical target in 15.00 and 14.87 seconds, including
UEFI and the boot-menu timeout. Neither reported failed system units.

## Desktop and graphics observations

- Plasma Wayland accepted the test login. KWin used VirGL, with one usable
  `Virtual-1` output and no hidden firmware primary display.
- Both live and installed Plasma followed the UTM window size. Recorded modes
  include 1280×800, 1468×944 and 2560×1370 at about 75 Hz.
- Installed Yukimi opened its overview, showing generation one, the installed
  Firefox choice, package count and store usage. Firefox opened its normal
  first-run/new-tab windows. No failed user units were reported.
- Tatami signed in through the greeter and showed Hyprland, Waybar, the normal
  wallpaper, a terminal and the Displays window. Hyprland reported the DRM
  backend, GL 3.0, explicit sync support and one 1468×944 output at about 75 Hz.
  There were no failed user units. Automatic UTM window resizing did not work
  in Tatami, with or without the X11 SPICE session agent running; the normal
  agent was restored after the diagnostic.
- UTM pause/resume returned to a responsive guest with correct time and no
  failed system units. This did not test putting the Mac itself to sleep.
- The installed guest shut down through its normal `systemctl poweroff` path;
  UTM then reported the VM stopped. The installed VM was left shut down, with
  its normal Plasma session available on the next start.
- Normal paced keyboard input was complete. Very rapid synthetic text injection
  through the UI automation tool dropped characters, so it was not treated as
  evidence about human typing latency.
- macOS-to-Plasma text paste worked. Native Wayland guest-to-macOS copy failed.
  No clipboard shim or unreviewed SPICE patch is shipped.

The live native Wayland `glmark2` run used a 1280×720 window and reported
`virgl (Apple M5 Max)`, OpenGL 4.1, Mesa 26.2.5. Three ten-second scenes measured
1301 FPS (build), 1422 FPS (texture) and 1113 FPS (shading), score 1277. This
measures rendering throughput; the virtual display was about 75 Hz. It does not
measure input-to-display latency, sustained frame pacing or gaming performance.

UTM's Apple Core OpenGL backend was required for the tested GL 4.1 path. Its
default ANGLE/Metal backend exposed only GL 2.1 in the initial experiment.
The VM's firmware framebuffer otherwise appeared as a hidden primary output;
the ARM guest module disables that `Unknown-1` output so app windows remain
on the accelerated display.

## Remaining coverage and operational limits

Plasma is the recommended UTM desktop for automatic resizing. Bidirectional
Wayland clipboard, image/file clipboard transfer, microphone input, audible
speaker output, host Mac sleep/wake, long-running workloads and physical ARM
hardware were not validated. PipeWire enumerated the virtual stereo sink/source;
that is not an audible playback test.

Public Sekka nightlies and the Sekka binary cache still target x86_64. ARM
updates can require local compilation, and switching this development VM to the
current public nightly can remove the required ARM module. The root flake and
generated installed configuration use the signed public Determinate bootstrap
cache for its pinned native Nix binaries. Release publication remains inhibited.

Raw build, installation and diagnostic logs are preserved locally under
`.work/aarch64-utm/logs/`. The ISO and installed `Sekka ARM Test.utm` bundle are
local ignored artifacts, with launch instructions beside them in
`artifacts/utm/README.md`. No physical-device installation was attempted.

# Screenshots

The `01`–`12` pictures are the end-to-end walkthrough's: the real session
(compositor, shell and apps, release builds) running headless, driven
through `aizomectl input …` as a person would drive it, and captured with
`grim` over wlr-screencopy at the output's full resolution. Nothing in them
is staged in an image editor or composited afterwards; the PNGs were only
recompressed losslessly (oxipng, pixel-identical).

Pictures taken before Settings was renamed from Shitsurae to Settei show
the old name wherever Settings appears in them.

| File | What |
|---|---|
| `01-desktop-night.png` | Night. Sumi focused on `architecture.md` with text selected; foot (fish) and Hako stacked as one tab group, slid along the top edge; Mail's three notifications filed under it in the Deskbar; README.md and docs/ parked on the Shelf; home, Trash and `~/Desktop` items as desktop icons |
| `02-menus.png` | The menus board's scene: Sumi's whole menu at the pointer (❖-right-click) with Edit open, the Toolchest and Sumi's Format menu torn off into palettes |
| `03-palette.png` | ❖Space over Sumi, "find": its menu commands with paths and keycaps, apps below, and the shortcut coach for Find and Replace (run three times from the palette earlier) |
| `04-overview.png` | ⌃❖↑: four Desks with their windows, the current one outlined, a "new Desk" slot |
| `05-switcher.png` | ❖Tab, then Tab to Hako and ↓: the apps row and Hako's windows with a live thumbnail |
| `06-settings.png` | Settings on Appearance, named Shitsurae when this was taken |
| `07-day.png` | Day, switched live from Settings: the same session, the Toolchest's Applications open |
| `08-yukimi.png` | Yukimi on this machine's own NixOS configuration (read-only, `AIZOME_YUKIMI_ETC=/.host-etc`, so it shows its hostname and generations) |
| `09-kura.png` | Kura scanning the session's scratch home, with its suggestions |
| `10-tokei-tenki.png` | Tokei's world clocks beside Tenki (forecast from Open-Meteo) |
| `11-marking-menu.png` | Right press and pause on Sumi's tab: the marking menu |
| `12-lock.png` | `aizomectl lock`, a password typed (outside the VM the PAM service `aizome` doesn't exist, so it stays locked) |
| `13-full-effects.png` | The Full effects tier in Night: the Toolchest torn off over the city's lights, Sumi's menu at the pointer with File open over the bridge, the Deskbar with a text clipping dragged from Sumi onto the Shelf; the frosted faces let the wallpaper tint through, with a fine grain and no blur. The system's disk (`nvme0`) is on the desktop |

The `14`–`16` pictures are screen recording's, taken on 18 October 2026
with `scripts/headless-session --output 2880x1800@2` (release builds of
the `screen-recording` branch, GLES on the Intel render node, scratch
`HOME` and a private D-Bus without service activation) and `aizomectl
screenshot`. The recorder was wf-recorder through screencopy: the
headless session has no KMS, and gpu-screen-recorder was a stand-in that
fails, so it never read the real display's planes.

| File | What |
|---|---|
| `14-recording.png` | Recording (`aizome-shell msg '{"request":"record","target":"screen"}'`, as the palette's Record Screen): the red dot and `0:04` leading the systray, and ❖⇧5's Capture menu with Stop Recording, the screenshots and Record the Screen ▸ / Record an Area ▸ |
| `15-recording-palette.png` | ❖Space, "record": Record Screen, with Desktop Audio, with Microphone, and with both (Record Area's follow below) |
| `16-recording-saved.png` | Stopped: the file announced in the bar's line and filed under Screen Recording in the Deskbar, with Show in Files and Open |

The `compositor-*.png` pictures are older ones from the compositor's own
tests.

`13-full-effects.png` was taken on 18 October 2026 after the follow-up
fixes: Aizome at `d488677`, the Sidestep fork at `992e3dc` (see-through
windows get buffers with alpha, so the frost shows through on screen at
all). It ran without the machine's system bus (`DBUS_SYSTEM_BUS_ADDRESS`
pointed at nothing), so the systray has no network or battery and the
desktop's disk came from the mounts and sysfs rather than UDisks.

## How they were made

- **Date**: 18 October 2026 (the session ran from late on the 17th).
- **Builds**: release (`cargo build --release --workspace`, thin LTO),
  Aizome at `de92f1b` (main) against the Sidestep fork's `aizome` branch at
  `53f9919`, with fenix's nightly Rust through the repository's flake.
- **Session**: `scripts/headless-session --output 2880x1800@2` (one
  2880 × 1800 output at scale 2, GLES on the laptop's Intel render node),
  with `WAYLAND_DISPLAY`/`DISPLAY` unset, scratch `HOME` and XDG
  directories, and a private D-Bus session bus without service activation
  (so no portals or other services started). The shell was the
  notification server on that bus; the notifications came from
  `notify-send` there.
- **System services**: read from the machine's system bus (NetworkManager,
  UPower, logind, UDisks2, BlueZ), so the systray shows this laptop's
  battery and Wi-Fi state. Nothing was changed: no network joined, no disk
  touched. Sound came from a private PipeWire with a null sink and source.
- **Fonts**: `AIZOME_FONT_DIR` (Archivo, IBM Plex Mono, Noto Sans Symbols 2: `nix build .#fonts`, see docs/kit.md).
  **Wallpapers**: the design repository's *Da Nang at night* and *Tokyo at
  dusk*.
- The terminal is foot with Aizome's `foot.ini`, running fish from nixpkgs.

The script that drove the session is the walkthrough's (`final.sh` in its
scratch directory), the same keys and clicks a person would use: the
Toolchest's pin to tear it off, ❖-right-click for the menu at the pointer,
a submenu's pin, dragging files from Hako onto the Shelf, and so on.

# Screenshot provenance

README and website images are original PNG captures with links to the full
files. They are not upscaled, composited, or generated illustrations.

The validation captures in `test-results/luks-gui-uefi/` are separate from the
README and website gallery. They are original 1280 × 800 QEMU screendumps from
the [2026-10-21 encrypted GUI installation](test-results/luks-gui-uefi.json),
copied byte for byte without cropping or scaling. They show the live installer
(including a rendering failure of Genkan's former GTK window in the former
Plasma live session), the disk-only unlock prompt, and applications in the
installed Aizome and Plasma sessions. Input was sent as keyboard and pointer
events through QMP; there was no automatic login. The report records the exact
ISO and test-only diagnostics setting.

The captures in `test-results/encrypted-splash/` are original QEMU screendumps
of the boot splash, copied byte for byte without cropping or scaling. The two
`install-*-unlock.png` captures (1280 × 800 UEFI, 1024 × 768 BIOS) are installed
systems from the [2026-10-24 encrypted installations](test-results/encrypted-splash-install.json)
asking for the disk passphrase on the firmware's framebuffer, saved by
`respin-tools` when it found the passphrase field. The `check-*.png` captures
(1920 × 1080) come from the `encrypted-boot-splash` component check: the first
prompt, a prompt after the GPU took over with Caps Lock on, restarting, Caps
Lock at a prompt, and Plymouth's text view after Esc.

The captures in `test-results/encrypted-splash-gui/` are original QEMU
screendumps from the [2026-10-24 encrypted GUI installations](test-results/encrypted-splash-gui.json),
copied byte for byte without cropping or scaling: 1280 × 800 once the GPU's
driver has the display, and 1024 × 768 on GRUB's framebuffer at the BIOS
prompt. Genkan's window and the installed systems were driven only with
keyboard and pointer events through QMP, with no fixture, guest agent or
automatic login. The UEFI captures include the emergency console the initrd
left before the fix; the BIOS run's restart and shutdown frames were
byte-identical to the UEFI run's and aren't repeated.

The captures in `desktop/aizome/docs/screenshots/nisshi/` are original
1440 × 900 `grim` captures, at scale 1, of Nisshi's release build from
29 October 2026. Each window ran alone under a private headless sway
(`desktop/aizome/sukumo/scripts/headless-wayland`, pixman), drawing its own
menu bar since no Aizome shell was running. It read through its real
helper, but from a stand-in journalctl that answers from the made-up boot
of `apps/nisshi/src/fixture.rs` (`cargo run -p aizome-nisshi --example
sample`). No message of a real computer is shown. The window was moved
between views by launching `aizome-nisshi` again with `--cursor`,
`--search`, `--scope` and `--launch`, which hand those to the running
window, not by pointer input. They are not cropped or scaled. The Night and Day goldens
in `desktop/aizome/apps/nisshi/tests/snapshots/` are headless renders from
the kit's offscreen renderer, not window captures.

Desktops are captured whole. An application is captured on its own: the
virtual display is captured with nothing else on it, then cropped to the
application's window, in the display's own pixels. A crop is a rectangle of
the capture, unscaled, with the window's own edges; the bar and the desktop
around it are left out. The shared native Yukimi capture below is an exception:
it keeps the whole Tatami display, including its bar and window edges.

| Image | Original resolution | What it shows |
| --- | --- | --- |
| [Activity](images/sekka/activity.png) | 2320 × 1440 | Tatami's Activity window on its CPU view, after a minute of readings, opened with Super+Ctrl+T from a terminal (Super+Return) measuring how small the system's software compresses (`tar -chC /run/current-system sw \| xz -T2 \| wc -c`, which writes nothing), so that terminal, with its `tar`, `xz` and `wc`, is selected, then moved to an empty workspace with Super+Shift+2 |
| [Activity's Memory view](images/sekka/activity-memory.png) | 2320 × 1440 | The same window on its Memory view (Ctrl+2): each application's proportional, private, resident and shared memory, and memory use and pressure under the list |
| [Tatami menu](images/sekka/tatami-menu.png) | 2880 × 1800 | The whole Tatami desktop, with nothing open but the Tatami menu (Super+Space) |
| [Displays](images/sekka/displays.png) | 1760 × 1600 | Tatami's Displays window, opened with Super+Ctrl+D, showing the VM's one virtual display |
| [Network](images/sekka/network.png) | 1440 × 1520 | Tatami's Network window, joined to a simulated network, with made-up network names |
| [Yukimi](images/sekka/yukimi.png) | 2560 × 1600 | Shared native Yukimi in Tatami, reading the test VM `sekka-demo`; whole display, scale 2 |
| [Plasma](images/sekka/plasma.png) | 2560 × 1600 | Sekka's Plasma session with Konsole running fastfetch over Dolphin |
| [GNOME](images/sekka/gnome.png) | 2560 × 1600 | Historical capture of Sekka's GNOME session with Files over the former GTK Yukimi |
| [Aizome](images/sekka/aizome-desktop.png) | 2880 × 1800 | The whole Aizome desktop: Sumi with a Markdown file, Files and a terminal (running fastfetch) stacked in one tab group, a notification filed under Tenki in the Deskbar |
| [Aizome's menu at the pointer](images/sekka/aizome-menu.png) | 2880 × 1800 | The same desktop with Super + right-click's whole menu of Sumi at the pointer |
| [Aizome's palette](images/sekka/aizome-palette.png) | 2880 × 1800 | The same desktop with the command palette (Super + Space), "find" typed |
| [Settei](images/sekka/aizome-settings.png) | 1896 × 1380 | Aizome's Settings at Appearance, cropped to its window with its tab, frame and shadow |
| [Yukimi in Aizome](images/sekka/aizome-yukimi.png) | 2256 × 1580 | Aizome's native Yukimi reading the test VM `sekka-demo`, cropped to its window |
| [Genkan](images/sekka/genkan.png) | 2216 × 1580 | Genkan's window in Aizome, in its debug-only welcome preview with synthetic memory and cache data, cropped to its window |

The shared native Yukimi image was captured on 2026-10-28 by
[`yukimi-tatami`](../nix/yukimi-discover.nix), a disposable component-test VM
using the shipped Tatami module with test-only automatic login. It is the
unaltered `native-overview.png` capture, with software-rendered graphics and
the VM's real configuration and store. The [comparison report](test-results/yukimi-unified-2026-10-27.json)
also records the scaling and Xfce captures under `test-results/yukimi-unified/`.
These checks do not qualify an ISO or an installed physical system.

The other Tatami, Plasma and GNOME images come from [nix/screenshots.nix](../nix/screenshots.nix):

```sh
nix build .#screenshots --out-link result-screenshots
```

The test boots three disposable NixOS VMs in turn. Each is a Sekka system built
from the same desktop module installed systems import, with Tatami, Plasma or
GNOME selected, and logs in automatically. The virtual display is set to scale
2 with the desktop's own tool (Tatami's Displays apply, KDE's
`kscreen-doctor`, GNOME's `gdctl`), so its layout is drawn at twice the
density: 2560 × 1600 for Plasma and GNOME, a 1280 × 800 layout, and
2880 × 1800 for Tatami, a 1440 × 900 layout with room for the Displays window
under the bar. There is no GPU, so Mesa renders in software. Tatami is driven
with the keys a person would press; before each capture it presses Shift,
which hides the pointer until it moves. GNOME Shell's first-login welcome dialog is
dismissed with Escape. Captures are the virtual display at its native
resolution; the test keeps each whole capture an application was cropped
from as `NAME-display.png`, and crops with ImageMagick at the window's
rectangle as the compositor reports it (Hyprland's `hyprctl clients`), doubled
for the scale. Tatami's windows are a little translucent, so the wallpaper
shows faintly through them. No installation, update, or garbage collection is
invoked.
Yukimi shows the real test VM's limited configuration and store, including
unavailable freshness information. The Network window reads the VM's real
NetworkManager, but its Wi-Fi is simulated (mac80211_hwsim, with hostapd and
a DHCP server): Home, Café Corner, Riverside Library and Studio 5G are
made-up names, and the saved networks and WireGuard VPN were set up by the
test. Displays reads the VM's real virtual display, which QEMU describes as a
34-inch Red QEMU Monitor. Activity reads the VM's
real processes. Hyprland's share of the processor is Mesa drawing in
software, and the VM reports no processor speed or temperature, which show
as dashes; so do the memory maps of root's processes, which a user can't
read. None of these is a screenshot of an installed Sekka release, and
automatic login is set only in the test.

Older captures under [images/tour](images/tour) were imported byte-for-byte from
[determinate-nixos-graphical at e483b80](https://github.com/bitemyapp/determinate-nixos-graphical/tree/e483b80b3279c2baeece5fa1fe4dbf00a1932bcb/docs/images/tour)
at 1280 × 800. They illustrate the predecessor image, remain for the archived
documentation that links them, and do not certify the monorepo's image.

The Aizome images come from [nix/screenshots-aizome.nix](../nix/screenshots-aizome.nix):

```sh
nix build .#screenshots-aizome --out-link result-screenshots-aizome
```

It boots one disposable NixOS VM: a Sekka system built from the same desktop
module installed systems import, with Aizome chosen, logged in automatically
as the demo user. The 2880 × 1800 virtual display is set to scale 2 through
Aizome's own display settings path (`aizomectl output`), a 1440 × 900 layout.
There is no GPU, so Aizome composites in software, its pixman fallback. It is
driven through the compositor's own input path (`aizomectl input`, `spawn`,
`stack`, `resize`), as keys and clicks a person would make; captures are the
virtual display at its native resolution, and the three application images are
crops of it to the window with its tab and shadow, unscaled. Genkan is the
debug build's welcome preview (`CALAMARES_UI_PREVIEW=welcome`), with the
pointer moved off its window; its sample data are fixed, and production builds
leave the preview out.

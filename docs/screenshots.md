# Screenshot provenance

README and website images are original PNG captures with links to the full
files. They are not upscaled, composited, or generated illustrations.

| Image | Original resolution | What it shows |
| --- | --- | --- |
| [Genkan](images/sekka/genkan.png) | 2560 × 1600 | Current GTK interface in its debug-only welcome preview, with synthetic installation/cache data, on Sekka's Plasma session |
| [Tatami](images/sekka/tatami.png) | 2560 × 1600 | Tatami's Activity window on its CPU view, opened with Super+Ctrl+T over a terminal (Super+Return) running Tatami's fastfetch, with that terminal selected, after a minute of readings |
| [Tatami menu](images/sekka/tatami-menu.png) | 2560 × 1600 | The Tatami menu, opened with Super+Space over Activity and the terminal |
| [Yukimi](images/sekka/yukimi.png) | 2560 × 1600 | Current application in Tatami, reading the test VM `sekka-demo` |
| [Plasma](images/sekka/plasma.png) | 2560 × 1600 | Sekka's Plasma session with Konsole running fastfetch over Dolphin |
| [GNOME](images/sekka/gnome.png) | 2560 × 1600 | Sekka's GNOME session with Files over Yukimi |
| [Network](images/sekka/network.png) | 2560 × 1600 | Tatami's Network window, joined to a simulated network, with made-up network names |
| [Activity](images/sekka/activity.png) | 2560 × 1600 | The same Activity window on its Memory view (Ctrl+2): each application's proportional, private, resident and shared memory, and memory use and pressure under the list |

They come from [nix/screenshots.nix](../nix/screenshots.nix):

```sh
nix build .#screenshots --out-link result-screenshots
```

The test boots three disposable NixOS VMs in turn. Each is a Sekka system built
from the same desktop module installed systems import, with Tatami, Plasma or
GNOME selected, and logs in automatically. The virtual display is 2560 × 1600,
set to scale 2 with the desktop's own tool (Tatami's Displays apply, KDE's
`kscreen-doctor`, GNOME's `gdctl`), so a 1280 × 800 layout is drawn at twice
the density; there is no GPU, so Mesa renders in software. Tatami is driven
with the keys a person would press. GNOME Shell's first-login welcome dialog is
dismissed with Escape. Captures are the virtual display at its native
resolution. No installation, update, or garbage collection is invoked.
Genkan's preview uses fixed sample data and is excluded from production builds.
Yukimi shows the real test VM's limited configuration and store, including
unavailable freshness information. The Network window reads the VM's real
NetworkManager, but its Wi-Fi is simulated (mac80211_hwsim, with hostapd and
a DHCP server): Home, Café Corner, Riverside Library and Studio 5G are
made-up names, and the saved networks and WireGuard VPN were set up by the
test. Activity reads the VM's real processes and is opened with
Super+Ctrl+T. Hyprland's share of the processor is Mesa drawing in software,
most of all while the menu animates, and the VM reports no processor speed or
temperature, which show as dashes; so do the memory maps of root's processes,
which a user can't read. None of these is a
screenshot of an installed Sekka release, and automatic login is set only in
the test.

Older captures under [images/tour](images/tour) were imported byte-for-byte from
[determinate-nixos-graphical at e483b80](https://github.com/bitemyapp/determinate-nixos-graphical/tree/e483b80b3279c2baeece5fa1fe4dbf00a1932bcb/docs/images/tour)
at 1280 × 800. They illustrate the predecessor image, remain for the archived
documentation that links them, and do not certify the monorepo's image.

# Tatami

Tatami is a keyboard-driven [Hyprland](https://hypr.land) desktop for NixOS,
inspired by [Omarchy](https://github.com/basecamp/omarchy) 4.0.4 ("Quattro"):
its look, behavior and key bindings, adapted for NixOS without home-manager.
It is not Omarchy and is not affiliated with Omarchy or Basecamp.

Omarchy 4 draws its bar, menus, notifications, OSD and lock screen with a
Quickshell (QML/JavaScript) shell driven by Bash. Tatami reproduces that design
with Waybar, Walker, Mako, SwayOSD and hyprlock, styled to match, and replaces
the scripts with one Rust program, `tatami`.

Tatami is part of [Sekka](../../README.md), installed through Genkan.
It also works independently in any NixOS flake through Sekka's Tatami module.

## Using it

```nix
{
  inputs.nixpkgs.url = "https://flakehub.com/f/NixOS/nixpkgs/0.1";
  inputs.sekka = {
    url = "github:bitemyapp/sekka/nightly";
    inputs.nixpkgs.follows = "nixpkgs";
  };
  outputs = { nixpkgs, sekka, ... }: {
    nixosConfigurations.example = nixpkgs.lib.nixosSystem {
      system = "x86_64-linux";
      modules = [
        sekka.nixosModules.tatami
        { programs.tatami.enable = true; }
        ./configuration.nix
      ];
    };
  };
}
```

`programs.tatami.enable` adds a **Tatami** session to the login screen and
enables Hyprland with [uwsm](https://github.com/Vladimir-csp/uwsm), which it
needs. Tatami needs a display manager that offers Wayland sessions, such as
Plasma Login Manager, SDDM or GDM, and NetworkManager for its Wi-Fi panel.

`nix flake update sekka` and a rebuild bring Tatami's updates. Development
happens on topic branches merged into `main`; releases use version tags.

## What it is

| Component | Configuration |
|---|---|
| Look | Tokyo Night throughout, JetBrainsMono Nerd Font, the Adwaita cursor. Window gaps 5/10, a 2px `#7aa2f7` border, square corners, no blur or shadow, v4 animations, inactive windows at 0.985/0.96 opacity. Da Nang at night as the background, one of 23 wallpapers chosen with Style › Background or Super+Ctrl+Space (Omarchy's own have unclear licenses) |
| Bar | Waybar, 26px. NixOS logo (menu), workspaces 1–5, a centered "Monday 10:23" clock with screen-recording, night-light, do-not-disturb and stay-awake indicators, the month on hover and the weather beside it, then the tray (only while some application has an icon), Bluetooth, network, audio, display and battery, with the time left to empty or full on hover (right-click keeps it in the bar beside the percentage) |
| Menu and launcher | Walker with Elephant. The v4 menu tree (Apps, Learn, Trigger, Setup, About, System) in v4's card style, with chevrons on submenus. Typing searches menu entries and applications together. Keybindings (Super+K) are listed in v4's order and format |
| Notifications and OSD | Mako as v4's 380px accent-bordered cards; SwayOSD as v4's bottom card |
| Lock and idle | hyprlock with PAM in v4's style; hypridle locks and turns off displays |
| Authentication | hyprpolkitagent in Tokyo Night, centered with the screen dimmed around it |
| Panels | The bar's icons drop panels from the top right, under the bar, as v4's do. Network: the wired connection, Wi-Fi networks by signal with a password prompt for new ones, the Wi-Fi switch, the DNS provider (DHCP, Cloudflare, Google or custom servers) and Network settings…, the Network window (below), for VPNs, enterprise and hidden networks and each connection's settings. It opens at once with the networks NetworkManager has already seen, and asks for a fresh scan for its next opening. Power profile. The display icon opens the Displays window (below). Audio is wiremix and Bluetooth is bluetui (it pairs devices that ask for a passkey), in the same place; a second click closes them |
| Terminal and tools | foot with v4's configuration, the bottom system monitor (Super+Ctrl+T), fastfetch, Nautilus, imv, mpv |
| GTK applications | Dark Adwaita with Yaru icons, from a dconf profile selected only in this session |

The configuration lives in `/etc/xdg/tatami` and is used only by this
session. The session puts that directory first in `XDG_CONFIG_DIRS` and
starts Hyprland with `--config`, so other desktops on the same system are
unaffected: a file in the session's runtime directory that loads
`/etc/xdg/tatami/hypr/hyprland.lua`, because Hyprland re-reads the file it
started with on every reload, and the `/etc` path would pin the store copy
the session began with. Its components are systemd user units wanted by the
session's own target, so they never start elsewhere.
`config/hypr/hyprland.lua` contains only declarative `hl.*` calls: v4's
window rules and key bindings, minus Arch-specific ones. It also loads the
display settings chosen in the Displays window and with Super+/, then the
session's monitor state, which the helper writes before every reload and as
it turns a display off, so a reload turns no monitor on or off. Both Shift keys together toggle Caps Lock. The module
appends the system's keyboard layout (`services.xserver.xkb`); non-Latin
layouts follow `us`, switched with both Alt keys.

The `tatami` helper (`tool/`) provides:

- the session launcher, and `tatami reload` (System › Reload, or Super+R), which applies
  a rebuilt Tatami to the running session
- the menu: system (lock, suspend, hibernate when swap exists, reload,
  logout, restart, shutdown), capture, toggles, hardware, audio, Wi-Fi,
  Bluetooth and power profile; the power key opens System, as in Omarchy,
  and Setup › Monitors the Displays window
- the network panel; Wi-Fi passwords reach NetworkManager in a file only the
  user can read, never on a command line, and are stored root-only; DNS
  changes apply to the connection in use without reconnecting
- key binding help
- copy and paste that works in terminals (Super+C/V/X)
- window commands: pop out, tiled fullscreen, transparency, gaps, square
  aspect, workspace layout. Gaps, aspect, layouts and a disabled touchpad
  are re-applied at login
- monitor scaling, laptop display, touchpad and zoom toggles, and keeping
  and applying the Displays window's settings (`tatami displays apply`)
- time, battery and keyboard backlight notifications
- a terminal in the active window's directory, editor and browser launch
- volume, microphone, brightness and media keys with OSD
- the background picker, whose global choice the lock screen also shows, and
  Style › Background settings for scaling, color and per-display wallpapers
- the weather beside the clock: the conditions' icon, and on hover the
  temperature, place, feels-like temperature, wind, humidity and the next
  three days, from wttr.in (which finds the place from the IP address unless
  Setup › Weather sets one) and Open-Meteo, as in Omarchy. The bar shows the
  last report at once and fetches a new one apart from it every 15 minutes,
  trying again while the network comes up; a click fetches at once, the icon
  dimmed until it is in. The place found from the IP address is kept for an
  hour
- screenshots (freeze, region or window, copy, notify, Satty editing)
- screen recordings of a region or monitor, with desktop audio and the
  microphone, by gpu-screen-recorder, or wf-recorder where it cannot run
  (no hardware OpenGL); text (OCR) and QR codes from a region
- lock and wake, and logout, reboot and shutdown after closing windows

Applications started from the menu, launcher or key bindings run as systemd
services of their own, with the environment they were started from: a
memory-pressure kill of one cannot end the session, and since the service
manager starts them, restarting the launcher cannot end them either.

With other desktops installed, Tatami keeps their pieces out: their tray
applets (nm-applet, the printer applet) do not start in it, and the session
reports `XDG_CURRENT_DESKTOP=Hyprland:Tatami` so such entries can tell it
apart. Setup › Hyprland creates `~/.config/tatami/hyprland.lua`, which loads
Tatami's defaults first and then your own settings, so later updates of the
defaults still apply.

Not ported from Omarchy:

- Arch package installation and updates, Limine/snapper, Plymouth theming and
  the first-run script.
- Theme and font switching, web apps, the webcam overlay, sharing,
  reminders, dictation and AI agents.
- Setup's Plugins, Security, Defaults, Direct Boot and Reset entries, and the
  Install, Remove, Update and Style menus.
- The shell's own panels: the calendar is Waybar's, audio and Bluetooth are
  the wiremix and bluetui terminal programs, and there is no notification
  history panel or network statistics.

## Known issues

- **Claude Desktop closes on `tatami reload` if an older Tatami started
  it.** The launcher used to start applications as its own children
  (`systemd-run --scope`), and Claude Desktop's NixOS sandbox
  (`bwrap --die-with-parent`) is killed, without a crash report, when its
  parent exits: restarting the launcher ended it, and any other application
  in such a sandbox. Applications are now services of their own; one started
  before the update closes on the first reload, and survives once started
  again.
- **Hyprland keeps the configuration file it started with.** It resolves
  the path once, so a session started by an older Tatami (on the `/etc`
  symlink), or with a `~/.config/tatami/hyprland.lua` that is itself a
  symlink into the Nix store (home-manager), re-reads the configuration it
  began with. `tatami reload` then leaves Hyprland's configuration alone,
  and it applies from the next login.
- **Hyprland 0.56.2 trusts clients' shared memory.** It copies screen
  frames into a client's buffer without libwayland's SIGBUS guard, so a
  buffer whose memory is gone crashes the compositor. hyprpicker, which
  freezes the screen for screenshots, keeps its frames as files in the
  runtime directory and leaves them there when killed: they filled
  `/run/user`, and the next screen copy into a file there crashed Hyprland.
  The helper now gives each freeze a runtime directory of its own and
  removes it, with any frames earlier freezes left behind.
- **Hyprland 0.56.2 ignores `hyprctl reload config-only`.** Every reload
  applies the monitor rules again, so Tatami keeps the session's monitor
  state for the configuration to load; a monitor switched off or rescaled
  by other means than Tatami is turned back by the next reload.
- **No lid handling.** The configuration turns every display on at login,
  so with the lid closed the laptop display is on, out of sight, and can
  hold workspace 1. Super+Ctrl+Delete turns it off for the session; when
  it comes back on, Hyprland moves the workspaces it had back to it.
- **Hyprland's variables reach only what key bindings start.** The
  `hl.env` settings in `hyprland.lua` (`ELECTRON_OZONE_PLATFORM_HINT`,
  `GDK_BACKEND`, `QT_QPA_PLATFORM`…) are Hyprland's environment, not the
  user service manager's, so applications started from the launcher do not
  get them. Variables every application needs go in `env-tatami` instead,
  as `NIXOS_OZONE_WL` does, and take a new login.
- **ChatGPT desktop (nixpkgs) aborts in "Use Qt" mode.** Its wrapper unsets
  `QT_PLUGIN_PATH`, so Qt finds no platform plugin; keep its appearance on
  GTK. It also ignores `ELECTRON_OZONE_PLATFORM_HINT`: `NIXOS_OZONE_WL`
  is what puts it on Wayland at the display's scale.

## Displays

The Displays window (`displays/`, `tatami-displays`, GTK 4 and libadwaita)
is the bar's display icon, Super+Ctrl+D, Setup › Monitors and "Displays" in
the launcher, after macOS's Displays settings:

- the arrangement, to scale: drag a display and it goes beside the others,
  sharing an edge and lining up with theirs. The main display, where the
  pointer starts and workspace 1 opens, has a menu bar
- for each display: use as main display, extended display, mirror of
  another, or off (for the session, as Super+Ctrl+Delete); resolution as
  "looks like" sizes from larger text to more space, or every resolution
  and scale; refresh rate; variable refresh rate (off, on, full-screen
  windows, full-screen games and video); rotation; HDR, with the brightness
  and saturation of SDR content; color profile; 10-bit color; and the
  built-in display's brightness
- Night Shift: on or off, and its color temperature, which the nightlight
  toggle then uses too

Changes apply at once. Those that can leave a screen blank (resolution,
refresh rate, variable refresh rate, rotation, HDR, 10-bit color, mirroring
and turning a display off) ask to be kept and are undone after 15 seconds
otherwise. The displays to the right of and below one that changes size move
with its edges, and every display a change moves moves in one step.

The `tatami` helper keeps the settings, as Super+/'s scales, in
`~/.local/state/tatami/displays.json`, and writes the `hl.monitor` rules
Hyprland loads from them to `displays.lua` next to it. A display's settings
are kept under its make, model and serial (Hyprland's `desc:`), so they
follow it to another port or dock, or under its connector when two displays
describe themselves the same. With `TATAMI_DISPLAYS_DRY_RUN=1` the window
prints what it would apply instead.

It has Tatami's look, Tokyo Night in JetBrainsMono Nerd Font with square
corners, from `config/tatami-displays/style-dark.css`
(`/etc/xdg/tatami/tatami-displays/`), which imports the colors, font and
corners Tatami's windows share from `config/tatami-gtk/style-dark.css`:
libadwaita's stylesheet with Tatami's colors, so the layout is
libadwaita's. The window takes the first
`tatami-displays/style-dark.css` or `style.css` in `~/.config` and then
`XDG_CONFIG_DIRS`, so one of your own comes first; a `-dark` one makes it
dark whatever the light or dark preference. Outside Tatami, without one, it
is plain libadwaita.

## Network

The Network window (`network/`, `tatami-network`, GTK 4 and libadwaita) is
Network settings… in the network panel and in Setup › Network, and
"Network" in the launcher, after macOS's Network settings:

- Wi-Fi: the switch, the network in use, the known and other networks in
  range by signal, and joining one by name (a hidden network). Joining asks
  for what the network needs: a password, or an organization's username and
  password and how to check its server (PEAP or TTLS, with the system's
  certificate authorities and the server's domain). A saved password the
  network refuses is asked for again
- wired connections; VPNs, each with its switch; adding a WireGuard VPN from
  its configuration file
- the saved Wi-Fi networks, in range or not
- for each connection: its addresses while in use; joining automatically;
  metered; on Wi-Fi, a private hardware address and its password; IPv4
  automatically or a manual address and router; DNS (automatic, Cloudflare,
  Google or your own servers); IPv6 on or off; forgetting it

Changes apply at once: to the connection in use without reconnecting where
NetworkManager can (IPv4, DNS, IPv6), otherwise the next time it connects.
A change NetworkManager refuses is undone on the page too. NetworkManager
does the work, through `nmcli`: the window reads it on a worker thread when
NetworkManager reports a change and every few seconds while open, asks for a
scan now and then, and never waits for one. Passwords reach `nmcli` in a
file only you can read, never on a command line. `tatami-network --join
NAME` opens it to join that network, as the panel does for enterprise
networks. With `TATAMI_NETWORK_DRY_RUN=1` it prints the changes it would
make instead.

It looks as Displays does, from `config/tatami-network/style-dark.css`,
found the same way under `tatami-network/`.

It does not offer WEP networks, which anyone nearby can break, or
enterprise networks that sign in with a certificate (EAP-TLS) rather than a
password. VPNs other than WireGuard need NetworkManager's plugin for them,
which Sekka does not install; a VPN set up elsewhere still shows, with its
switch. Proxies, bridges, bonds, VLANs and mobile broadband are left to
`nmcli` and `nmtui`.

## Wallpapers

`wallpapers/` has the backgrounds, with their credits and licenses in
[wallpapers/README.md](wallpapers/README.md): photographs from Unsplash under
the Unsplash License, and a Hubble image under CC BY 4.0. They are not under
Tatami's license. The package `wallpapers` (also the read-only option
`programs.tatami.wallpapers`) lays them out for every desktop from one copy of
each image: for Plasma's, GNOME's and Xfce's wallpaper settings as well as
Tatami's own picker. Other desktops' modules can install it to offer them.

## Development

`nix flake check` builds the helper and the Displays and Network windows
(with their tests) and evaluates an example system with the module. They
are Cargo crates in `tool/`, `displays/` and `network/`; `nix develop` has
Cargo and the libraries they build with, and `cargo test` in any of them
runs its tests on Linux.

To try a checkout in a running Tatami session, switch the system to it from
the repository and reload the session, without logging out:

```sh
nixos-rebuild switch --sudo --flake /etc/nixos --override-input tatami "git+file://$PWD" --no-write-lock-file
tatami reload
```

`/etc/nixos` is where the graphical installer puts the system's flake. The
override builds the checkout's tracked files, committed or not (`git add`
new ones), and the system's `flake.lock` keeps its revision, so the next
ordinary rebuild goes back to it. `tatami reload` (System › Reload in the
menu) reloads Hyprland's configuration without turning any monitor on or
off, re-applies the remembered toggles, and restarts the session's running
units (bar, launcher, notifications…) from the new configuration. Those are
the units the module lists in `/etc/xdg/tatami/units.json`; a unit started by
hand is left alone, even one named `tatami-…`. A session
started by an older Tatami, whose Hyprland can only re-read the store copy
it began with, keeps its Hyprland configuration until the next login, and
`tatami reload` restarts the units only. Changes to the session's
environment (`env-tatami`) take a new login.

New code is Rust; configuration stays declarative (Nix, TOML, JSONC, CSS and
`hl.*` calls in `hyprland.lua`).

## Wallpaper settings

Use **Style › Background** or **Super+Ctrl+Space** to choose a wallpaper.
**Style › Background settings** controls the image, scaling mode and background
color for all displays or a particular display. The original image files stay
intact; the display handles scaling and cropping.

| Mode | Result |
| --- | --- |
| Fill (default) | Fill the screen without distortion, cropping excess edges |
| Fit | Show the entire image without distortion, with the chosen color around it |
| Stretch | Fill the screen by changing the image's proportions |
| Center | Center at original size, cropping if larger than the display |
| Tile | Repeat at original size |
| Solid color | Hide the image and use the chosen color |

Display overrides inherit any settings you have not overridden from the global
defaults, including the wallpaper. They survive logout, rebuilds and reconnecting
the display. Choosing another image does not reset its scaling mode or color.
“Use defaults for this display” removes its overrides. The global reset clears
all appearance overrides and restores Fill, keeping your global wallpaper choice.

The same controls are available from the terminal:

```sh
tatami background settings
tatami background mode fit
tatami background color '#1a1b26'
tatami background mode fill --output DP-1
tatami background set /absolute/path/to/portrait.jpg --output DP-1
tatami background color '#112233' --output DP-1
tatami background reset --output DP-1
tatami background status
```

CLI mode names are `fill`, `fit`, `stretch`, `center`, `tile`, and `solid_color`.
Settings are saved in `$XDG_STATE_HOME/tatami/background.json` (normally
`~/.local/state/tatami/background.json`); the existing `background` symlink keeps
the global image choice compatible with older Tatami versions. The lock screen
continues to use that global image with its own blurred presentation.

A missing per-display image falls back to the global image when the wallpaper
service starts. Once the file is available again, run `tatami reload` to restore
it. An invalid settings file is preserved while the service uses defaults;
`tatami background reset` recovers it explicitly.

## License

Tatami is licensed under either of

- the [Apache License, Version 2.0](LICENSE-APACHE), or
- the [MIT License](LICENSE-MIT),

at your option. The files under `config/` are adapted from Omarchy, which is
MIT-licensed; its copyright notice is in [config/LICENSE](config/LICENSE).

Unless you state otherwise, any contribution you intentionally submit for
inclusion in Tatami, as defined in the Apache-2.0 license, is dual licensed
as above, without additional terms or conditions.

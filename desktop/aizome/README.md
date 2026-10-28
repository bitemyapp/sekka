# Aizome 藍染

*Aizome* is indigo dyeing: cloth dipped again and again, each dip adding
depth. It is also Sekka's desktop: a Wayland desktop environment written in
Rust on Sukumo 蒅, its native toolkit, from the compositor to the calculator.

- **Haiku's tabs.** Every window wears a tab only as wide as its title,
  amber when focused. Tabs slide along the top edge, and dropping one tab on
  another stacks the windows into a group, from any apps.
- **NeXT's menus.** One menu bar for the focused app, and the whole of its
  menu at the pointer on ❖-right-click. Any menu tears off into a palette.
- **IRIX's Toolchest and Desks**, and a Deskbar from Haiku: every app with
  its windows, its notifications filed beneath it, and the Shelf, where a
  drag can be parked halfway.
- **A command palette for everything** (❖Space): every menu command of the
  focused app, with its shortcut taught back to you, plus windows, files,
  apps, settings and arithmetic.
- **Super is the command key** (❖C, ❖V, ❖Z), in every app.
- **One file chooser**: apps that open and save through the desktop
  portal (browsers, Electron apps, Zed, Flatpaks) get Files in miniature,
  over their window, with their filters and options.
- **Fast by construction**: integer scaling, opaque surfaces, damage-only
  redraws, and a 4 ms budget for every frame of the desktop and its apps; GPU
  compositing with a software fallback.

The design lives in [aizome-design](https://github.com/bitemyapp/aizome-design).

## Apps

Settei (Settings), Yukimi (Software), Hako (Files), Sumi (Text Editor),
Fusen (Notes), Koyomi (Calendar), Tokei (Clock), Soroban (Calculator), Tenki
(Weather), Tansu (Disks), Kura (Disk Usage), Myaku (Activity) and Koe
(Dictation), with foot as the terminal. See [docs/apps.md](docs/apps.md).

## Using it

On Sekka, choose Aizome in the installer (it is the default), or add it to
an installed system:

```nix
calamares.desktops = [ "aizome" /* and any others */ ];
calamares.defaultDesktop = "aizome";
```

On any NixOS system with flakes, from Sekka's flake:

```nix
{
  inputs.sekka.url = "github:bitemyapp/sekka";
  outputs = { nixpkgs, sekka, ... }: {
    nixosConfigurations.example = nixpkgs.lib.nixosSystem {
      modules = [
        sekka.nixosModules.aizome
        { programs.aizome.enable = true; }
      ];
    };
  };
}
```

Then pick **Aizome** on the login screen.

Games with desktop shortcuts appear in **Toolchest → Applications → Games**.
The GPU compositor supports explicit synchronization, including through
Xwayland, and adaptive sync for fullscreen windows on capable displays.
Choose Off, Fullscreen or Always under **Settings → Displays**; changes
revert after 15 seconds unless kept. Turn adaptive sync off if the display
flickers. Software and nested outputs do not offer adaptive sync.

Aizome's display pipeline is SDR and synchronized to vertical refresh; it
does not yet implement native HDR or tearing control. Sekka's separate
Steam login session uses Gamescope for those features. See
[Playing games](../../README.md#playing-games) for Steam launch options.

## Developing

Aizome lives in Sekka's repository, in `desktop/aizome`; Sekka's flake
builds it, and its changes are Sekka pull requests. From the repository root:

```sh
nix develop .#aizome
cd desktop/aizome
cargo build
```

Sukumo, the toolkit, is in `sukumo/`: a workspace of its own, whose crates
this one takes by path ([its README](sukumo/README.md)).

To try one app from the current checkout or worktree in your running Aizome
session:

```sh
just replace yukimi
```

Recipes automatically enter Aizome's development shell when needed.
`just` itself must
be on your PATH; it is also included in `nix develop .#aizome`, so if it is not
installed yet, use `nix develop .#aizome -c just replace yukimi`.
Substitute any app name, such as
`hako` or `sumi`. This builds that app, asks its existing windows to close,
and starts the new binary. A failed build leaves the running app alone;
an app asking to save is not forcibly stopped. Future launches still use
the installed app; run the command again to test the checkout version.
See [live reload](docs/reload.md) for watching changes on every save.

Run the whole desktop without touching your own session, headless:

```sh
scripts/headless-session --no-shell --bin target/debug -- aizomectl screenshot desktop.png
```

`nix build .#aizome-checks.session` logs into Aizome in a VM.

- [docs/architecture.md](docs/architecture.md): the parts and their contracts.
- [docs/shell.md](docs/shell.md): what the shell draws and does.
- [docs/kit.md](docs/kit.md): writing apps with the kit.
- [docs/ipc.md](docs/ipc.md): the compositor's IPC.
- [docs/performance.md](docs/performance.md): the frame budget and its measurements.

## Licence

MIT or Apache-2.0, at your option. Fonts (Archivo, IBM Plex Mono, Noto) are
under the SIL Open Font License; icons from Lucide under the ISC licence.

# Screenshot provenance

README images are original PNG captures with links to the full files. They
are not upscaled, composited, or generated illustrations.

| Image | Original resolution | What it shows |
| --- | --- | --- |
| [Genkan](images/sekka/genkan.png) | 1920 × 1080 | Current GTK interface in its debug-only welcome preview, with synthetic installation/cache data |
| [Yukimi](images/sekka/yukimi.png) | 1920 × 1080 | Current application reading a disposable NixOS test VM named `sekka-demo` |
| [Tatami menu](images/tour/installed-tatami-menu.png) | 1280 × 800 | Installed desktop captured from the predecessor image |
| [Plasma](images/tour/installed-plasma.png) | 1280 × 800 | Installed desktop captured from the predecessor image |
| [GNOME](images/tour/installed-gnome.png) | 1280 × 800 | Installed desktop captured from the predecessor image |

The current app captures come from [nix/screenshots.nix](../nix/screenshots.nix):

```sh
nix build .#screenshots --out-link result-screenshots
```

The test boots a disposable NixOS VM, renders the actual GTK applications under
IceWM with Cairo software rendering, and saves the virtual display at its
native resolution. The test window manager maximizes the apps so the entire
capture is readable. No installation, update, or garbage collection is invoked.
Genkan's preview uses fixed sample data and is excluded from production builds.
Yukimi shows the real test VM's limited configuration and store, including
unavailable freshness information; it is not a screenshot of an installed
Sekka release.

The desktop images were imported byte-for-byte from
[determinate-nixos-graphical at e483b80](https://github.com/bitemyapp/determinate-nixos-graphical/tree/e483b80b3279c2baeece5fa1fe4dbf00a1932bcb/docs/images/tour).
They illustrate the desktop experience and do not certify the monorepo's image.

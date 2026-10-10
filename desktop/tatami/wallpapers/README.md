# Wallpapers

Photographs and renders offered as backgrounds by Tatami and, on systems from
the NixOS graphical installer, by every desktop. All 23 wallpapers retain
their original published pixel dimensions and aspect ratios. The JPEGs are
the unmodified downloads: Unsplash's `raw` image URLs with no resize, crop,
format or quality parameters, and ESA/Hubble's full-resolution JPEG for the
Lagoon Nebula (4782×6028). Nothing is upscaled or re-encoded.

`wallpapers.json` records each source page, exact download URL, original
dimensions and SHA-256 checksum. `package.nix` links every desktop to those
same original files. `thumbnails/` contains separate 480×300 picker previews;
those are never used as desktop backgrounds.

Cropping happens on the display, preserving the image's proportions:

| Desktop | Display mode |
| --- | --- |
| Tatami | `fill` by default; configurable in Style › Background settings, including per-display overrides |
| Plasma | Image plugin's `FillMode=2` (`PreserveAspectCrop`, “Scaled and Cropped”) default |
| GNOME | Explicit `picture-options='zoom'`, plus `zoom` in the wallpaper catalog |
| Xfce | xfdesktop's `XFCE_BACKDROP_IMAGE_ZOOMED` default, separately for each monitor |

These fill the screen without stretching, cropping only the excess at its
edges when the image and display have different aspect ratios. User-selected
display modes remain configurable in the desktop's wallpaper settings. Tatami's
[wallpaper controls](../README.md#wallpaper-settings) also offer fit, stretch,
center, tile and solid color, with a configurable background color.

Run `nix build .#checks.x86_64-linux.wallpapers` to verify the originals'
checksums and dimensions, the installed desktop links, and the GNOME catalog.
When adding an image, preserve the full downloaded file and record its source
dimensions and checksum; never resize it to a particular monitor's shape.

They are not covered by Tatami's MIT OR Apache-2.0 license:

- Photos from Unsplash are under the [Unsplash License](https://unsplash.com/license):
  free to use, modify and redistribute, without attribution, but not to sell
  unaltered or to compile into a competing service. Credit is given anyway.
- The Lagoon Nebula is © ESA/Hubble, released under
  [CC BY 4.0](https://creativecommons.org/licenses/by/4.0/). Credit: NASA,
  ESA, and STScI; [heic1808a](https://esahubble.org/images/heic1808a/).

| File | Title | By | License |
|---|---|---|---|
| `blue-to-red.jpg` | [Blue to red](https://unsplash.com/photos/6TnLg-TkW9I) | Marek Piwnicki | [Unsplash License](https://unsplash.com/license) |
| `lost-in-the-light.jpg` | [Lost in the light](https://unsplash.com/photos/LI748t0BK8w) | Alexander Slattery | [Unsplash License](https://unsplash.com/license) |
| `tokyo-at-dusk.jpg` | [Tokyo at dusk](https://unsplash.com/photos/jR_djoNFLw0) | Wenhao Ruan | [Unsplash License](https://unsplash.com/license) |
| `da-nang-at-night.jpg` | [Da Nang at night](https://unsplash.com/photos/XpGORWJWPi8) | Ngô Thanh Tùng | [Unsplash License](https://unsplash.com/license) |
| `milky-way.jpg` | [Milky Way](https://unsplash.com/photos/Knwea-mLGAg) | Felix Mittermeier | [Unsplash License](https://unsplash.com/license) |
| `looks-flat.jpg` | [Looks flat](https://unsplash.com/photos/vO7jeSQhnMo) | Marek Piwnicki | [Unsplash License](https://unsplash.com/license) |
| `turbacz-after-sunset.jpg` | [Turbacz after sunset](https://unsplash.com/photos/5bWUAEY30QE) | Cezary Kukowka | [Unsplash License](https://unsplash.com/license) |
| `burn.jpg` | [Burn](https://unsplash.com/photos/Z0I2iGp4Fw4) | Marek Piwnicki | [Unsplash License](https://unsplash.com/license) |
| `manhattan-at-night.jpg` | [Manhattan at night](https://unsplash.com/photos/xygKeG0UZJw) | Brad Smith | [Unsplash License](https://unsplash.com/license) |
| `chicago-at-dusk.jpg` | [Chicago at dusk](https://unsplash.com/photos/RQfSIqLuTHs) | Willian Justen de Vasconcellos | [Unsplash License](https://unsplash.com/license) |
| `kala-patthar.jpg` | [Kala Patthar before dawn](https://unsplash.com/photos/B49iArnIXD0) | Theodor Lundqvist | [Unsplash License](https://unsplash.com/license) |
| `path-to-heaven.jpg` | [Path to heaven](https://unsplash.com/photos/HxPkohFqDGY) | Marek Piwnicki | [Unsplash License](https://unsplash.com/license) |
| `wake-up.jpg` | [Wake up](https://unsplash.com/photos/kMdf282tpX4) | Marek Piwnicki | [Unsplash License](https://unsplash.com/license) |
| `hintersee.jpg` | [Hintersee at sunset](https://unsplash.com/photos/-aTKERomQOg) | Johannes Plenio | [Unsplash License](https://unsplash.com/license) |
| `wind-tunnel.jpg` | [Wind tunnel turning vanes](https://unsplash.com/photos/b-P6xrDMFSU) | NASA | [Unsplash License](https://unsplash.com/license) |
| `bubble-nebula.jpg` | [Bubble Nebula](https://unsplash.com/photos/rTZW4f02zY8) | NASA | [Unsplash License](https://unsplash.com/license) |
| `lagoon-nebula.jpg` | [Lagoon Nebula](https://esahubble.org/images/heic1808a/) | NASA, ESA, and STScI | [CC BY 4.0](https://creativecommons.org/licenses/by/4.0/) |
| `isometric-building.jpg` | [Isometric building](https://unsplash.com/photos/IQi4jAlVeOI) | A Chosen Soul | [Unsplash License](https://unsplash.com/license) |
| `on-the-dunes.jpg` | [On the dunes](https://unsplash.com/photos/IyRJgzv499I) | Jonatan Pie | [Unsplash License](https://unsplash.com/license) |
| `ocean-wave.jpg` | [Blue ocean wave](https://unsplash.com/photos/5Dga0T0x6GY) | Austin Schmid | [Unsplash License](https://unsplash.com/license) |
| `moonlit-hoodoos.jpg` | [Sand mountain by moonlight](https://unsplash.com/photos/bgGr1eKlYNw) | John Fowler | [Unsplash License](https://unsplash.com/license) |
| `fishing-in-the-mist.jpg` | [Fishing in the mist](https://unsplash.com/photos/u1lBUAh5Wrw) | VietNam Beautiful | [Unsplash License](https://unsplash.com/license) |
| `foggy-forest.jpg` | [Trees and fog](https://unsplash.com/photos/l7Wb6FXHIOQ) | Ethan Dow | [Unsplash License](https://unsplash.com/license) |

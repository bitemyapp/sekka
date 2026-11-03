# Crates Sukumo carries with a change

## fontique

`fontique/` is [fontique](https://github.com/linebender/parley) 0.11.1 as
crates.io publishes it (archive SHA-256
`6688bc1294fe7117d788937b6c53480169b29c566954af490830d4c09da9516a`,
linebender/parley `eea3503d`), with one change, in
`fontique/SUKUMO-PATCH.patch`. `fontique/SUKUMO-PROVENANCE.json` holds the
hashes of the published files. Sukumo's, Aizome's and Genkan's window's
workspaces patch it in (`[patch.crates-io]`); their package sources include
this directory. Uta builds the kit in its own workspace with crates.io's
fontique, which gives the same faces and only starts slower.

### Why

fontique opens the system's font collection by asking fontconfig for its
eight generic families (`serif`, `sans-serif`, `monospace`, `cursive`,
`fantasy`, `system-ui`, `emoji`, `math`). Each is an `FcFontSort` of every
installed face, trimmed to the faces that add coverage, then an
`FcFontRenderPrepare` of each face it returns, to read its family. With
1,623 faces installed (a Ryzen 7 9800X3D desktop), that is 8 sorts
returning 143 to 188 faces each, about 11 ms on the main thread before a
kit app has connected to Wayland: most of its 19 ms to the first frame.
An app uses one or two of the eight.

### The change

- **Each generic family is found the first time it is asked for**, by the
  same code `SystemFonts::new` ran for all eight (the same pattern,
  substitution, trimmed sort, render-prepare and de-duplication), and kept
  for every clone of the collection (`System::generic_families`, a
  `OnceLock` a family). The other backends found theirs as they opened and
  give them out the same way.
- **A query expands a generic family when matching reaches it**, and finds
  the script's fallback families (and the Han family it always adds) when
  matching gets past the families asked for. The candidates and their order
  are the same; a query that stops at an earlier family no longer sorts
  every font for the later ones, nor asks fontconfig for the script's
  fallback.
- A generic family whose sort fails (fontconfig has no fonts) has no
  members, where `new` panicked. Sukumo scanned the font directories after
  that panic, and still does when the collection has no families.
- The copy allows the `deprecated` lint: parlance 0.1.1 deprecates a
  function this release calls 161 times, which Cargo shows for a path crate
  and hides for a crates.io one.

Kept as it was: the `FcFontRenderPrepare` of each sorted face (a user's
`<match target="font">` rule can rename a family, so skipping it could
change the result), and the untrimmed sort that finds a family's faces the
first time a family is used. fontconfig reads the locale on the first
substitution, which now comes at first use rather than as the collection
opens: the same, unless a program changes its locale variables in between,
which none of Sekka's does.

### Checking it

`crates/sidestep-engine/src/text/tests.rs`'s `fallback_faces_are_pinned`
lays fixture text in twelve scripts and kinds out in the system's roles
and in named families, and compares the face of every run with
`fixtures/fallback-pins.txt`, made with crates.io's fontique on the
Sukumo check's fonts. `SUKUMO_FALLBACK_PINS=FILE` (with
`SUKUMO_FALLBACK_GLYPHS=1` for every glyph and position) writes a
machine's report instead, for comparing two builds on any fonts.

### Moving it

On a new fontique release, extract it here, apply `SUKUMO-PATCH.patch`
(`patch -p1 -d fontique`), and record its archive hash and files in
`SUKUMO-PROVENANCE.json`; or drop the copy and the three `[patch]` entries
if upstream finds generic families lazily itself.

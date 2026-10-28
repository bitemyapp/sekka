# Shiori

Shiori 栞 (Documents) is Aizome's PDF reader; what it does is in
[apps.md](apps.md#shiori-documents). This is how it's put together, and
why PDFium draws its pages.

## Which engine

Licence first: Aizome is MIT OR Apache-2.0 and Sekka ships it, so no GPL
or AGPL engine. That rules out MuPDF (AGPL-3.0; the fastest and most
complete of them), Poppler (GPL-2.0-or-later, what Evince and Okular use),
xpdf and Ghostscript. pdf.js is Apache-2.0 but JavaScript, needing a web
engine. Parsing libraries such as lopdf don't draw pages. Two remain:

- **PDFium**, Chrome's PDF engine (BSD-3-Clause and Apache-2.0), through
  `pdfium-render` (MIT OR Apache-2.0). nixpkgs builds it from source (build
  8066) against the system's FreeType, libjpeg, OpenJPEG, lcms2 and ICU,
  without V8 or XFA; `libpdfium.so` is 4.8 MiB and cache.nixos.org has it.
- **hayro** (Apache-2.0 OR MIT), a renderer in Rust that forbids unsafe
  code, from Typst's PDF author.

Both drew the pdf.js test corpus (1,012 files, many of them broken on
purpose), the first five pages of each at 2× in a process per file. The
record, with the method and every number, is
[`docs/test-results/pdf-engines-2026-10-28.json`](../../../docs/test-results/pdf-engines-2026-10-28.json).

| | PDFium | hayro |
|---|---|---|
| Pages drawn | 1,193 | 1,204 |
| Per page, p50 / p90 / p99 | 4.5 / 18 / 161 ms | 4.8 / 22 / 203 ms |
| Slowest page | 1.8 s | 14.2 s |
| All 1,188 pages both drew | 16.4 s | 36.0 s |
| Stopped the process | 1 (its own check, on a JPEG 2000 image) | 1 (a panic, on a tiling pattern) |
| Over 30 s on a file | 2 (tiling patterns) | 1 |

On the median page they're as fast as each other (hayro is the faster on
55% of pages); PDFium's tail is much shorter. Of the 1,185 pages both drew
at one size, 900 differ on under 1% of pixels and 48 on over 5%. Looked
at by eye, those split both ways: PDFium is right where hayro lacks
knockout groups, a blend mode and transfer functions, and where a font
isn't embedded; hayro is right on a subsampled JPEG 2000 image PDFium
left out, colorimetric CalGray and a Type 3 glyph error after which
PDFium drew nothing. Most of the rest is antialiasing.

What decided it is what a reader needs besides drawing: PDFium gives each
character's box, search with the rectangles of each hit, links and their
destinations, the outline, forms, annotations, metadata and permissions.
With hayro each of those would be written on its parsing crates. hayro is
the one to watch: memory-safe, and already close.

## A worker per document

PDFium is a large C++ library reading files from anywhere, and it stops
its process on purpose when one of its checks fails (`jpx_numcomps.pdf`
does that). Shiori is one program with a window per document, so a bad
file in the same process would close every document. Each document has
its own worker instead, `aizome-shiori --pdfium-worker` (`worker.rs`),
started by `engine.rs` in the app:

- They speak over the worker's stdin and stdout (`protocol.rs`): frames of
  a length and JSON, with a drawing's RGBA after its frame. Geometry is in
  points of the page as it shows, from its top left; the worker turns
  PDF's upward, rotated, cropped coordinates into that, so the app never
  does.
- The worker says what it starts on. The engine's watchdog stops it after
  20 seconds on one drawing (10 on a page's text, links or search, 30
  opening); a worker that crashes or is stopped is reported with what it
  was doing, and another opens the document. The app marks that page
  "couldn't be drawn" (or that page's text empty, or that page searched)
  and asks for the rest again. After four failures in a minute it stops
  trying.
- The worker dies with the app (`PR_SET_PDEATHSIG`), can't gain
  privileges (`PR_SET_NO_NEW_PRIVS`) and may map at most 8 GiB.
- PDFium finds fonts a document names but doesn't carry, Japanese say, in
  fontconfig's folders, which the worker asks fontconfig for (NixOS keeps
  fonts in the store, not under `/usr/share/fonts` where PDFium looks).
- Running a worker per document also lets documents draw at the same time;
  PDFium allows one caller per process.

Over the pdf.js corpus through this path (`examples/corpus.rs`), 987 of
1,012 files opened, drew, gave their text and links and were searched in
44 ms at the median (200 ms p99); the two tiling-pattern files were
stopped at 20 seconds and the JPEG 2000 file crashed its worker, each
costing one page, and the rest were refused at once (passwords, files that
aren't PDFs) or have no pages PDFium can read.

Not done: a seccomp or Landlock sandbox for the worker. It reads only the
document it was started for, so the system calls it needs are few; that's
the next step for hostile files.

## Drawing

- **Tiles** of 1024 device pixels, for exactly the zoom and the screen's
  scale, are copied one to one; while the right ones come, the page's
  preview (112 points wide, also the sidebar's) and drawings at other
  zooms stand in, scaled. A pinch or a ❖-scroll asks for tiles once the
  zoom has rested 160 ms.
- **What's asked for**, most needed first: the tiles of what shows,
  nearest the middle; the previews of the pages that show; the rest of
  those pages and the whole of the page above and the page below; the
  previews around; the sidebar's. A new request replaces what the worker
  hasn't started, and goes only when the set wanted changes, not its
  order. Text and links come for the pages that show and those either
  side.
- **What's kept**: drawings up to 320 MiB a document, least recently used
  going first, never those of the pages in view and either side at the
  current zoom, nor the previews around them and in the sidebar. A page
  larger than 64 MiB at the zoom (zoomed far in) has a view's height
  either side drawn ahead instead of the whole of it. So scrolling a
  little either way and back asks the worker for nothing (tested).
- **The sidebar** is the kit's `virtual_list`, which builds only the rows
  that show. Each row, as it's built, notes itself and the three rows
  either side, and asks for any of their previews missing; the last 48
  rows noted keep theirs.
- **The pages** are a widget of Shiori's own (`pages.rs`). Like the kit's
  scroll views it leaves its offset out of its hash and says how far it
  scrolled, so Sidestep moves what was drawn and only the strip scrolled
  into view is drawn; drawings and highlights that came, went or changed
  are drawn again by their rectangles. Only the gutters are filled and only
  the pages' edges drawn under them, so each pixel is written about once.

At 2880 × 1800 (scale 2), release, with the drawings there
(`cargo test -p aizome-shiori --release --test budget -- --nocapture`), at
load average 10:

| Interaction | p50 | p95 | Of the window drawn |
|---|---|---|---|
| Scrolling a wheel's notch | 0.23 ms | 0.27 ms | 5% |
| Moving the pointer over text | 0.01 ms | 0.01 ms | 0% |
| Page Down | 5.7 ms | 8.0 ms | 100% |

Page Down shows a window of drawings never drawn before; Shiori's own part
of it is 0.07 ms, the rest is Sidestep copying 21 MB of new tiles. A
canvas redrawing everything took 9.9 ms for a notch of scrolling.

## Text

The worker gives each page's characters in reading order with their boxes
(PDFium's loose boxes, a line high; spaces and line ends it inferred have
none). `text.rs` places a press between two characters (by the nearest on
the nearest line), joins boxes along each line for the selection, finds
words and lines for double and triple clicks, and copies across pages.
Copy follows the document's permission. Find is PDFium's, page by page
from the page being read, round to it; hits come as rectangles and step in
document order.

## Testing

`cargo test -p aizome-shiori`: the geometry, text and protocol alone; the
worker through the engine (`tests/worker.rs`: opening, drawing where the
text is on a turned page, links, find, passwords, a killed worker
replaced); the app headless with real workers (`tests/shiori.rs`: drawing
exactly, moving and zooming in place, find, selecting and copying, links,
the sidebar, a locked document, a dying worker, your place, a file
changing, drawing ahead) and its look in Night and Day (`tests/golden`).
Documents are written by `tests/common` from text; `tests/fixtures/locked.pdf`
is the sample encrypted by qpdf 12.4 (the command is in `tests/worker.rs`).

`nix build .#aizome-checks.shiori -L` runs the packaged reader in an
Aizome session in a VM (`nix/vm-shiori-test.py`): it opens a document,
its worker has the package's `libpdfium` mapped, ❖F and typing find two
hits and Return goes to the second, a worker killed with `SIGKILL` is
replaced with the window open and the notice shown, and the session's
`application/pdf` default is Shiori.

Not yet tested on hardware.

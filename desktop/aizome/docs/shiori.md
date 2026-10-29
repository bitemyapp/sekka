# Shiori

Shiori 栞 (Documents) is Aizome's PDF reader; what it does is in
[apps.md](apps.md#shiori-documents). This is how it's put together, why
hayro draws its pages, and why PDFium did at first.

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

What decided it then was what a reader needs besides drawing: PDFium gives
each character's box, search with the rectangles of each hit, links and
their destinations, the outline, forms, annotations, metadata and
permissions. With hayro each of those would be written on its parsing
crates. So Shiori drew with PDFium first, while hayro, memory-safe and
already close, was made faster (below); then those were written on its
parser, and hayro replaced PDFium (`Moving to hayro`).

### Other Rust renderers, and a faster hayro

A second pass the same day
([`rust-pdf-renderers-2026-10-28.json`](../../../docs/test-results/rust-pdf-renderers-2026-10-28.json))
drew the corpus with every permissively licensed Rust renderer that draws
pages: stet, pdfrum (a port of PDFium), pdfboss, pdf_oxide and zpdf. Each
took 2.3 to 6.9 times PDFium's time over the pages both drew; pdfrum
matches PDFium's pixels most closely and is four times slower. None is a
fast renderer waiting to be made complete. hayro's current master takes 1.28
times PDFium's time, and seconds on a few pathological pages.

A fork of hayro ([bitemyapp/hayro](https://github.com/bitemyapp/hayro),
branch `aizome-perf`;
[`hayro-fork-2026-10-28.json`](../../../docs/test-results/hayro-fork-2026-10-28.json))
caches glyph coverage, drawn images, shadings, color spaces and patch-mesh
triangles. It also draws soft masks, pattern textures and mesh samples only
where clips let them show, and skips what clips hide. Five of its changes
went upstream as pull requests (the JBIG2 one is merged). With each
process pinned to the performance cores, over the 1,189 pages all three
drew in under 5 seconds, it takes 8.0 seconds against PDFium's 10.8 and
master's 14.1. Per page it's as fast as PDFium at the median, 1.4 times
PDFium's time at the 90th percentile and faster at the 99th, with no
crashes or timeouts where PDFium has three. JPEGs drawn smaller than their
size are now decoded at 1/2, 1/4 or 1/8 of it, as PDFium's libjpeg-turbo
does, through a fork of zune-jpeg
([bitemyapp/zune-image](https://github.com/bitemyapp/zune-image), branch
`scaled-idct`), which makes thumbnails of scanned pages three to four times
faster. Measured again with that on a shared Threadripper workstation (five
runs on one CCD's cores), the fork takes 6.6 seconds over those pages
against PDFium's 11.4 and master's 13.0, and is the faster on 857 of them.
hayro's code does relatively better on that machine (master takes 1.14
times PDFium's time there, 1.31 on the laptop), so compare builds on one
machine. A further round there (a CMYK table built ahead of time, meshes
sampled by runs of rows, a run of text's cached glyphs drawn as one image,
decoding only the rows of a JPEG the page can show, skipping fully
transparent paints) brings it to 5.9 seconds, 0.52 of PDFium's, faster on
961 of the 1,189 pages and behind on the rest by 0.15 seconds in all; no
page changed by more than 2 levels. A third (pixmaps of zeroed memory, Type
3 glyphs cached as other glyphs are, soft masks drawn only where their
groups draw, CID font widths kept as ranges, ICC transforms and gradient
stops built only when and as often as needed) brings it to 5.6 seconds
against PDFium's 11.3, faster on 1,068 pages and behind on the rest by 0.09
seconds. Cached Type 3 glyphs are placed to a quarter pixel, which moves
their edges on 16 pages; the other changes leave every page as it was. A
fourth round (predefined CJK CMaps embedded as tables searched in place,
PostScript functions compiled to instructions on registers, and moxcms's sRGB
curve computed once rather than for every color transform, through a fork of
moxcms, [bitemyapp/moxcms](https://github.com/bitemyapp/moxcms) branch
`gamma-table-cache-0.8`) brings it to 0.49 of PDFium's time, faster on 1,077
pages and behind on the rest by 0.07 seconds, with every page as it was; the
CJK and PostScript pages are now faster than PDFium's. It's still slower than
PDFium on free-form and CMYK patch meshes, soft masks over images and nested
transparency groups, and setting up a Lab color space. Its pages differ from
hayro's own
references on under 1% of pixels by more than 48 levels, except one page
where a hard edge in a gradient moved a pixel.

### Moving to hayro

Shiori now draws with the fork, pinned by revision, and reads with hayro's
parser what PDFium gave it:

- **Text**: hayro hands each glyph's Unicode (its `ToUnicode`, else its
  name) and transform to a drawing device; `pdftext.rs` keeps them as
  boxes a line high, in the order the page draws them, and infers the
  spaces and line ends between them (see Text).
- **Find** searches that text in the worker (`pdftext::find`).
- **Links, destinations and the outline** (`document.rs`): Link
  annotations' rectangles and their `/Dest` or GoTo and URI actions;
  explicit destinations and names from the catalog's `/Dests` and name
  tree; the outline's items, depth and count bounded for damaged files.
- **Metadata, permissions and passwords**: the information dictionary,
  and `Pdf::permissions`, which the fork added (copying follows its bit 5).
- **Forms and annotations** are drawn from their appearance streams, as
  PDFium drew them; filling them in isn't done (nor was it with PDFium).
- **Fonts** a document names but doesn't carry: hayro's built-in Foxit
  fonts stand in for the standard 14 and for Latin ones; for Chinese,
  Japanese and Korean collections the worker asks fontconfig for a font of
  that language (Noto Sans CJK in Aizome's session), as PDFium's font
  folders did (`fonts.rs`).

Through Shiori's own path (`examples/corpus.rs`: each file opened, its
first three pages drawn at 1.5×, their text and links read, ten pages
searched), over the same 1,012 files, on the same laptop, back to back:

| | PDFium's worker | hayro's |
|---|---|---|
| Files opened, drawn, read and searched | 985 | 993 |
| A file, p50 / p90 / p99 / slowest | 24 / 42 / 176 / 918 ms | 14 / 25 / 78 / 373 ms |
| The 981 files both did | 30.7 s | 16.4 s |
| Stopped at 20 s / crashed / tiles not drawn | 2 / 1 / 2 | none |

hayro reads eleven files PDFium couldn't use (it found no pages, or
refused, crashed on or was stopped by them: `jpx_numcomps`, the two
tiling-pattern files, `crypt_filters` among them). Four go the other way:
two encrypted only for their attachments (`auth-event-ef-open` asks for
a password, `encrypted-attachment` won't open), `issue15590` won't open,
and `poppler-85140-0` has no pages. Where the drawings differ, PDFium is
still right on knockout groups, a blend mode, transfer functions and some
fonts that aren't embedded (the evaluation above); that is the fork's
next work. The package no longer carries `libpdfium` or loads it.

## A worker per document

hayro is memory-safe, but it reads files from anywhere: a document can
still make it panic, or keep it busy for seconds on a page, and a page can
ask for more memory than there is. Shiori is one program with a window per
document, so a bad file in the same process would hold up or close every
document. Each document has its own worker instead, `aizome-shiori
--worker` (`worker.rs`), started by `engine.rs` in the app (it was written
for PDFium, which stops its process on purpose when one of its checks
fails):

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
- A panic costs only the job it was on: the worker catches it, answers as
  if that tile couldn't be drawn (so the page is marked) or the page had
  no text, lets go of what hayro had cached, and carries on with the
  document open.
- The worker dies with the app (`PR_SET_PDEATHSIG`), can't gain
  privileges (`PR_SET_NO_NEW_PRIVS`) and may map at most 8 GiB.
- It reads the document once, and keeps it for its life, with what hayro
  keeps of it: parsed fonts, decoded images, glyphs drawn, and the text of
  the last 16 pages asked about (text, links and find share it).
- Running a worker per document also lets documents draw at the same time.

Over the pdf.js corpus through this path, 993 of 1,012 files opened, drew,
gave their text and links and were searched, in 14 ms at the median (78 ms
p99; the table above); none was stopped or crashed its worker, and the rest
were refused at once (passwords, files that aren't PDFs) or have no pages.

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
load average 2 to 3:

| Interaction | p50 | p95 | Of the window drawn |
|---|---|---|---|
| Scrolling a wheel's notch | 0.17 ms | 0.20 ms | 5% |
| Moving the pointer over text | 0.00 ms | 0.00 ms | 0% |
| Page Down | 3.2 ms | 3.8 ms | 100% |

Page Down shows a window of drawings never drawn before; Shiori's own part
of it is 0.04 ms, the rest is the toolkit copying 21 MB of new tiles (at
load average 10 with PDFium drawing, it was 5.7 ms). A canvas redrawing
everything took 9.9 ms for a notch of scrolling. These measure the window,
not the engine: the drawings are there before they start.

## Text

The worker gives each page's characters in reading order with their boxes
(`pdftext.rs`). They come in the order the page draws its glyphs, which is
the reading order of almost every document; each glyph's box runs along
its baseline for its advance, from 0.22 em below it to 0.88 em above (a
line high, as readers select), and is split evenly among the characters of
a ligature. Between glyphs the worker infers a space where the gap is over
0.15 em, and a line end where the next glyph is half an em off the line or
runs another way; these have no box. Vertical text reads down. A glyph
drawn again in nearly the same place (bold made by overprinting) counts
once, and `/ActualText` takes the place of the glyphs it covers. Invisible
text (a scan's recognized words) counts like any other.

`text.rs` places a press between two characters (by the nearest on the
nearest line), joins boxes along each line for the selection, finds words
and lines for double and triple clicks, and copies across pages. Copy
follows the document's permission. Find runs in the worker on that text,
page by page from the page being read, round to it: runs of white space
(line ends included) match one another, ligatures match their letters,
soft hyphens are skipped, and case is ignored unless asked. Hits come as
rectangles, one for each line they cross, and step in document order.

## Testing

`cargo test -p aizome-shiori`: the geometry, text and protocol alone,
and the worker's parts (laying glyphs out into text, finding, PDF text
strings, fonts); the worker through the engine (`tests/worker.rs`:
opening, drawing where the text is on a turned page, links, find,
passwords, a killed worker replaced); the app headless with real workers
(`tests/shiori.rs`: drawing exactly, moving and zooming in place, find,
selecting and copying, links, the sidebar, a locked document, a dying
worker, your place, a file changing, drawing ahead) and its look in Night
and Day (`tests/golden`: made when PDFium drew the pages, and hayro's
drawings match them within the kit's tolerance). Documents are written by
`tests/common` from text; `tests/fixtures/locked.pdf` is the sample
encrypted by qpdf 12.4 (the command is in `tests/worker.rs`).

`nix build .#aizome-checks.shiori -L` runs the packaged reader in an
Aizome session in a VM (`nix/vm-shiori-test.py`): it opens a document,
its worker is the package's own program and loads no PDFium, ❖F and
typing find two hits and Return goes to the second, a worker killed with
`SIGKILL` is replaced with the window open and the notice shown, and the
session's `application/pdf` default is Shiori.

Not yet tested on hardware.

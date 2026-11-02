# PDF rendering

How fast Shiori's PDF engine draws pages, how close its pages are to
PDFium's, and what it costs in memory and energy, against the engines other
readers use; and what hayro's conformance tests do and don't show. Shiori,
Aizome's PDF reader, draws with hayro, a PDF renderer in Rust, through a fork
of it ([`desktop/aizome/docs/shiori.md`](../../desktop/aizome/docs/shiori.md)).

Measured on runnymede (AMD Ryzen 7 9800X3D, 8 cores, NixOS) on 2 November
2026. This is component evidence, headless: each engine drawing pages into
memory, not a reader on a display. The dataset is in [`data/pdf/`](data/pdf/)
(described in its [README](data/pdf/README.md)); the programs and scripts are
in [`tests/performance/pdf/`](../../tests/performance/pdf/).

## In short

- **Speed.** Over the 1,179 pages of the pdf.js corpus that every engine
  drew (first five pages of each file, 2×, one thread), Shiori's hayro takes
  **0.48 of PDFium's time** (4.2 s against 8.7 s) and is the faster on 89% of
  pages; per page, 1.10 / 5.6 / 43 ms at the 50th / 90th / 99th percentile
  against PDFium's 1.45 / 7.8 / 81. Upstream hayro takes 1.08 of PDFium's
  time, MuPDF 1.26, Poppler 1.31 (Cairo) and 2.46 (Splash); pdf.js, run in Node, 9.1 times (see [pdf.js](#pdfjs) for why that overstates it).
- **Robustness.** No hayro build crashed or hit the 30-second limit on any of
  the 1,012 files; PDFium, MuPDF and Poppler each crashed on one or two files
  and ran out of time on one to three.
- **Where it's still slower than PDFium**: meshes, soft masks over images,
  nested transparency groups, Lab set-up and shadings through gradient tables
  (1.3 to 2.4 times PDFium's time on those pages), and plain-text pages
  derived from one paper (about 1.5 times).
- **Pixels.** Its pages are as close to PDFium's as MuPDF's and Poppler's are
  (61 of 1,189 pages differ from PDFium on over 5% of pixels; MuPDF 86,
  Poppler 124); where they differ, PDFium is right on knockout groups,
  transfer functions and annotations without appearance streams, and hayro
  on CalGray, CMYK through ICC and some broken files.
- **Memory**: the median process peaks at 18.5 MiB, like PDFium's and
  MuPDF's; one 847-million-pixel page took 8.3 GiB (PDFium and MuPDF refuse
  to draw it).
- **Energy**: over the same 5,895 pages the fork used **0.46 of PDFium's energy** at the CPU socket (386 against 833 J); upstream 0.89, MuPDF 1.17, Poppler 1.53 (Cairo) and 2.40 (Splash). Every engine kept its cores equally busy, so energy follows drawing time.
- **Conformance.** hayro's test suite includes 101 runnable files from
  PDFium's bug tracker, checked against hayro's own reference images: upstream
  passes all 101, the fork 58 at the suite's 3-level tolerance, and the other
  43 are within anti-aliasing. PDFium's own pixel tests aren't part of it; run
  here, the fork matches 132 of 182 within 1% of pixels (45 exactly), where
  PDFium itself matches all 182 (180 exactly). Neither is an exhaustive
  conformance suite; see [Conformance](#conformance).

## What was compared

| Engine | Build | Licence | Drawn through |
|---|---|---|---|
| **hayro, Shiori's fork** | [bitemyapp/hayro](https://github.com/bitemyapp/hayro) `a2e9e62b` (branch `aizome-perf`, the revision `apps/shiori/Cargo.toml` pins: 50 commits on upstream `8558a8a5`), with the moxcms (`b6329802`) and zune-jpeg (`13f18bce`, `scaled-idct`) forks Aizome's workspace patches in; hayro's default features and hayro-syntax's `unsafe` (SIMD Flate, JPEG, JBIG2 and JPEG 2000 decoding), as Shiori builds it | Apache-2.0 OR MIT | `hayro::render`, default settings, one render cache per document |
| **hayro upstream** | [LaurenzV/hayro](https://github.com/LaurenzV/hayro) `04a801de` (main, 1 November 2026), same features; its defaults now hint text and cache glyphs (added since the fork's base) | Apache-2.0 OR MIT | as the fork |
| **PDFium** | nixpkgs `pdfium` 8110, built from source with GN and GCC: no V8 (so no JavaScript or XFA), AGG renderer (no Skia), system FreeType 2.14.3, libjpeg-turbo, OpenJPEG, lcms2, ICU | BSD-3-Clause, Apache-2.0 | its C API, as a reader: a form-fill environment, `FPDF_RenderPageBitmap` with `FPDF_ANNOT`, then `FPDF_FFLDraw` |
| **MuPDF** | nixpkgs `mupdf` 1.28.5 | AGPL-3.0 | its C API: the draw device into an RGB pixmap cleared to white, `fz_run_page` (contents, annotations, widgets), one thread |
| **Poppler (Splash)** | nixpkgs `poppler` 26.10.0, poppler-cpp | GPL-2.0-or-later | `page_renderer` with anti-aliasing and text anti-aliasing, as `pdftoppm` draws |
| **Poppler (Cairo)** | the same Poppler, poppler-glib, Cairo 1.18.4 | GPL-2.0-or-later | `poppler_page_render` into a Cairo image surface painted white, as Evince and Papers draw |
| **pdf.js** | `pdfjs-dist` 6.5.438 (legacy build) on Node.js 24.21.0, drawing into `@napi-rs/canvas` 1.1.0 (Skia) | Apache-2.0 | `getPage` and `page.render`, its worker running in the same thread |

MuPDF and Poppler can't be part of Aizome (their licences are copyleft; Aizome
is MIT OR Apache-2.0; [`shiori.md`](../../desktop/aizome/docs/shiori.md#which-engine)),
and pdf.js needs a JavaScript engine. They're here as the yardsticks a reader
on Linux is usually measured against: Evince and Papers draw with Poppler and
Cairo, Okular with Poppler, Zathura and many others with MuPDF, Firefox with
pdf.js, Chrome with PDFium.

The C and C++ programs were built with GCC 16.2.0 `-O2`, the Rust ones with
rustc 1.99.0 in the Aizome workspace's release profile (four codegen units,
no LTO, baseline x86-64), everything from the nixpkgs revision the Sekka flake
pins. Store paths and revisions are in
[`data/pdf/environment.json`](data/pdf/environment.json).

## Corpus

The pdf.js test corpus at revision `279175b9`, as in the earlier records: the
1,012 PDFs in `test/pdfs` (not the 461 it links to). Many are reduced bug
reports, fuzzer output and files broken on purpose, so failure rates here are
not real-world failure rates, and the slow pages are the pathological ones
that bug reports are made of. A second, small set of real documents (the
first 20 pages of each of seven public-domain and CC-BY documents: tax
guidance and a form, the Federal Register, the US budget, the annotated
Constitution, a pocket Constitution and a PLOS ONE paper) is in
[`data/pdf/realworld.tsv`](data/pdf/realworld.tsv) with URLs and SHA-256s;
the PDFs aren't committed.

## Method

The same method as the earlier records, so ratios can be compared:

- **A process per file**, under `timeout 30`, four at a time, each pinned to
  a core of its own (`taskset -c 2`, `3`, `4`, `5`; their SMT siblings 10-13
  left idle); release builds; every engine single-threaded.
- **The first five pages at 2×** (144 dpi) on white; a page's time runs from
  loading the page to its finished pixels (opening the document is timed
  apart and not included). Sizes are the page's size in points times two,
  rounded down (Poppler's Splash rounds up, so its pages can be a pixel
  larger).
- **Three timed runs** of the whole corpus per engine, the engines' order
  rotated between runs (`r1`: PDFium, fork, upstream, MuPDF, Splash, Cairo;
  `r2` and `r3` rotated), each run waiting for a quiet machine (one-minute
  load under 1.5) and holding a lock that kept the other agents' measurements
  and builds off the machine. A page's time is the median of its three.
  pdf.js was added afterwards and ran three rounds of its own alternating
  with PDFium.
- **Pages drawn** counts pages with a time; **failures** are files the engine
  refused to open, stopped after 30 s, or crashed on (a signal or non-zero
  exit before it finished), in every run.
- **Totals** are over pages every engine compared drew; ratios are to PDFium;
  "faster" counts pages whose median is below PDFium's.
- **Peak memory** is each process's peak resident set (`ru_maxrss` of the
  process, through `wait4`), the median of its three runs, then the maximum
  and median over files.
- **Energy**: each engine's whole pass sat inside the PPT power meter (the
  AMD socket's power: cores, SoC and IO die; not RAM or the PSU), with
  20-second idle baselines at the start and end of every round. A pass over
  the whole corpus lasts as long as its slowest file (30 s for an engine that
  times out, with the other cores idle by then), so three more rounds
  measured energy over the 969 files every engine finished and drew the same
  number of pages of, the list passed five times over (4,845 processes, 7 to
  40 s a pass), with a "null" program that draws nothing to measure what
  starting a process per file costs. A pilot round over the list once is in
  the data (`p1`) but not in the medians.
- **Pixel agreement** with PDFium comes from a separate, untimed pass that
  wrote every page: a pixel differs when its largest channel difference
  exceeds 48 of 255, on pages both drew at the same size (within 2 pixels),
  over the area they share.

## Speed

Over the pdf.js corpus, the first five pages of each file at 2×, each page's
time the median of three runs:

| Engine | Pages drawn | p50 | p90 | p99 | Slowest | Total, 1,179 pages | Of PDFium's time | Faster than PDFium on |
|---|---|---|---|---|---|---|---|---|
| PDFium 8110 | 1,195 | 1.45 ms | 7.80 ms | 80.9 ms | 1,588 ms | 8.70 s | 1.00 | - |
| **hayro, Shiori's fork** | 1,208 | 1.10 ms | 5.62 ms | 43.5 ms | 6,024 ms | 4.17 s | 0.48 | 1,045 (89%) |
| hayro upstream | 1,206 | 1.24 ms | 8.16 ms | 103 ms | 6,140 ms | 9.36 s | 1.08 | 886 (75%) |
| MuPDF 1.28.5 | 1,193 | 3.02 ms | 15.0 ms | 116 ms | 10,617 ms | 10.98 s | 1.26 | 281 (24%) |
| Poppler 26.10 (Splash) | 1,198 | 5.36 ms | 25.4 ms | 163 ms | 7,239 ms | 21.35 s | 2.46 | 40 (3%) |
| Poppler 26.10 (Cairo) | 1,196 | 2.26 ms | 13.6 ms | 117 ms | 917 ms | 11.37 s | 1.31 | 306 (26%) |

Totals are over the 1,179 pages all six engines drew. Ratios and "faster"
are against PDFium on the same pages; against PDFium on the pages each engine
and PDFium both drew, the ratios are within 0.03 of these (in
[`summary.json`](data/pdf/summary.json), `against_pdfium_pages_both_drew`).
The three runs' totals over those pages agree within 1.5% for every engine
but hayro upstream (2.5%: 9,153 to 9,382 ms; `total_ms_by_run`).

What it shows:

- **The fork draws the corpus in 0.48 of PDFium's time** and is the faster on
  1,045 of 1,179 pages. On the 135 pages where it is slower (of the 1,193 both
  drew), it is behind by 94 ms in all. Its median page takes 0.72 of PDFium's
  time, its 90th-percentile page 1.02 and its 99th 1.67. This matches the
  last record on another machine (0.488 of PDFium's total, faster on 1,077 of
  1,189 pages, on wx-workstation's Threadripper), so the fork's lead holds
  across both machines.
- **Upstream hayro is about as fast as PDFium overall** (1.08 of its total)
  and faster on the median page, but its tail is long: its 99th-percentile
  page takes 101 ms to PDFium's 80, and a few pages take seconds. The fork
  is 2.2 times faster than upstream over the corpus.
- **MuPDF and Poppler are slower than both** on this corpus: MuPDF takes
  1.26 of PDFium's time, Poppler with Cairo 1.31 and with Splash 2.46. MuPDF
  and Poppler's Cairo path are faster than PDFium on about a quarter of pages
  (Splash on 3%), the fork on 89%.
- **Failures**, in every run alike: no hayro build crashed or ran out of
  time on any file. PDFium stopped itself (`SIGTRAP`, one of its own checks)
  on `jpx_numcomps` and ran past 30 s on the two `tiling-pattern-tiny-steps`
  files; MuPDF aborted on `bug_jpx` and ran past 30 s on
  `tiling-pattern-tiny-steps-large`; Poppler crashed on `bug_jpx` (Splash
  with `SIGFPE`, Cairo with `SIGSEGV`, and Cairo also on
  `ContentStreamCycleType3insideType3`) and ran past 30 s on `bomb_giant`
  and both tiny-steps files. Each engine refused 12-18 files: those needing
  a password (none was given) and broken ones; the fork opens two that
  upstream refuses (`auth-event-ef-open` and `encrypted-attachment`,
  encrypted only for their attachments), and refuses `issue15590` (its
  objects have lost their `<<` and `>>`), `Brotli-Prototype-FileA` and
  `poppler-937-0-fuzzed`, which PDFium opens. Lists per engine are in
  `summary.json` (`files_not_ok`).
- **The slowest pages**: the fork's slowest page is `issue19517`, a page of
  12,608 by 16,806 points drawn at 25,216 by 33,612 pixels (847 million) in
  6.0 s; PDFium and MuPDF don't draw it (their bitmap can't be made), Poppler's
  Splash returns a 1-by-1 image after 7.2 s and Cairo gives up after 22 s.
  Shiori never asks for such a drawing: it draws in tiles of at most 4,096
  pixels. Without it, the fork's slowest page is 706 ms
  (`function_based_shading_cmyk`, which PDFium takes 1,170 ms over).

### Where the fork is still slower than PDFium

The pages where the fork loses most time to PDFium are the ones the last
record named, and they are still behind (milliseconds, medians of three):

| Page | What | PDFium | Fork | Upstream | MuPDF | Poppler Splash | Poppler Cairo |
|---|---|---|---|---|---|---|---|
| `issue2948` p1 | free-form mesh | 5.10 | 11.2 | 248 | 7.02 | 45.2 | 262 |
| `personwithdog` p1 | CMYK patch mesh | 55.0 | 56.8 | 182 | 57.2 | 62.7 | 10.0 |
| `issue6231_1` p1 | mesh coloured by a stitching function | 3.02 | 5.63 | 25.3 | 7.64 | 23.0 | 10.0 |
| `bug1755507` p1 | a JPEG under a soft mask | 6.89 | 13.5 | 39.5 | 95.6 | 251 | 66.3 |
| `issue18032` p1 | soft mask | 3.72 | 6.04 | 78.3 | 70.8 | 258 | 17.6 |
| `issue12798_page1_reduced` p1 | nested transparency groups | 3.88 | 6.89 | 27.0 | 59.8 | 132 | 31.5 |
| `issue5044` p1 | soft masks and groups | 1.76 | 3.21 | 8.45 | 23.3 | 14.2 | 9.15 |
| `issue18894` p1 | soft mask | 1.84 | 2.99 | 3.78 | 17.1 | 71.0 | 3.55 |
| `issue10339_reduced` p1 | Lab colour space set-up | 0.44 | 1.05 | 2.85 | 3.23 | 0.91 | 0.34 |
| `issue2761` p1 | Lab colour space set-up | 1.55 | 1.96 | 5.85 | 5.91 | 4.24 | 2.34 |
| `issue16316` p1 | Type 1 font parsing | 2.19 | 3.55 | 4.11 | 7.96 | 11.7 | 3.46 |
| `issue8092` p1 | shadings through gradient tables | 5.44 | 8.92 | 36.7 | 9.84 | 29.6 | 14.5 |
| `large_jpeg_downscale` p1 | JPEG decoding | 13.8 | 16.3 | 37.8 | 14.6 | 33.6 | 48.3 |
| `radial_gradients` p3 | radial gradients | 9.45 | 13.6 | 27.1 | 6.75 | 38.6 | 64.7 |
| `radial_gradients` p5 | radial gradients | 9.73 | 14.3 | 27.6 | 7.31 | 39.4 | 68.3 |
| `issue918` p1 | Type 3 image glyphs | 4.00 | 6.25 | 16.4 | 6.30 | 6.01 | 23.9 |
| `function_based_shading` p1 | PostScript function shading | 42.4 | 43.9 | 85.9 | 100 | 69.1 | 13.8 |
| `vertical_rotated_fallback_glyphs` p1 | predefined CJK CMaps | 1.10 | 0.62 | 9.73 | 5.26 | 126 | 135 |
| `tracemonkey` p3 | text (pdf.js's tracemonkey paper) | 2.95 | 3.63 | 3.09 | 1.92 | 12.9 | 5.24 |

Most of the fork's remaining deficit is in these: free-form and patch meshes
(`issue2948` 11.2 against 5.1 ms, `issue6231_1`), soft masks over images
(`bug1755507` 13.5 against 6.9, `issue18032`), nested transparency groups
(`issue12798_page1_reduced` 6.9 against 3.9, `issue5044`), shadings sampled
through gradient tables (`issue8092`, `radial_gradients`), the first drawing
of Type 3 image glyphs (`issue918`) and Lab set-up (`issue10339_reduced`
1.05 against 0.44 ms, `issue2761`). `personwithdog` (a CMYK patch mesh) is
within 4% of PDFium (56.8 against 55.0 ms); of the pages the last record had
brought ahead of PDFium, the predefined-CMap page stays well ahead (0.62
against 1.10 ms) and the PostScript-function shading is level (43.9 against
42.4). Also behind, though plain text: the third page of pdf.js's
tracemonkey paper takes the fork 3.6 ms against PDFium's 3.0 and upstream's
3.1, and the same page in nine other files built from that paper
(`tracemonkey_with_annotations`, `issue18911`, `comments` and others) about
4.5 ms against PDFium's 3.0 and upstream's 3.0-3.5. The last record saw
these pages move by a millisecond between builds with the same instruction
counts (memory layout); that wasn't investigated further here. The fork is
faster than upstream on every page in the table but this text page, where
upstream's own glyph cache (added since the fork's base) now does as well.

### pdf.js

pdf.js ran three rounds of its own over the whole corpus, alternating with
PDFium (rounds `j1`-`j3`; PDFium's totals in these rounds are within
0.2% of its totals in the main rounds):

| Engine | Pages drawn | p50 | p90 | p99 | Slowest | Total over pages both drew | Of PDFium's time | Faster than PDFium on |
|---|---|---|---|---|---|---|---|---|
| pdf.js 6.5 (Node, Skia) | 1,208 | 27.7 ms | 105 ms | 368 ms | 8,970 ms | 67.01 s | 9.05 | 2 of 1,192 |
| PDFium 8110, same rounds | | 1.46 ms | 7.71 ms | 81.0 ms | | 7.40 s | 1.00 | |

pdf.js takes 9.1 times PDFium's time over the 1,192 pages both drew, and is
the faster on 2 of them; its median page takes 28 ms to PDFium's 1.5. Part
of that is this setup rather than pdf.js's drawing: every file starts a fresh
Node process, so the first page also pays for compiling pdf.js (its first
pages take 3.5 times its fifth, PDFium's 2.6), and its time includes reading
the finished pixels back out of the canvas, which the other engines don't
need (not measured apart). Even its fifth pages take ten times PDFium's (a
median of 25 against 2.5 ms over the 34 files that have five). It refused 14 files and ran
past 30 s on `issue16263`; it drew `issue19517`'s giant page in 9.0 s. Its
pages are about as close to PDFium's as the others' (436 of 1,187 under 0.1%
of pixels differing, 87 over 5%; table below). Firefox runs pdf.js
differently (a warm JIT, parsing in a worker, its own canvas), so this is
not a measure of reading PDFs in Firefox.

### Other documents

The pdf.js corpus is mostly small and broken files. Seven real documents,
public domain or CC-BY (their URLs and SHA-256s in
[`realworld.tsv`](data/pdf/realworld.tsv)), the first 20 pages of each, five
runs, each page's time the median:

| Engine | Total, 113 pages | Of PDFium's time | Faster than PDFium on |
|---|---|---|---|
| PDFium 8110 | 0.48 s | 1.00 | - |
| **hayro, Shiori's fork** | 0.37 s | 0.78 | 102 |
| hayro upstream | 0.46 s | 0.96 | 81 |
| MuPDF 1.28.5 | 0.53 s | 1.11 | 93 |
| Poppler 26.10 (Splash) | 2.20 s | 4.61 | 0 |
| Poppler 26.10 (Cairo) | 0.77 s | 1.61 | 50 |

Per document, the total of the 20 pages in milliseconds:

| Document (first 20 pages) | PDFium 8110 | hayro, Shiori's fork | hayro upstream | MuPDF 1.28.5 | Poppler 26.10 (Splash) | Poppler 26.10 (Cairo) |
|---|---|---|---|---|---|---|
| `constitution-annotated-2017` | 59 | 39 | 54 | 62 | 769 | 127 |
| `federal-register-2026-10-30` | 94 | 76 | 86 | 62 | 310 | 125 |
| `irs-f1040` | 18 (2 p.) | 14 (2 p.) | 14 (2 p.) | 32 (2 p.) | 45 (2 p.) | 23 (2 p.) |
| `irs-p17` | 101 | 74 | 101 | 118 | 286 | 98 |
| `plos-one-0000001` | 53 (11 p.) | 54 (11 p.) | 61 (11 p.) | 55 (11 p.) | 291 (11 p.) | 198 (11 p.) |
| `us-budget-fy2025` | 72 | 50 | 68 | 142 | 270 | 63 |
| `us-constitution-pocket` | 81 | 67 | 73 | 58 | 230 | 136 |

On these the fork takes 0.78 of PDFium's time and is the faster on 102 of
the 113 pages; upstream hayro 0.96, MuPDF 1.11, Poppler 1.61 with Cairo and
4.6 with Splash. The fork is the fastest of the six on four of the seven
documents and within 2% of PDFium on the PLOS paper; MuPDF is the fastest on
the Federal Register and the pocket Constitution. None of the engines failed
on any of these pages, and none took 100 ms (the slowest, 93 ms, is MuPDF on a
page of the budget).

### Shiori's own path

Shiori's own path, built from this tree (`apps/shiori/examples/corpus.rs`,
as in [`shiori-hayro-2026-10-28.json`](../test-results/shiori-hayro-2026-10-28.json)):
each of the 1,012 files opened by a worker process, its first three pages
drawn at 1.5× in tiles of up to 1,024 pixels, their text and links read and
its first ten pages searched, four files at a time on cores 2-5, three runs:

| | This tree (hayro fork `a2e9e62b`), runnymede | Last record (same fork), laptop |
|---|---|---|
| Files opened, drawn, read and searched | 995 of 1,012, in every run | 995 |
| A file, p50 / p90 / p99 / slowest | 3 / 7 / 51 / 282 ms | 15 / 26 / 82 / 356 ms |
| All of them | 5.4 s | 17.4 s (the 983 PDFium also did) |
| Worker stopped, crashed, or a tile not drawn | none | none |

Of the other 17, 16 were refused when opened (8 need a password, 8 are
damaged or not PDFs) and `poppler-85140-0` opened with no pages, as in the
last record. A file's time is its median of the three runs, from starting
its worker to the last answer. This desktop is several times faster than the
laptop of the last record, so compare the columns for what completes, not
for time. The worker's CJK fallback looks fontconfig up at run time, which
this standalone run may not have found; that affects only files whose CJK
fonts aren't embedded.

## Memory

Peak resident memory of each file's process (median of its three runs):

| Engine | Median | 90th percentile | Largest | Largest but the giant pages |
|---|---|---|---|---|
| PDFium 8110 | 18.6 MiB | 23.2 MiB | 176 MiB | 176 MiB |
| **hayro, Shiori's fork** | 18.5 MiB | 18.8 MiB | 8,341 MiB | 171 MiB |
| hayro upstream | 18.4 MiB | 19.1 MiB | 8,340 MiB | 498 MiB |
| MuPDF 1.28.5 | 18.5 MiB | 26.4 MiB | 121 MiB | 121 MiB |
| Poppler 26.10 (Splash) | 27.5 MiB | 40.2 MiB | 4,117 MiB | 294 MiB |
| Poppler 26.10 (Cairo) | 28.7 MiB | 37.1 MiB | 4,323 MiB | 145 MiB |

The typical process is small for every engine (18-29 MiB, most of it the
page's pixels: 7.4 MiB for a letter page at 2×, plus the program); Poppler's
processes are about 10 MiB larger. The largest are the giant pages: the fork
needed 8.3 GiB for `issue19517`'s 847-million-pixel page that PDFium and
MuPDF refused, and Poppler 4.1-4.3 GiB for it and 2.4 GiB for `bomb_giant`.
Below those, every engine stays under 200 MiB per file except upstream hayro
(498 MiB on `tiling-pattern-box`) and Poppler's Splash (294 MiB on
`issue14497`).

## Energy

Energy at the AMD socket (the PPT sensor: cores, SoC and IO die) for each
engine's pass, the median of three rounds (`e2`-`e4`). The 969 files every
engine finished and drew alike, the list passed five times over: 4,845
processes and 5,895 pages for each engine.

| Engine | Pass (s) | Mean power (W) | Energy (J) | Over idle (J) | Over the null program (J) | Of PDFium's energy | Busy CPU, whole system (s) |
|---|---|---|---|---|---|---|---|
| null (starting a process) | 1.2 | 56.2 | 67 | 44 | - | - | 5.0 |
| PDFium 8110 | 14.9 | 55.7 | 833 | 546 | 766 | 1.00 | 61.9 |
| **hayro, Shiori's fork** | 6.9 | 56.1 | 386 | 254 | 320 | 0.46 | 28.4 |
| hayro upstream | 13.9 | 53.5 | 745 | 471 | 678 | 0.89 | 54.3 |
| MuPDF 1.28.5 | 16.7 | 58.3 | 976 | 646 | 910 | 1.17 | 69.1 |
| Poppler 26.10 (Splash) | 33.9 | 59.0 | 1,998 | 1,328 | 1,932 | 2.40 | 135.4 |
| Poppler 26.10 (Cairo) | 21.8 | 58.6 | 1,273 | 845 | 1,206 | 1.53 | 89.3 |

"Over idle" subtracts the round's idle power (18.2-20.1 W) for the pass's
length; "over the null program" subtracts what the same 4,845 processes cost
when they draw nothing, leaving the drawing itself. Every engine kept its
four cores about equally busy (54-59 W), so energy follows time: **the fork
used 0.46 of PDFium's energy** (386 against 833 J; 65 against 141 mJ a page),
0.42 of it for the drawing alone. Upstream hayro used 0.89 of PDFium's,
MuPDF 1.17, Poppler 1.53 with Cairo and 2.40 with Splash. Each engine's three
rounds agree within 5% (all in [`summary.json`](data/pdf/summary.json),
`energy.common_files`); the idle floor rose by 2 W over the rounds, and the
later rounds' totals with it.

Over the whole corpus once (rounds `r1`-`r3`), a pass lasts as long as its
slowest file, so the engines that ran out of time spent most of the pass on
one core waiting for it, and the fork's pass is mostly `issue19517`'s 6-second
page:

| Engine | Pass (s) | Mean power (W) | Energy (J) | Over idle (J) | Of PDFium's energy | Busy CPU, whole system (s) |
|---|---|---|---|---|---|---|
| PDFium 8110 | 33.1 | 45.7 | 1,511 | 884 | 1.00 | 77.2 |
| **hayro, Shiori's fork** | 6.9 | 35.6 | 246 | 117 | 0.16 | 13.1 |
| hayro upstream | 7.5 | 39.8 | 298 | 156 | 0.20 | 18.8 |
| MuPDF 1.28.5 | 33.4 | 35.7 | 1,192 | 561 | 0.79 | 59.4 |
| Poppler 26.10 (Splash) | 40.3 | 52.0 | 2,096 | 1,330 | 1.39 | 130.4 |
| Poppler 26.10 (Cairo) | 37.0 | 56.6 | 2,099 | 1,406 | 1.39 | 136.6 |

Those are the cost of a pass over this corpus as it is, timeouts included,
not of drawing the same pages; the first table is the comparison.

## How close the pages are to PDFium's

Every engine's pages compared with PDFium's, page by page, from an untimed
pass: a pixel differs when its largest channel difference exceeds 48 of 255,
on pages both drew at the same size.

| Engine | Pages compared | Under 0.1% of pixels differ | 0.1-1% | 1-5% | Over 5% | Size differs |
|---|---|---|---|---|---|---|
| **hayro, Shiori's fork** | 1,189 | 456 | 433 | 239 | 61 | 4 |
| hayro upstream | 1,187 | 442 | 394 | 312 | 39 | 4 |
| MuPDF 1.28.5 | 1,178 | 526 | 301 | 265 | 86 | 5 |
| Poppler 26.10 (Splash) | 1,186 | 459 | 351 | 252 | 124 | 0 |
| Poppler 26.10 (Cairo) | 1,185 | 523 | 311 | 227 | 124 | 0 |
| pdf.js 6.5 (Node, Skia) | 1,187 | 436 | 438 | 226 | 87 | 5 |

There is no right answer to compare with; PDFium is the reference only
because it is what Shiori used before and what Chrome draws. By this measure
the fork is as close to PDFium as MuPDF and Poppler are: about half of every
engine's pages differ on under 0.1% of pixels, and the fork has fewer pages
over 5% (61) than MuPDF (86) or Poppler (124). Upstream is closer still on
the worst pages (39 over 5%); the fork's quarter-pixel glyph placement moves
text edges. The fork and upstream are identical on 406 of 1,205 pages and
within 0.1% of pixels on 768.

Looked at by eye, the pages that differ most split as they did in the first
evaluation ([`pdf-engines-2026-10-28.json`](../test-results/pdf-engines-2026-10-28.json)):

- **PDFium is right**: knockout groups (`knockout_blend_multiply`,
  `knockout_inner_backdrop`, 35-38% of pixels), transfer functions
  (`transfer_maps`), fonts that aren't embedded (`non-embedded-NuptialScript`,
  where PDFium picks a script face), and annotations without an appearance
  stream, which PDFium (and MuPDF) draw an appearance for and hayro leaves out
  (`issue12337`'s highlight, `annotation-square-circle-without-appearance`).
- **hayro is right**: CalGray drawn colorimetrically (`calgray`, 51-55% of
  pixels; PDFium treats it as gray), a subsampled JPEG 2000 image PDFium
  leaves out (`jpx_subsampling`), a Type 3 glyph error after which PDFium
  draws nothing (`type3_glyph_error`), and `bug920426`, whose text PDFium
  draws as empty boxes. On `function_based_shading_cmyk` the fork's CMYK
  colours match MuPDF's (both convert through an ICC profile) and PDFium's
  don't.
- **The same, but for anti-aliasing**: tracemonkey's pages 4 and 5 and their
  copies in ten other files (5.3-5.7% of pixels, indistinguishable by eye),
  `copy_paste_ligatures`, `issue16316`, `issue2884_reduced`.

## Conformance

There is no PDF test suite with right answers for rendering. What exists are
engines' own regression suites: files, and for each an image that engine once
drew and someone checked. Two bear on Shiori's engine:

1. **hayro's own test suite** (`tests/` in hayro): five manifests of PDFs whose
   reference images are drawn by hayro's CI. One of them,
   `manifest_pdfium.json`, is 127 PDFs from PDFium's bug tracker (Chromium
   issues in the PDFium component, not fuzzer reports, up to 14 March 2025).
   Its reference images are hayro's drawings of those files, not PDFium's.
2. **PDFium's own pixel tests** (`testing/resources/pixel` in PDFium): small
   hand-made PDFs with the PNGs PDFium draws for them. hayro's test README
   says of these that "since those are rather basic it is not planned to run
   those in hayro", and they are not in its suite.

### hayro's test suite

`cargo test -p hayro-tests --release`, every manifest, at three revisions:
the fork Shiori pins (`a2e9e62b`), upstream main (`04a801de`), and
`8558a8a5`, the upstream commit the fork branched from, whose manifests and
`references.lock` the fork still carries unchanged. A one-hunk patch to the
runner writes each test's outcome as a JSON line
([`conformance/hayro-tests-results-jsonl.patch`](../../tests/performance/pdf/conformance/hayro-tests-results-jsonl.patch));
it doesn't change how tests pass. A page passes when no pixel differs from
its reference by more than 3 in any channel (some entries allow a number of
pixels more). PDFs and reference images were downloaded from hayro's asset
store on the first run.

Raster tests per manifest (rendering a PDF and comparing every page; entries
marked `ignore` in a manifest aren't run):

| Manifest | Entries | Ignored | Run | Fork passes | Upstream passes | Fork's base passes |
|---|---|---|---|---|---|---|
| `manifest_pdfium.json` (PDFium bug tracker) | 127 | 26 | 101 | **58** | 101 | 101 |
| `manifest_pdfjs.json` (pdf.js test suite) | 679 | 22 | 658 | 264 | 657 | 657 |
| `manifest_pdfbox.json` (PDFBox bug tracker) | 454 | 25 | 429 | 130 | 429 | 425 |
| `manifest_custom.json` (hayro's own) | 335 (338) | 0 | 334 (337) | 205 | 337 | 334 |
| `manifest_corpus.json` (PDF Association corpus) | 41 | 0 | 41 | 16 | 41 | 41 |
| All raster tests | 1,636 (1,639) | 73 | 1,563 (1,566) | **673** | 1,565 | 1,558 |

Upstream's manifest of its own has three more entries (its counts in
brackets). Besides these, 52 SVG tests (entries also drawn through hayro-svg)
pass at every revision, as do the load, text and visibility tests (128, 5
and 1 at the fork); 9 of the
fork's 30 `write` tests fail, as they draw what hayro-write wrote. In all,
the fork passes 880 tests and fails 899; upstream passes 1,784 and fails 1;
the base passes 1,773 and fails 5. Every failure at upstream and the base,
and 23 of the fork's, is a test whose drawing doesn't match its reference by
hash and whose reference image can no longer be downloaded (`404`), so it
can't be compared within the tolerance.

The fork fails these tests because it draws slightly differently from
upstream on purpose (glyphs cached and placed to a quarter pixel, Type 3
glyphs cached, mesh colours from tables, JPEGs decoded at reduced size; the
last record traces each change), and the suite's tolerance is 3 levels.
Graded by the share of pixels off by more than 48 levels on each failing
test's worst page, the differences are the size of anti-aliasing:

| Fork's failing raster tests | PDFium manifest | All manifests |
|---|---|---|
| No pixel off by more than 48 | 26 | 539 |
| Under 0.1% of pixels off by more than 48 | 15 | 322 |
| 0.1-1% | 1 (`42270651`) | 5 |
| 1.0% | 0 | 1 (`pdfbox_4913`, a hard edge in a gradient a row lower) |
| Reference image no longer downloadable | 1 (`40643674`) | 23 |
| **Failed** | **43** | **890** |

The 23 that can't be compared with their references were compared with the
base revision's drawings of them instead (which match CI's references
exactly for the 18 of them the base passes): 21 differ on under 0.1% of
pixels by more than 48 (`40643674` on 0.009%), and two on 0.16-0.17%
([`conformance/hayro-suite-fork-404-vs-base.txt`](data/pdf/conformance/hayro-suite-fork-404-vs-base.txt)).
Every failing test's message and its pages' grades are in
[`conformance/hayro-suite-fork-failures.tsv.xz`](data/pdf/conformance/).
So of the fork's 1,563 raster tests, 673 match hayro's references within 3
levels, 882 more are within 0.1% of pixels of them (anti-aliasing at glyph
and shape edges), and 8 differ on 0.1-1% of a page's pixels. That is a
regression check against hayro's own past output; it says nothing about how
close hayro is to PDFium.

### PDFium's own pixel tests

To see whether "passing PDFium's tests" could be made true, the pixel tests
of PDFium's `chromium/8110` branch (the build nixpkgs packages) were drawn by
the fork and, as a control, by that PDFium, and each page compared with the
test's expected PNG
([`conformance/pdfium_pixel_tests.py`](../../tests/performance/pdf/conformance/pdfium_pixel_tests.py)).
It follows PDFium's runner where it can: `.in` templates are expanded with
PDFium's own `fixup_pdf_template.py`; pages are drawn at 72 dpi with a form
environment, fields highlighted, annotations drawn and transparent pages on
transparent, as `pdfium_test` does; fonts a document doesn't embed come from
Chromium's test fonts with `--croscore-font-names`' renaming (Helvetica and
Arial to Arimo, Times to Tinos, Courier to Cousine), which hayro was given
through its font resolver; and the expected image is the most specific of
`_agg_linux`, `_agg`, `_linux` and none, as PDFium's `pngdiffer.py` picks for
its AGG renderer. Left out: the 34 tests under `xfa_specific/` (XFA, which
neither build has) and 17 tests with an `.evt` file (mouse and keyboard
events sent before drawing, which only `pdfium_test` can send), leaving 205;
23 of those PDFium's `SUPPRESSIONS` file marks `diff` for this configuration
(PDFium's own output is known not to match; one of them only without
JavaScript), leaving 182. No `fuzzy` suppression applies to these 182, so
PDFium's own criterion for them is an exact match.

| Of the 182 tests | PDFium 8110 (control) | hayro fork | hayro upstream |
|---|---|---|---|
| Every page exactly as expected | **180** | 45 | 45 |
| Within PDFium's fuzzy defaults (3 levels, mean squared error 0.05) | 180 | 47 | 47 |
| Under 0.1% of pixels off by more than 48 | 180 | 95 | 92 |
| Under 1% | 182 | **132** | 130 |
| Under 5% | 182 | 151 | 151 |

The control shows the comparison is sound: this PDFium matches 180 of the
182 exactly, so the 50 tests the fork fails at 1% are hayro drawing
differently, not the harness. Upstream fails the same 50 and two more
(`bug_1021762` and `font_size`, text at 1.2-1.5%), so these are hayro's
differences, not the fork's. Grouped by eye
([`conformance/pdfium-pixel-tests.tsv`](data/pdf/conformance/pdfium-pixel-tests.tsv)):

- 23 are drawn differently: content missing (a tiling pattern, a Gouraud
  triangle, paths and images in malformed files PDFium recovers), content
  PDFium leaves out, colours (a JPEG 2000 image whose colour space
  disagrees with its data, Separation and blend cases), shading edges and
  extension, a knockout group;
- 9 are a different page size: with no valid MediaBox hayro falls back to
  A4 and PDFium to US Letter (7), or the two choose different boxes (2);
- 6 are damaged content streams (a wrong `/Length`) that PDFium reads to
  their `endstream` and hayro cuts short;
- 5 are annotations and form fields without an appearance stream, which
  PDFium draws an appearance for and hayro leaves blank;
- 5 differ only at glyph and stroke edges, which at 72 dpi on these small
  pages exceed 1% of pixels;
- 2 are damaged files PDFium repairs and hayro won't open.

Most of these tests are PDFium's regressions on edge cases (malformed files
and its own choices for them) rather than right answers; but some are plain
gaps in hayro (appearance streams, knockout groups, the tiling and shading
cases).

### What the conformance results support

Neither suite is an exhaustive conformance suite, and Shiori's engine passes
neither entirely. What the numbers support:

- hayro's test suite includes **127 PDFs from PDFium's bug tracker**, of
  which it runs 101. They are checked against **hayro's own reference
  images**, not PDFium's renderings. **Upstream hayro passes all 101; the
  fork Shiori uses passes 58 at the suite's 3-level tolerance**, and the
  other 43 are within anti-aliasing of hayro's references (42 with under
  0.1% of pixels off by more than 48 levels, one with 0.17%). That suite is
  a regression suite for hayro, not a conformance suite, and it isn't
  exhaustive: 127 files from one tracker, up to March 2025.
- **PDFium's own pixel tests are not part of hayro's suite.** Run here, with
  PDFium's fonts and settings, the fork draws **45 of 182** applicable tests
  exactly as PDFium expects and **132 of 182 (73%) within 1% of pixels**
  (upstream 45 and 130); the PDFium build itself matches 180 exactly and all
  182 within 1%. Those tests are about 200 small hand-made cases, not an
  exhaustive suite either.

A precise sentence: *Shiori's hayro is checked against hayro's regression
suite of about 1,560 rendering tests, including 101 files from PDFium's bug
tracker (58 of them within its 3-level tolerance, the other 43 within
anti-aliasing of hayro's reference images); on PDFium's own pixel tests it
draws 132 of the 182 that apply (73%) within 1% of pixels of PDFium's
expected images, where PDFium itself draws all 182.*

## Limits

- **One machine.** Ratios between engines depend on the machine: the last
  records found hayro's code relatively faster on a Threadripper than on an
  Intel laptop. Times here are for a desktop Zen 5 core with a large cache.
- **A hostile corpus, five pages a file.** The pdf.js corpus is bug reports
  and broken files; its slow pages are pathological, and failure counts are
  not real-world rates. Only the first five pages of a file are drawn, each
  file in a fresh process, so every page pays its document's first-use costs
  (fonts parsed, tables built); a reader that keeps a document open, as
  Shiori's worker does, pays them once.
- **Drawing only.** Opening a document isn't in a page's time; neither is
  text extraction, search or anything else a reader does (Shiori's own path,
  above, covers those for the fork).
- **The engines as built here.** PDFium is nixpkgs' GCC build with its AGG
  renderer and system libraries; Chrome builds it differently. MuPDF and
  Poppler are drawn through their libraries with the settings described, not
  through `mutool` or a viewer; Poppler finds substitute fonts through
  fontconfig on this system. pdf.js runs in Node drawing into Skia through
  `@napi-rs/canvas`, not in Firefox: a fresh Node process for each file,
  its parsing sharing the drawing thread.
- **Energy** is the AMD socket's power (cores, SoC and IO die) from the PPT
  sensor, not the whole computer: RAM, the NVIDIA card and the power supply
  aren't in it. Passes are short (7-34 s), each engine's energy is the median
  of three, and every pass includes starting a process per file (the null
  program's row).
- **Memory** is each process's peak resident set (`ru_maxrss` through
  `wait4`, which also covers `timeout`'s own few hundred KiB), not the
  memory a long-running reader holds.
- **Pixel agreement** takes PDFium as the reference because it is the one
  Shiori replaced, not because it is right; the threshold (48 levels) and the
  buckets are those of the earlier records.
- **hayro's suite** compares with hayro's own references, and 23 of the
  fork's failing tests (1 and 5 at the other revisions) can't be compared
  with them because their reference images are no longer downloadable; the
  fork's were compared with its base revision's drawings instead.
- **PDFium's pixel tests** were run without XFA, input events or
  JavaScript, and hayro was given PDFium's substitute fonts only for the
  standard fonts and the standard font it picks for another missing font;
  the grouping of failures was made by eye from thumbnails.
- No VM, ISO or hardware qualification is implied: this is component
  evidence from one desktop.

## Reproducing it

From the worktree root, inside the Nix shell (the engines come from the
nixpkgs the Sekka flake pins). `WORK` is a scratch directory; the scripts
default to `$TMPDIR/aizome-perf/pdf`, and those that measure ran under
`tests/performance/lock-measure`, which kept other measurements and builds
off the machine (alone, run them on a quiet machine). `POWERMETER` names the
power meter (`tests/performance/powermeter.c` built with `cc -O2`, the PPT
sensor); without it, `measure_round.sh` runs the passes unmetered.

1. The corpus and the programs:

   ```sh
   export WORK=${TMPDIR:-/tmp}/aizome-perf/pdf; mkdir -p $WORK/src $WORK/results
   git clone --filter=blob:none --no-checkout https://github.com/mozilla/pdf.js $WORK/src/pdfjs
   git -C $WORK/src/pdfjs sparse-checkout set --no-cone 'test/pdfs/*'
   git -C $WORK/src/pdfjs checkout 279175b948b23077808d744435c07b32d23aa5bc
   ls $WORK/src/pdfjs/test/pdfs/*.pdf | sort > $WORK/corpus.txt          # 1,012 files
   nix-shell tests/performance/pdf/shell.nix --run tests/performance/pdf/build.sh
   npm ci --prefix $WORK/pdfjs-node tests/performance/pdf/pdfjs           # pdf.js, optional
   printf '#!/bin/sh\nPDFJS_MODULES=%s exec node %s "$@"\n' $WORK/pdfjs-node/node_modules \
     $PWD/tests/performance/pdf/pdfjs/bench.mjs > $WORK/bin/pdfbench-pdfjs; chmod +x $WORK/bin/pdfbench-pdfjs
   ```

   Each `pdfbench-<engine>` takes `FILE.pdf [OUT_DIR]` and prints a line per
   page; `run_corpus.py` runs one over a list of files (`--help`).

2. Three timed rounds over the corpus, the order rotated:

   ```sh
   S=tests/performance/pdf
   nix-shell $S/shell.nix --run "$S/measure_round.sh 1 pdfium hayro-fork hayro-upstream mupdf poppler-splash poppler-cairo"
   nix-shell $S/shell.nix --run "$S/measure_round.sh 2 mupdf poppler-splash poppler-cairo pdfium hayro-fork hayro-upstream"
   nix-shell $S/shell.nix --run "$S/measure_round.sh 3 poppler-cairo hayro-upstream hayro-fork poppler-splash mupdf pdfium"
   ```

3. The energy rounds over the files every engine finished alike
   ([`data/pdf/common-files.txt`](data/pdf/common-files.txt)), listed five
   times over, and pdf.js's rounds:

   ```sh
   for i in 1 2 3 4 5; do sed "s|^|$WORK/src/pdfjs/test/pdfs/|" docs/performance/data/pdf/common-files.txt; done > $WORK/corpus-common.txt
   E="LIST=corpus-common.txt PREFIX=e PAGES=pages-common.tsv FILES=files-common.tsv"
   nix-shell $S/shell.nix --run "env $E $S/measure_round.sh 2 mupdf poppler-splash poppler-cairo null pdfium hayro-fork hayro-upstream"
   # ... rounds 3 and 4 with the order rotated, as in data/pdf/rounds.log
   J="PREFIX=j PAGES=pages-pdfjs.tsv FILES=files-pdfjs.tsv"
   nix-shell $S/shell.nix --run "env $J $S/measure_round.sh 1 pdfjs pdfium"     # and 2, 3
   ```

4. The real documents (download each URL in
   [`realworld.tsv`](data/pdf/realworld.tsv), check its SHA-256, list the
   paths in `$WORK/realworld.txt`) and Shiori's own path:

   ```sh
   nix-shell $S/shell.nix --run $S/measure_realworld.sh
   nix develop .#aizome -c env CARGO_TARGET_DIR=$WORK/target-shiori cargo build --locked --release \
     --manifest-path desktop/aizome/Cargo.toml -p aizome-shiori --bin aizome-shiori --example corpus
   $S/measure_shiori.sh
   ```

5. The untimed pass that writes every page (5-7 GB per engine), and the
   comparison with PDFium:

   ```sh
   nix-shell $S/shell.nix --run "$S/render_pass.sh pdfium hayro-fork hayro-upstream mupdf poppler-splash poppler-cairo"
   nix-shell $S/shell.nix --run "python3 $S/compare.py $WORK/render $WORK/results/agreement.tsv hayro-fork hayro-upstream mupdf poppler-splash poppler-cairo"
   ```

6. hayro's test suite, at each revision (`a2e9e62b`, `04a801de`, `8558a8a5`):

   ```sh
   git clone https://github.com/bitemyapp/hayro $WORK/src/hayro-fork && cd $WORK/src/hayro-fork
   git checkout a2e9e62bdeb6872745ee6f0c811760ba31d441e9
   git apply <worktree>/tests/performance/pdf/conformance/hayro-tests-results-jsonl.patch
   HAYRO_RESULTS_JSONL=$WORK/results/suite-hayro-fork.jsonl cargo test -p hayro-tests --release -- -j 8
   python3 <worktree>/tests/performance/pdf/conformance/summarize_suite.py $WORK/results/suite-hayro-fork.jsonl \
     tests tests/report $WORK/results/suite-fork-summary.json $WORK/results/suite-fork-failures.tsv
   ```

   The tests whose references can't be downloaded: `cargo test -p hayro-tests
   --release -- --store --exact TEST...` in the fork and the base, then
   `conformance/compare_stores.py FORK/tests/store/pdf BASE/tests/store/pdf`.

7. PDFium's pixel tests:

   ```sh
   git clone --filter=blob:none --no-checkout https://pdfium.googlesource.com/pdfium $WORK/src/pdfium
   git -C $WORK/src/pdfium sparse-checkout set --no-cone '/testing/*'
   git -C $WORK/src/pdfium checkout 7c55203ed66b0461378323e3fcf2efaf76755241   # chromium/8110
   curl -L https://storage.googleapis.com/chromium-fonts/cd96fc55dc243f6c6f4cb63ad117cad6cd48dceb | tar xz -C $WORK
   nix-shell $S/shell.nix --run "python3 $S/conformance/pdfium_pixel_tests.py $WORK/src/pdfium $WORK/pixel $WORK/test_fonts \
     pdfium=$WORK/bin/pdfbench-pdfium-pixel hayro-fork=$WORK/bin/pdfbench-hayro-fork-pixel \
     hayro-upstream=$WORK/bin/pdfbench-hayro-upstream-pixel"
   python3 $S/conformance/pixel_tests_table.py $WORK/pixel/pixel-results.json pdfium-pixel-tests.tsv
   ```

8. The summaries and the dataset: `nix-shell $S/shell.nix --run $S/collect.sh`
   (runs `analyze.py` and `analyze_extra.py` and writes
   `docs/performance/data/pdf`).

# PDF rendering data

What [`../../pdf.md`](../../pdf.md) reports, as measured on runnymede
(AMD Ryzen 7 9800X3D) on 2 November 2026. The programs and scripts that
produced it are in [`tests/performance/pdf/`](../../../../tests/performance/pdf/).
Engines are named `pdfium`, `hayro-fork` (bitemyapp/hayro a2e9e62b, Shiori's),
`hayro-upstream` (LaurenzV/hayro 04a801de), `mupdf`, `poppler-splash`,
`poppler-cairo`, `pdfjs` and `null` (a program that draws nothing: the cost
of starting a process per file).

## The pdf.js corpus (1,012 files, first 5 pages at 2x)

| File | What it holds |
|---|---|
| `summary.json` | Everything the write-up's tables come from (`analyze.py`): per engine, pages drawn, per-page p50/p90/p99/max/mean of the per-page medians, failures by file (refused, timeout, crash, per run), peak RSS per process; against PDFium on the pages both drew (totals, ratio, pages faster and behind, ratio percentiles); the pages all six engines drew (`common_set`); the 25 pages where the fork loses most time to PDFium; energy per pass; pixel agreement with PDFium. |
| `page-medians.csv.xz` | engine, file, page, the median of the runs' times (ms), how many runs drew it, width, height. |
| `pages.tsv.xz` | Every page of every timed run: run tag (r1-r3), engine, file, page, ms, width, height. |
| `files.tsv.xz` | Every process of every timed run: run, engine, file, status (ok, refused, timeout, crash), exit, wall ms, peak RSS (KiB, `ru_maxrss` of the process and `timeout`), open ms, pages in the file, pages drawn, pages failed, detail. |
| `agreement.tsv.xz` | Per page drawn by an engine and PDFium: sizes, whether compared (same size within 2 px), pixels whose largest channel difference exceeds 48, that as a fraction, the largest difference, mean absolute difference. From an untimed render pass. |
| `agreement-fork-vs-upstream.tsv.xz` | The same, the fork against upstream hayro. |
| `render-files.tsv.xz` | The untimed render pass's process records (as `files.tsv`). |
| `power/` | The PPT meter's JSON summary for each engine's pass and each idle baseline: `r<round>-<engine>.json` over the whole corpus (rounds 1-3), `e<round>-<engine>.json` over the 969 files every engine finished alike, the list passed five times (4,845 processes; rounds 2-4), `p1-<engine>.json` a pilot that passed over them once (`round e1` in `rounds.log`), `j<round>-<engine>.json` pdf.js's rounds with PDFium; `power-samples.tsv.xz` has every sample (round, subject, seconds, watts). |
| `pages-common.tsv.xz`, `files-common.tsv.xz` | The energy rounds' timings over those files (as `pages.tsv`, `files.tsv`; tags `p1`, `e2`-`e4`). |
| `common-files.txt` | Those 969 files. |
| `pages-pdfjs.tsv.xz`, `files-pdfjs.tsv.xz`, `page-medians-pdfjs.csv.xz`, `agreement-pdfjs.tsv.xz` | pdf.js's three rounds with PDFium alongside (tags `j1`-`j3`), its per-page medians and its pixel agreement with PDFium. |
| `rounds.log` | Start and end of every pass with the load averages. |
| `environment.json` | Machine, kernel, CPU settings, compilers, engine versions and Nix store paths, revisions. |

## Other documents

| File | What it holds |
|---|---|
| `realworld.tsv` | The seven public-domain and CC-BY documents: name, URL, SHA-256, bytes, licence. The PDFs aren't committed. |
| `realworld-pages.tsv.xz`, `realworld-files.tsv.xz` | Five timed runs of each engine over their first 20 pages (as `pages.tsv`, `files.tsv`). |
| `realworld-summary.json` | Per document and engine: pages drawn, total of the per-page medians, p50 and max. |

## Shiori's own path

| File | What it holds |
|---|---|
| `shiori-corpus.tsv.xz` | `examples/corpus.rs` over the 1,012 files, three runs: per file the median and each run's time and what happened. |
| `shiori-corpus-summary.json` | Files completed, the per-file distribution, failures. |

## Conformance

| File | What it holds |
|---|---|
| `conformance/hayro-suite-<build>.jsonl.xz` | hayro's test suite, one JSON line per test (name, outcome, message, ms), for `fork` (a2e9e62b), `upstream` (04a801de) and `base` (8558a8a5, the upstream commit the fork branched from). |
| `conformance/hayro-suite-<build>-summary.json` | Counts per suite (raster, svg, load, write, text) and manifest, with each failing raster test graded by its worst page. |
| `conformance/hayro-suite-<build>-failures.tsv.xz` | Every failing test: suite, manifest, name, cause, grade, the suite's message, and per page the pixels over 3, 16 and 48 levels and the largest difference. |
| `conformance/hayro-suite-fork-404-vs-base.txt` | The fork's 23 failing tests whose reference images can't be downloaded, compared with the base revision's drawings of them. |
| `conformance/pdfium-pixel-summary.json` | PDFium's pixel tests (chromium/8110): what was left out and why, and per engine (PDFium, the fork, upstream) how many tests pass each criterion. |
| `conformance/pdfium-pixel-results.json.xz` | Per test and page: expected pages, suppression, and per engine the comparison with the expected PNG. |
| `conformance/pdfium-pixel-tests.tsv` | One line per test: each engine's result, and for the fork's failures at 1% the group they were put in by eye. |

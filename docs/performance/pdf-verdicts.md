# Which is right: PDFium or Shiori's renderer (hayro fork a2e9e62b)?

Part of [pdf.md](pdf.md#which-drawing-is-right).

This covers the 50 PDFium pixel tests (chromium/8110) where the fork differs from PDFium's expected image on more than 1% of pixels. Each case was judged against ISO 32000-2, with 32000-1 for wording. MuPDF 1.27 and Poppler 26.09 (splash and cairo), both at 72 dpi with the test fonts, served as tie-breakers, not votes. The case-by-case rows (verdict, confidence, hayro's cause, the reasoning and the spec sections) are in [`conformance/pdfium-pixel-verdicts.tsv`](data/pdf/conformance/pdfium-pixel-verdicts.tsv). A verdict is a reading of the spec for one case, not a reference rendering, so each carries a confidence. Wherever a hayro cause is marked as confirmed, it was checked by editing the input (for example, blanking an xref key) and re-rendering.

## Counts

| Verdict | All | High | Medium | Low |
|---|---|---|---|---|
| PDFium more correct | 19 | 9 | 9 | 1 |
| hayro fork more correct | 6 | 5 | 1 | 0 |
| Both acceptable | 24 | 8 | 16 | 0 |
| Both wrong | 1 | 1 | 0 | 0 |
| **Total** | 50 | 23 | 26 | 1 |

In 19 of the 24 "both acceptable" cases the input is malformed and the spec defines no result. That includes the 7 pages with no MediaBox, which is required. The other 5 are plain implementation choices: hinting and anti-aliasing, font substitution, and whether to generate an appearance that isn't required.

## hayro causes, by how many cases each explains

**Where hayro is less correct** (the "PDFium more correct" and "both wrong" cases):

| Cases | Cause | Tests |
|---|---|---|
| 3 | **The xref chain is thrown away when it revisits a section.** In `populate_xref_impl_inner` the visited check returns `None`, and `populate_from_xref_stream` and `populate_from_xref_table` propagate it with `?`. A hybrid file's `/XRefStm` stream with its own `/Prev`, or two updates naming the same `/XRefStm`, therefore discards the whole parsed xref. The scan-based rebuild then picks bait or stale objects. All three confirmed. | bug_1484283, bug_335309995, bug_332462378 |
| 3 | **`stream` must be followed directly by its EOL.** `parse_proper` and `parse_fallback` in `object/stream.rs` require this, so a space or junk before the EOL drops the page's only content stream, even though /Length is exact. | bug_543018_1, bug_543018_2, bug_551258_1 |
| 3 (of 5) | **No appearance generation.** `interpret_annotation` draws only `/AP /N`. It ignores AcroForm `/NeedAppearances` and never builds FreeText or widget appearances from DA, V or Contents. checkboxes and text_form_custom_font have the same cause but count as both acceptable, because nothing there requires generating an appearance. | bug_733528, freetext_annotation_with_da, freetext_annotation_without_da |
| 2 (of 5) | **Image data disagreeing with the image dictionary is stretched or reinterpreted.** `fix_image_length` shrinks Height to the rows present, and `decode_context`'s scale_factors stretch the decoded size over the dictionary's. bug_554151, bug_603518 and jpxdecode_with_mismatch_colorspace have the same cause but count as both acceptable, because their data is corrupt. | bug_1395648, bug_1478366 |
| 2 (of 3) | **Wrong /Length recovery ends the stream at the first raw byte match of `endstream`.** `parse_fallback` uses memmem and doesn't check the token delimiter, so it stops inside `eendstream` or `endstream.`. On bug_524043_5 (`endstream%`, a real keyword) the same cause counts as both acceptable. | bug_524043_3, bug_524043_4 |
| 1 | Type 4 `round` uses `f32::round` (-5.5 gives -6) instead of PostScript round, which rounds ties up (-5.5 gives -5). | bug_1321 |
| 1 | A closed degenerate subpath (`m h`, or `m l h` at one point) with round caps paints no dot (8.5.3.2). | bug_1983 |
| 1 | `Indexed::new` rejects the whole colour space when the lookup string is short, even though every index used has an entry. | bug_372749732 |
| 1 | One unparsable operand (`[0.000001, 0.000001]`, with a comma) ends the content stream, so the valid operators after it are lost. Separately, a valid 1e-6 dash takes 16 s and stops halfway along the line. | bug_467170761 |
| 1 | RunLengthDecode discards complete data when there's no EOD byte (`read_byte()?` in `run_length::decode`). | bug_512557 |
| 1 | `b`/`B` fill and stroke aren't composited as a knockout group (11.7.4.4): `fill_stroke_path` paints them as two objects. | same_color_knockout_fill |
| 1 | Mesh shadings in an Indexed space interpolate the index rather than the base-space colour (8.7.4.4.1). | shade (both wrong) |

**Choices and lenient recovery** (both acceptable; a strict mode may want to change them):

- **7 cases: A4 when there's no MediaBox.** `page.rs` uses `unwrap_or(A4)`; PDFium, MuPDF and Poppler use US Letter. bug_527174, bug_551460, bug_554151, bug_71, bug_714187, bug_981347, direct_content_stream.
- **3 cases: substitutes for non-embedded fonts.** bug_736703, bug_846, bug_42270459.
- **2 cases: unrecoverable damaged files are refused** (`LoadPdfError::Invalid`), where PDFium opens a blank page. bug_828206, bug_830221.
- **Malformed input accepted leniently, one case each:**
  - a direct (non-indirect) content stream is drawn: direct_content_stream;
  - `--10.12` is lexed as `-` then `-10.12`: bug_432208037;
  - a corrupt JPX is drawn as grey noise: bug_557223;
  - SCN on an invalid pattern falls back to plain black: bug_1099446;
  - a negative dash element is passed straight to kurbo: dashed_lines.
- **Malformed input rejected, one case each:**
  - a stream dictionary containing `{...}}` is dropped: bug_1546;
  - a fuzzed JBIG2 stream is dropped: bug_867501.
- **1 case: hinting and anti-aliasing only.** bug_1922.

## PDFium bugs (hayro fork more correct, plus one half of "both wrong")

- **bug_1693:** pattern fills ignore the constant alpha (11.6.7). PDFium's own commit says its expected image shows the current incorrect result.
- **bug_1746:** `ca` is applied twice to an image mask inside a Type 3 glyph, giving 25% instead of 50%.
- **bug_1966:** an upscaled image mask is smoothed even though there's no /Interpolate, and 8.9.5.3 says interpolation "shall be disabled by default".
- **image_transformer_other:** an out-of-range Indexed sample is painted black instead of being clamped to hival (8.6.6.3, 8.9.5.2).
- **radial_shading_point_at_border:** the AGG renderer fills a region that no blend circle covers. PDFium's own Skia expected image leaves it white.
- **shade-tensor:** about 45% of the area of the self-overlapping type 6/7 patches is missing (8.7.4.5.7).
- **shade:** CIE-based (Lab) mesh colours are blended in RGB (8.7.4.4.1). This is PDFium's half of the "both wrong" verdict.
- **A quirk rather than a rendering bug:** on bug_733528, PDFium merges two same-named fields that have different values.

## Where the first grouping in pdfium-pixel-tests.tsv was wrong (13)

- **File structure, not page size or drawing:**
  - bug_1484283 and bug_335309995 (the xref-chain abort; their pages get the wrong MediaBox);
  - bug_332462378 (the same abort; a decoy object is drawn).
- **Bytes after the `stream` keyword, not a wrong /Length.** /Length is correct in all three: bug_543018_1, bug_543018_2, bug_551258_1.
- **Malformed stream dictionary, not a drawing difference:** bug_1546.
- **Not edge differences:**
  - bug_1966 is image-mask interpolation;
  - bug_736703 and bug_846 are font substitution;
  - dashed_lines is an invalid negative dash element.
  - Only bug_1922 of the five "edges" cases really is edges.
- **More than page size:**
  - bug_554151: PDFium fills the missing image data black, while hayro draws nothing;
  - direct_content_stream: hayro draws an illegal direct content stream that PDFium ignores.

Borderline: bug_467170761 is better described as a content-stream syntax error than as "drawn differently".

## Uncertain

- **bug_524043_3 and _4:** the verdict assumes recovery should look for the real `endstream` keyword. Honouring the wrong /Length literally would also lose the second line.
- **freetext_annotation_without_da (low):** with neither AP nor DA, any appearance is invented.
- **Medium:**
  - bug_42270459 and bug_432208037: PDFium claims to follow Acrobat here.
  - direct_content_stream and bug_1546: in both, the spec forbids the construct without defining a rendering.
  - bug_1966: the 8.9.5.3 note lets a reader implement interpolation as it wishes.
- **Corrupt-image cases** (bug_557223, bug_603518, bug_867501, jpxdecode_with_mismatch_colorspace): all are "both acceptable". PDFium drops the image in most of them, and that's the behaviour a strict mode should copy.

## Seeing them

[`divergence_gallery.py`](../../tests/performance/pdf/conformance/divergence_gallery.py)
draws each of the 50 side by side (PDFium's expected image, the fork's
drawing, and where they differ, with the densest cluster of differences
circled in red), from the same programs, settings and fonts as the pixel-test
run. [`divergence_verdicts.py`](../../tests/performance/pdf/conformance/divergence_verdicts.py)
then groups that page by verdict and puts each case's reasoning under its
images. The images themselves aren't kept here; they take a minute to
remake.

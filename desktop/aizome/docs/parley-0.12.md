# Moving Sukumo to Parley 0.12

Sukumo lays text out with Parley and finds fonts with Fontique, both at
0.11.1. Aizome's workspace builds the two from
[bitemyapp/parley](https://github.com/bitemyapp/parley)'s `sekka-0.11.1`
branch: the published 0.11.1 with two changes that save work
([DEVELOPERS.md](../../../DEVELOPERS.md) says what they are and how they
were checked).

That branch is a stopgap. Parley and Fontique 0.12.0 came out on
2026-10-09, and nothing upstream fixes from then on reaches a branch of
0.11.1. What is owed is the move to 0.12, with everything that makes new
text cheap brought along, so that Sekka carries no fork of Parley or as
small a one as it can.

None of it is done. This says what has to come over, and what a first look
at 0.12.0 found on 2026-11-09.

## What has to come over

| What saves the work | Where it is now | On 0.12 |
|---|---|---|
| A digit isn't looked up as an emoji | The branch's change to Parley | Upstream has its own: 0.12 no longer takes a lone digit for an emoji (linebender/parley#811), so the change goes. `#` and `*`, which the change also covers, have not been tried on 0.12. |
| A query's families are expanded as matching reaches them | The branch's change to Fontique | Not upstream, and the commit doesn't apply: 0.12's `matches_with` hands out borrowed fonts. Port it, or have upstream take it. A report for linebender/parley is drafted and not filed. |
| Left-to-right text is laid out without a direction mark | Sidestep, `text/layout.rs` | Sidestep's own, so it comes over when that file is ported. `text_that_runs_one_way_needs_no_mark_to_say_so` tests it. |
| Text whose own family has every character is set in that family alone | Sidestep, `fonts::covers` and `text/layout.rs` | Sidestep's own. It matters more on 0.12, where a generic family in the stack costs more than it did (below). `a_family_with_every_character_needs_none_after_it` tests it. |
| parlance and parley_data keep coming from crates.io | The branch's last commit | Only a git patch of Parley's tree needs it. It goes with the branch, or is made again if 0.12 is carried from one. |

## What a first look found

A bounded look, not a qualification: published 0.12.0 beside 0.11.1 and
the branch in one program on the system's fonts, on a shared workstation
(a Threadripper PRO 9985WX), one run of passes taken in turn. It times
Parley itself, not Sukumo, and no corpus was drawn with 0.12.

Microseconds for a layout of text nothing has laid out:

| Font stack | Text | 0.11.1 | 0.11.1 from the branch | 0.12.0 |
|---|---|---|---|---|
| `Noto Sans` | `item abcde` | 3.06 | 3.10 | 3.38 |
| `Noto Sans` | `item 00012` | 8.36 | 3.07 | 3.28 |
| `Noto Sans` | `a 😀 b 😀 c 😀` | 9.01 | 6.09 | 9.74 |
| `Noto Sans, sans-serif` | `item abcde` | 4.14 | 3.16 | 5.50 |
| `Noto Sans, sans-serif` | `item 00012` | 16.08 | 3.12 | 5.48 |
| `Noto Sans, sans-serif` | `項目 00012` | 16.14 | 3.72 | 6.29 |
| `Noto Sans, sans-serif` | `a 😀 b 😀 c 😀` | 16.39 | 6.28 | 17.51 |
| `system-ui, emoji` | `a 😀 b 😀 c 😀` | 20.83 | 6.98 | 22.28 |

- **Digits cost what letters do on 0.12**, with nothing patched.
- **With one named family**, as Sidestep sets text its own family has
  every character of, 0.12 is within about a tenth of the branch for text
  without an emoji, and takes 1.4 to 1.6 times as long with one.
- **With a generic family in the stack**, as Sidestep's has whenever the
  text's own family lacks a character, 0.12 takes 1.5 to 2.8 times what the
  branch does in `Noto Sans, sans-serif`, and up to 5 times with a generic
  family alone. Part of that is the Fontique change, which 0.12 doesn't
  have, and part is the next point.
- **Text without digits is slower on 0.12 than on the published 0.11.1**:
  by about a tenth with one named family, and with a generic one by a
  third or more for plain Latin (4.14 to 5.50 above) and by 1 to 9% for
  text with Han or an emoji. Neither has the Fontique change, so this is
  something else in 0.12. It has not been profiled.

So 0.12 alone gives the digits and gives back part of the rest. Whether
that is worth carrying a change to Fontique on 0.12 is to be decided with
Sukumo's own numbers, once it builds.

## The port

`cargo check -p sidestep-engine` against 0.12.0 stops at 17 errors, 15 in
`text/layout.rs` and 2 in `text/glyphs.rs`, of five kinds:

- `LineMetrics::trailing_whitespace` is gone (4);
- a layout's inline boxes come as an iterator, not a slice (7);
- `InlineBox` has two more fields, `baseline` and `vertical_align` (2);
- `Run::metrics` is gone (1);
- normalized variation coordinates are `NormalizedCoord`, not `i16` (3).

That is a first pass: more may show once these are fixed, and the crates
that use the engine were not reached. 0.12.0 keeps harfrust 0.12, skrifa
0.44, read-fonts 0.41 and parlance 0.1.1, and brings parley_engine,
parley_emoji and parley_data 0.12 (with icu 2.3), which are new to the
lock files and to what Nix vendors.

Five lock files name Parley: Aizome's, Sukumo's own, Genkan's
(`installer/genkan/ui`) and the two performance kits'
(`tests/performance/design/hello/kit` and
`tests/performance/why-toolkits/apps/kit`). Sukumo's and Genkan's build
the published crates today; the other three build the branch.

## Showing that it draws right

The branch could be shown to change nothing: the same pixels and the same
glyphs from the same font files
([record](../../../docs/test-results/parley-fork-2026-11-09.json)). The
move to 0.12 can't be shown that way. #811 changed how Parley settles
whether a cluster is drawn as an emoji, which is how the digits were
fixed, so some text may get another font than it gets now, and nobody
knows yet which.

- **From 0.11.1 to 0.12:** draw the record's corpus with both (101 strings
  in 21 fonts and six layouts, and the engine's glyph runs) and account for
  every drawing that differs: wanted, harmless, or to be fixed.
- **Anything carried on top of 0.12:** published and patched 0.12 side by
  side in one program on random layouts and font queries, with
  deliberately wrong versions as controls, as the record did for 0.11.1.
  That part has to change nothing again.
- **The cost:** the record's timings for a new layout, and the tables
  under "Measured" in [kit.md](kit.md) (`tests/scale.rs`), before and
  after.

The corpus and the side-by-side program were scratch code and are not in
the tree. The record describes both closely enough to write them again;
kept in the tree, they would serve the next Parley as well.

## In what order

1. Port Sidestep to the published 0.12 in Sukumo's own workspace, which
   has no patch. Draw the corpus and settle the differences.
2. Measure. If text that needs a generic family costs what the first look
   saw, port the Fontique change to 0.12, show it changes nothing, and
   file the report upstream. Carry it from a branch of 0.12 until upstream
   has it, or decide the cost is small enough to live with.
3. Move the five workspaces, take the `sekka-0.11.1` lines out of the
   three `[patch.crates-io]` tables and the hash out of `outputHashes`,
   and bring DEVELOPERS.md and kit.md up to date.

Until then the branch gets nothing from upstream by itself. A fix Sekka
needs from a later Parley has to be picked onto it by hand, and
DEVELOPERS.md says how to move the branch.

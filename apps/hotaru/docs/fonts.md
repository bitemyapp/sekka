# Terminal fonts and grid shaping

This is the October 28, 2026 source audit and proposed next implementation,
not a claim that the font port is complete. The behavioral reference is
Ghostty main `6ba144158577d033ccb6ed4b574e4013957c9161` (`1.4.1-dev`).
The audit used the pinned source and the actual Rust dependency sources;
no product code was changed for this document. See [the port ledger](upstream.md)
and [the GUI probe's qualifications](gui-probe.md).

## Current behavior and gaps

| Area | Hotaru and Sidestep today | Pinned Ghostty Linux behavior |
| --- | --- | --- |
| Primary family | Hotaru uses Kit's fixed `FontSpec::mono`, normally IBM Plex Mono. Kit's family enum contains only UI, Mono and Symbols. | Ordered explicit family lists, including separate style lists; absent styles fall back to the regular family and can be synthesized. |
| Font discovery | Sidestep uses fontique's shared collection and fontconfig. It also supports process-local registered files. Fallback is present, but Hotaru cannot configure its family order. | Fontconfig sorted discovery, deferred face loading, cached codepoint/style/presentation resolution, explicit and discovered fallback faces. |
| Emoji | Sidestep already shapes emoji and rasterizes color glyphs through swash. Its normal paragraph path prefers generic Emoji over the requested face. | Explicitly selected faces can override default presentation; VS15/VS16 explicitly select text/emoji. Linux embeds Noto Color Emoji and Noto Emoji. |
| Font availability | The standalone Aizome bundle contains Archivo, IBM Plex Mono and Noto Sans Symbols 2. It contains neither CJK nor emoji. Hotaru's Nix test fontconfig includes CJK, but its runtime wrapper only supplies the Aizome bundle. | Embedded text and emoji fallbacks provide a useful baseline even with restricted system fonts. CJK still requires a suitable available font. |
| Size | Hotaru's configured size is passed directly as logical drawing units. | Points are converted with `points * dpi / 72`; GTK combines output scale and the desktop text DPI scale. |
| Grid metrics | Hotaru measures `M`; width is its natural advance. Height is `max(ceil(size * 1.5), ceil(measured_height))`. | Primary-face metrics determine the grid. FreeType measures maximum ASCII advance; grid width/height and baseline are rounded in physical pixels. Decorations have face-derived metrics. |
| Scale changes | Sidestep emits `WindowEvent::ScaleChanged(f64)`. Kit currently ignores that event and does not retain scale in its window/layout environment. | Font size and physical metrics follow content scale; grid size and pixel reporting are updated together. |
| Shaping | Hotaru concatenates adjacent equally styled cells into a paragraph layout, then clips the natural glyph positions to the run's cell rectangle. Paragraph bidi and fallback advances can move following glyphs away from their terminal columns. | HarfBuzz is forced LTR, with character-level clusters retaining original terminal columns. Glyph offsets stay relative to the cell/ligature cluster rather than accumulating unrelated fallback advances. |
| Ligatures | Parley's ordinary shaping can form ligatures, but Hotaru has no feature controls or cursor-aware run boundary. | Default `liga=1`, configurable OpenType features, run boundaries at relevant font/style/selection/cursor changes, preserving multi-codepoint graphemes. |
| Rendering | Sidestep caches swash masks/color images and composites damaged regions into Wayland shared-memory buffers. | Separate grayscale/color glyph atlases and the Linux OpenGL renderer. Emoji glyphs are fitted and centered in their cells. |
| Terminal symbols | Hotaru relies on ordinary font glyphs. | Generated box, block, braille, Powerline and other sprite glyphs enforce grid geometry. |

Missing emoji boxes in the retained GUI probe demonstrate absent effective
fallback in that environment. They do not demonstrate an absent color rasterizer.
Sidestep's rasterizer has COLR and CBDT/sbix support already. It disables hinting
in the current swash scaler; pixel-for-pixel agreement with FreeType is not an
assumption of this plan.

Installing an emoji file alone is insufficient: Kit only loads its three known
families, and Sidestep's generic `Font::install` registers families without
assigning a generic Emoji family. A terminal font set must name its bundled
fallbacks explicitly. Do not replace the host's fontconfig configuration with a
restricted application configuration and thereby hide the user's fonts.

Sidestep's `emoji_ranges` and generic-Emoji precedence also require explicit
VS15/VS16 and user-family tests. For a default-emoji scalar followed by VS15,
the current range classifier begins an emoji range without consulting VS15.
The exact resulting glyph depends on the subsequent shaper and available faces;
source inspection alone does not certify the visible result.

## Narrow next implementation

Keep Aizome's native windows, tabs, splits, menus, accessibility and controls.
Add a terminal-specific grid shaping module to Sidestep, sharing its existing
font registry, discovery infrastructure and raster cache. The output should
describe glyph identities and positions, independent of software or GPU drawing.
Do not fork the terminal core into the UI toolkit or make the toolkit understand
Hotaru's packed cells.

The first slice has four connected parts:

1. Resolve an immutable terminal font configuration, including primary/style
   families, explicit text/emoji/CJK fallbacks and OpenType features. Load and
   prepare fonts on a worker. Preserve system fontconfig discovery and cache
   both successful and missing glyph resolutions.
2. Derive one grid from the primary face, point size and actual window scale.
   Use physical-pixel rounding and a common baseline. Fallback faces cannot
   change row height. Propagate native scale changes through Kit and relayout
   the terminal, using the existing coalescing PTY resize control mailbox.
3. Shape terminal runs with reusable harfrust buffers whose clusters map to
   terminal columns. Preserve combining sequences, ZWJ emoji and ligatures.
   Place each independent cluster at its cell origin, retaining offsets inside
   a ligature cluster. Use glyph metrics to fit/center emoji within the reserved
   cell span. Keep backgrounds and selection geometry in cell coordinates.
4. Adapt Hotaru's row-damage renderer and persistent font settings to that API.
   Expose the requested and resolved family in diagnostics so missing-family
   fallback is observable. Keep current bounded PTY/clipboard behavior intact.

Ship an application-specific fallback bundle or explicit installed-file paths
for Noto Color Emoji and a chosen CJK face, preserving their licenses. The
Aizome chrome font bundle need not expand for every native app. The exact CJK
face/region and closure size must be recorded with deterministic fixtures.

Full sprite generation, every Ghostty font configuration option, cursor visual
parity and a native GPU renderer remain separate follow-up work. An equal-pixel
rendering benchmark is required before claiming whole-application efficiency.
The existing CPU backend is useful for correctness but cannot establish that
claim by itself.

## Proposed API boundary

The names below are design names, not existing exported APIs. Final Rust types
should minimize mutable shared state and should not expose raw glyph pointers.

| Proposed type/operation | Responsibility |
| --- | --- |
| `GridFontConfig` | Owned, validated family lists, style preferences, point size, features and fallback policy. Keep a stable configuration revision for cache invalidation. |
| `GridFontSet::resolve(config)` | Worker-side discovery and owned face data. Report requested/resolved family identities and unavailable requested families. No terminal or window handle is retained. |
| `GridMetrics` | Physical cell width/height, baseline, underline/strike metrics and the scale used; provide explicit conversion to logical coordinates. Distinguish physical pixels, logical coordinates and typographic points. |
| `GridCell` | Borrowed grapheme text, starting column and reserved width. Width comes from terminal state; the shaper never recomputes terminal character width. |
| `GridRun` | A slice of cells with one shaping style and boundaries already selected by the application. A run includes no PTY, clipboard, widget or terminal-core type. |
| `GridShaper` | Worker-owned font resolution and shape caches, reusable scratch buffer, and cancellation checked between bounded runs. |
| `GridTextLayout` | Immutable owned glyph runs with face identity, font size, cell-relative glyph positions and diagnostic cluster spans; no dependence on the current rasterizer. |
| `Canvas::draw_grid_text` | Native adapter that emits existing glyph drawing operations. A future GPU adapter can consume the same layout. |

Do not shape each scalar independently: doing so would lose combining mark
positioning and ligatures, and multiply shaping overhead. Do not stretch a
whole paragraph to its expected width: that hides drift while distorting text
and still mishandles individual wide cells.

Font resolution and shape results need bounded caches. Cache keys must include
font configuration/face/variation identity, features, size/scale and cell mapping.
Colors can be applied at drawing time when they do not affect shaping. A hash
match alone must not be treated as text equality. Retain full keys or compare
the original run on collision. Reuse allocations between shape calls and count
retained bytes as well as entries before choosing final limits.

Keep expensive discovery and cold shaping away from the PTY actor and UI event
thread. A bounded worker can coalesce pending visible frames by pane. Completed
frames must carry pane/session, viewport and font/geometry revisions. Reject
results from closed panes or changed geometry. Do not reject every older output
generation during a continuous stream, which could starve presentation; present
a coherent completed snapshot and request the latest one. Selection hit testing
must refer to the snapshot actually displayed. All queue/retirement work should
avoid holding a UI mutex while formatting, decoding history or releasing large
payloads.

## Mapping to actual dependencies

These APIs were inspected in the pinned local Sidestep source and the versions
resolved by Aizome's lockfile: fontique/Parley `0.11.1`, harfrust `0.12.0`,
skrifa `0.44` and swash `0.2.10`.

| Requirement | Existing API and integration point |
| --- | --- |
| Shared font source | `sidestep_engine::text::fonts::shared().font_context()` clones a `parley::FontContext` whose collection/source cache share font data. `text::with_ctx` supplies thread-local layout state; a new worker-owned grid state can reuse the source without borrowing UI state. |
| Family query | fontique `Collection::query(&mut SourceCache)`, `Query::set_families`, `set_attributes`, `set_fallbacks` and `matches_with` expose ordered candidate fonts. `QueryFont` owns/clones its blob, face index and synthesis. Candidate coverage still needs checking; a family match is not proof that every scalar/variation selector in a grapheme is covered. |
| Existing resolved face | Sidestep `fonts::Face` owns optional `FontData`, variation settings, normalized metrics and family identities. Its public `Font` wrapper hides these internals; the new grid module should live inside the toolkit rather than obtain them through an unsafe escape hatch. |
| Cached shaping tables | `harfrust::ShaperData::new(&FontRef)` owns parsed shaping caches. Store it with owned font bytes/index. Recreate the cheap borrowing `FontRef`/shaper on use instead of building a self-referential struct. |
| Variable instance | `ShaperInstance::from_variations` or `from_coords`, then `ShaperData::shaper(&font).instance(Some(&instance)).build()`. Shaping and rasterization must use the same normalized coordinates. |
| Terminal clusters | `UnicodeBuffer::add(char, cluster)`, `set_direction(Direction::LeftToRight)`, `set_cluster_level(BufferClusterLevel::Characters)`. Keep a separate input-codepoint-to-cell mapping, as the reference does, so reordered mark glyphs and swallowed ligature inputs remain interpretable. |
| Shape scale/features | `Shaper::shape(buffer, ShapeOptions::new().scale(Some(scale)).features(&features).point_size(Some(points)))`. A 26.6 physical-pixel scale makes returned offsets comparable to the reference; validate rounding and overflow before conversion. |
| Buffer reuse | `GlyphBuffer::glyph_infos()`/`glyph_positions()` expose the owned result, and `GlyphBuffer::clear()` returns a reusable `UnicodeBuffer`. Keep buffer and scratch capacity bounded for hostile or unusually wide rows. |
| Metrics and coverage | skrifa provides `MetadataProvider`, `charmap`, normalized variation `Location` and `glyph_metrics`. Sidestep already obtains font metrics this way. Add terminal ASCII advance/primary baseline calculations rather than measuring a general paragraph. FreeType's hinted values can differ, so compare fixture results explicitly. |
| Stable draw identity | `fonts::register(&FontData, &[i16], Synth) -> u32` registers owned face data for render threads. `face_data(id)` returns owned data. Existing ids are process-stable; the registry retains faces, so avoid registering arbitrary unbounded synthesis/variation combinations. |
| Draw representation | `protocol::Glyph { id, x, y }` and `GlyphRun { font, size, x, y, glyphs: Arc<[Glyph]>, color, clip }` already separate shape output from rasterization. `Canvas::emit_text` currently accepts internal placed runs privately; a typed grid draw method can reuse its transform/clip path. |
| Raster/cache | `text::raster` already caches grayscale masks and premultiplied color images by face, physical size and glyph/subpixel offset, shared across raster workers. Reuse it for the first slice. |
| Scale propagation | Native `WindowEvent::ScaleChanged` exists. Kit's `app.rs` dispatch must call a new core scale update; `Win` and widget `Env` need scale, and `LayoutCx` needs a read accessor. The update must invalidate geometry and frame caches even when the logical window dimensions are unchanged. Tests and offscreen render paths must provide the same scale. |

Parley's ordinary paragraph API is still appropriate for chrome. Its public
`TextLayout` intentionally hides glyph cluster data, and setting paragraph
direction alone is not the same as forcing Ghostty's LTR shaping order. A direct
harfrust path reuses the existing dependency while making terminal cluster
behavior explicit. Any adapted Ghostty shaping algorithm or tests must retain
the upstream MIT attribution already recorded in the port ledger.

## Verification before acceptance

Use the same pinned font files in Hotaru and the untouched Linux Ghostty
reference. Record font hashes, resolved faces, size in points, physical scale,
cell dimensions and binary/source identities alongside results.

- Check every ASCII cell origin and the primary baseline at scales 1, 1.25
  and 2; test scale changes without logical resize and verify final PTY pixel
  and row/column geometry.
- Mix ASCII with CJK, combining marks and fallback symbols; assert the next
  ASCII cell stays at the expected column. Include wide cells at either edge.
- Cover VS15/VS16, skin tone, ZWJ, keycap and regional flags with explicit
  family override and bundled fallback. Assert selected face/glyph identities
  and inspect color pixels; a nonempty image alone does not prove correct emoji.
- Use a pinned ligature font for `!=`, `->`, `===`, combining marks and
  script-specific reorder cases. Verify feature disabling and cursor/selection
  boundaries without splitting a multi-codepoint terminal grapheme.
- Verify bold/italic fallback and synthesis, missing-family diagnostics,
  configuration reload, cancellation, pane closure, bounded cache growth and
  repeated font/scale changes. Preserve the existing Kit and Hako suites.
- Run real private Wayland tests against the exact Nix package with ambient
  font/library overrides cleared. Prove packaged emoji/CJK discovery instead
  of accidentally using the workstation's installed fonts.
- Measure cold/warm shaping, allocations, changed-row frame cost and final
  frame presentation at equal physical grids. Report software and hardware-GPU
  workloads separately. DSR completion alone is a parser test, not a completed
  rendering benchmark.

## Source pointers

Hotaru: `desktop/aizome/apps/hotaru/src/terminal_view.rs` and `settings.rs`.
Kit: `crates/aizome-kit/src/{theme,text,fonts,widget,engine,app}.rs` under
`desktop/aizome`. Packaging: `desktop/aizome/nix/{aizome-fonts,hotaru}.nix`.
Sidestep: `crates/sidestep-engine/src/text/{fonts,layout,glyphs,raster}.rs`,
`text.rs`, `protocol.rs`, `backend/mod.rs`, and
`crates/sidestep-ui/src/{text,canvas,event}.rs` under `vendor/sidestep`.

Pinned Ghostty: `src/font/{Collection,SharedGrid,Metrics,discovery,embedded}.zig`,
`src/font/face/freetype.zig`, `src/font/shaper/{harfbuzz,run,feature,Cache}.zig`,
`src/font/sprite/`, `src/config/Config.zig`, and
`src/apprt/gtk/class/surface.zig`. The GTK source supplies behavioral reference
for Linux scale semantics; GTK itself is not a Hotaru dependency.

The intended new toolkit patch is `upstream/sidestep-grid-font.patch`, applied
after the existing clipboard, input and MIME patches. Those earlier patch bytes
must remain unchanged. Packaging and development-hook integration must validate
the ordered series, including dependent changes to the same files.

# Cursor planes after #276: what's left, and the options

#326 stops a larger pointer size from crashing the compositor (#276). This
page records how the crash happened, what the fix still leaves to care rather
than to the compiler, and the ways to follow up, with their costs. The
Smithay side of the bug, and the fix sent there, is in
`docs/upstream/smithay/README.md` (#327).

Line numbers are at `730ba1ba` for Aizome's code and at Smithay 0.7.0 for
Smithay's. None of the follow-ups below has been written or built yet.

## How the crash happens

When `DrmCompositor` can put a `Kind::Cursor` element on a KMS cursor plane,
it first tries to copy the element's pixels straight into the plane's buffer:
`copy_element_to_cursor_bo` (`backend/drm/compositor/mod.rs:4178`). It takes
that path when the element's source starts at the origin, is drawn 1:1, and
neither the element nor the output is transformed. It then copies the rows of
the whole underlying buffer, at the buffer's own stride and height, not the
rows of the source. If the source is smaller than the buffer, the rows run
past the plane's mapping and the slice index panics.

Aizome reached this through `MemoryRenderBufferRenderElement::from_buffer`
(`backend/renderer/element/memory.rs:497–501`). Given a `size` and no `src`,
it takes the source to be `size` at the origin. Before #326, `render.rs`
passed the pointer size Settings asked for along with the theme's nearest
image. For 64 pt with Adwaita, that was a 72 px buffer whose source was its
top-left 64 px: a crop at the origin, drawn 1:1, which is exactly the fast
path's case. On a 64×64 cursor plane, which i915 offers first, every login
crashed.

Client cursors can reach the same copy in two ways. A `wp_viewport` can show
only the top-left part of a larger shm buffer. An shm buffer can also have a
stride wider than its pixels, and a wider stride than the plane's.

## What #326 does

- Theme cursors and the drawn arrow: every image is whole points at the
  output's integer scale, with a buffer of exactly that many pixels, built by
  `CursorImage::new` (`cursor.rs:32`). `render.rs` passes `img.size`, which
  now equals the buffer's size.
- Client cursors: their surfaces are `Kind::Unspecified` (`render.rs:256`),
  so they are composited on the primary plane and never reach the copy.

## What is still left to care

1. **Two sizes that must agree.** `render.rs:244` passes `size: Some(img.size)`
   next to a buffer that already has a size. Only `CursorImage::new` keeps them
   equal, and `CursorImage`'s fields are public (`cursor.rs:21–26`), so code
   elsewhere could build one that disagrees. A disagreement is a crop, and a
   crop is the crash.
2. **The pixel length is a `debug_assert`** (`cursor.rs:34`). In a release
   build, a short slice reaches Smithay's `assert!` in `MemoryBuffer::from_slice`
   (`memory.rs:165`) and panics the compositor; a long one is accepted.
3. **Any scene item can claim `Kind::Cursor`.** `kind` is a free field on
   `SceneItem::Surface` and `SceneItem::Memory` (`scene.rs:30` and `:40`).
   14 of the 15 places that build either pass `Kind::Unspecified`, including
   the logic library's (`aizome-comp-logic`: `overlays/mod.rs:248`,
   `render.rs:201`, `slides.rs:264`). Nothing stops one of them passing
   `Kind::Cursor` with a crop.
4. **Same-typed arguments side by side.** `fit(images, size, scale)` and
   `arrow(size, scale)` take two `u32`s that swap without complaint, and
   `resample` takes two `(usize, usize)` pairs. The integer scale is worked out
   as `scale.ceil().max(1.0) as u32` in five places (`render.rs:236`,
   `icons.rs:123`, `painter.rs:167`, `recovery/mod.rs:344`,
   `recovery/view.rs:178`).
5. **Client cursors lost the hardware cursor.** A cursor a client draws
   (X11 apps through xwayland-satellite, games, apps without cursor-shape) is
   now redrawn into the primary plane whenever it moves. Over a fullscreen
   window it should also stop direct scanout of that window, since an shm
   cursor can't go on an overlay plane above it. That follows from how Smithay
   assigns planes and has not been measured.

## Options

The theme cursor path and the client cursor path can be followed up
separately.

### Theme cursors

**T1. Leave it as #326 has it.** The sizes are right and the unit test
(`every_pointer_size_is_its_whole_buffer`) checks the condition the copy
depends on, for every Settings size at 1×, 2× and 3×. Items 1 to 4 above
stay: the invariant holds because each place that builds a cursor is careful,
and a later change that isn't careful compiles.

**T2. Make "the whole buffer, drawn 1:1" a type.**

- Add `SceneItem::Cursor { image: CursorImage, loc }`. Its arm in
  `scene_elements` is the only `Kind::Cursor` in Aizome, and it calls
  `from_buffer` with neither `src` nor `size`. Smithay then takes both from the
  buffer itself (`memory.rs:497–501`), so there is no second number to
  disagree. (With no `size`, even the image picking from before #326 would
  have drawn a 72 pt pointer rather than crashed.)
- Remove `kind` from `SceneItem::Surface` and `SceneItem::Memory`; every other
  constructor passes `Kind::Unspecified` today.
- Make `CursorImage`'s fields private, with one constructor that returns
  `None` unless the pixel size is a whole multiple of the scale. Smithay's
  `to_logical` divides by the scale and truncates, so 45 px at 2× would be
  22 pt: a crop again. `CursorTheme::get` already falls back to the arrow.
- An `Argb8888 { size: Size<i32, Buffer>, bytes }` that checks its length once,
  when it's built, with `resample` as its method taking a `Size<i32, Buffer>`.
  This replaces the `debug_assert` and the tuple pairs.
- An `IntScale(NonZeroU32)` with `IntScale::covering(f64)`, for the cursor
  functions and the five copies of the scale computation.
- `compile_fail` doctests, as for typestate: a `CursorImage { … }` literal
  outside `cursor.rs`, and `SceneItem::Memory { kind: … }`.

Cost: it touches all 15 scene constructors in the core and the logic
library, which the compiler lists. The core's ABI fingerprint changes, so a
logic library built before it won't load, as with any change to `SceneItem`.
It does nothing for client cursors.

### Client cursors

**C1. Keep them off the cursor plane (#326 as it is).** Safe and already
done. The cost is item 5, for as long as it stays.

**C2. Carry Smithay#2265 on 0.7.0.** Cherry-pick the fix
([Smithay#2265](https://github.com/Smithay/smithay/pull/2265), branch
`codex/cursor-copy-bounds` on bitemyapp/smithay, against `master`) onto a
bitemyapp/smithay branch cut at `v0.7.0`. Pin it under `[patch.crates-io]` in
`desktop/aizome/Cargo.toml` beside moxcms and zune-core, add its hash to
`nix/cargo-output-hashes.nix`, and make client cursors `Kind::Cursor` again.
Smithay then checks the source against the buffer and the plane itself, the
only place that knows the plane's stride. Aizome builds Smithay with
`renderer_pixman`, so a cursor the bounded copy refuses is still drawn onto
the cursor plane by pixman instead of going to the primary plane.

The patch applies to 0.7.0 with only a 19-line offset (`patch --dry-run`,
2026-11-06); it hasn't been built there. Cost: a fork of Aizome's largest
dependency, and a hash to update whenever its branch moves. It can go when
Aizome uses a Smithay release with the fix, but that release will come from
`master`, 513 commits past 0.7.0 on 2026-11-06, so the pin could stay a while.
The other fixes sent from Sekka (#327) and the GPU synchronization work (#281,
#317) could go on the same branch: one fork for all of them, which is also
harder to drop. The fix has unit tests upstream and has not run on a hardware
cursor plane.

**C3. Check each client cursor in Aizome.** In `cursor_items`, every frame,
walk the cursor's surface tree and make a proof only when every surface's
buffer is shm in ARGB8888 with a stride of 4 × its width, has no viewport
source or one covering the whole buffer, and has no buffer transform. A
`SceneItem::ClientCursor` takes the proof and is `Kind::Cursor`; without one,
the cursor stays `Kind::Unspecified`. Most client cursors get the hardware
cursor back with no fork.

Cost: it restates the conditions of a private Smithay function, so it has to
be checked again whenever Smithay changes the copy. It can't see the plane's
stride, so it must be stricter than Smithay needs to be. A cursor that fails
goes to the primary plane, not pixman on the cursor plane as in C2. It reads
the shm metadata of the cursor tree each frame.

**C4. Wait for a Smithay release.** C1 until Aizome moves to a Smithay
release with the fix. Nothing to do now. Client cursors stay off the cursor
plane until that migration, which is a large one.

### Recommendation

T2 either way: it makes the theme path safe because nothing else can be
built, not because each place is careful. For client cursors, C2 fixes the
copy where the bug is and keeps cursors on the plane even when the fast copy
can't be used; C3 only if carrying a Smithay fork is ruled out. C1 stands
until then.

## What has been checked

- The mechanism, by reading Smithay 0.7.0's `memory.rs` and
  `drm/compositor/mod.rs` on 2026-11-06.
- That Smithay#2265 applies to 0.7.0, with `patch --dry-run`; not built.
- #326's unit tests: for theme cursors and the arrow, the element's source is
  the whole buffer and it is drawn 1:1.
- Not checked: the crash on hardware, and any fix on a hardware cursor plane.
  No VM reaches a cursor plane: virtio-gpu, qxl and vmwgfx hide it from clients
  that don't set `DRM_CLIENT_CAP_CURSOR_PLANE_HOTSPOT`, which Smithay doesn't
  ([Smithay#2012](https://github.com/Smithay/smithay/issues/2012)).

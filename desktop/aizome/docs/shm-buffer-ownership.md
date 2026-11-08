# Giving wl_shm buffers back: a proposal to release by ownership

*Written by Claude (Opus 5.5) on behalf of Aidan Coyne (@raptros), as a
proposal for review. Nothing below has been built yet.*

Aizome gives a client's shared-memory buffer back early. Under GLES it sends
`wl_buffer.release` at commit time instead of waiting until the next commit
replaces the buffer, so a client can draw its next frame into the same buffer
and needs fewer buffers in its pool. But that release is sent by a code path
of Aizome's own, beside the one in Smithay that owns the buffer, and the two
don't know about each other.

This page explains the problems that come from that and proposes a different
design: each use of a buffer is released exactly once, by whichever comes first of
"the pixels have been copied" and "nothing holds the buffer any more". It is
meant to be read on its own, by someone deciding whether the change is worth
making.

Line numbers are at `462dd31f` for Aizome and at Smithay 0.7.0 for Smithay.
#355 (a fix for sync subsurfaces, below) is not merged at that commit.

## Background: what release means

A Wayland client draws into a `wl_buffer`, attaches it to a surface and
commits. From then on the compositor may read the buffer's memory, and the
client must not touch it until the compositor sends `wl_buffer.release`. A
client that keeps a pool of buffers (GTK, Firefox, Sidestep, most toolkits)
takes a release to mean "this one is free again" and draws into it next.

Two rules follow, and a compositor has to keep both:

1. **Never release a buffer while you may still read it.** If you do, the
   client draws over pixels you haven't copied yet, and a frame shows half of
   the next one (or nothing, if the client destroys the buffer).
2. **Release each use once.** A use starts when the client commits a buffer
   and ends with its release; attaching the same buffer again before that
   release doesn't start a second one. A client counts releases. A second one
   for the same use can tell it a buffer is free while it is in use, or make
   it free a buffer it is still holding.

When a buffer is released is up to the compositor. For `wl_shm` buffers,
once the pixels have been copied into a GPU texture nothing reads the
client's memory any more, so the buffer can go back straight away. Buffers a
renderer reads in place (dmabufs, or shm under a software renderer such as
pixman) have to wait until a newer buffer replaces them.

## How Aizome releases buffers today

Two separate mechanisms send `wl_buffer.release`.

**Smithay's, by ownership.** `on_commit_buffer_handler` keeps the surface's
current buffer in its `RendererSurfaceState` as a `Buffer`, an
`Arc<InnerBuffer>`. The `Arc` lets render elements keep the old attachment
after the surface has moved on to another buffer. Dropping the last reference
sends the release (`backend/renderer/utils/wayland.rs:68–78`). That normally
happens when a later commit attaches a different buffer (`:168–178`) or
removes it, or a little later if a render element still holds the old one.

Smithay uploads pixels lazily. Each renderer context that draws the surface
and has no texture for it yet imports the buffer at that moment
(`import_surface`, `:493–529`).

**Aizome's, by event.** In `CompositorHandler::commit` (`shell.rs:105–114`):

1. `fresh_shm_buffers` (`backend/mod.rs:488–511`) walks the surface tree and
   lists the shm buffers this commit makes current.
2. Smithay's handler runs, and `early_import` asks the backend to import.
3. `release_shm_after_import` (`backend/mod.rs:106–125`) asks the backend
   whether the shm pixels are now in textures (`import_shm_copies`), and if
   so sends `release()` on each listed buffer itself.

`import_shm_copies` answers per backend:

- headless and winit import the tree into their GLES renderer
  (`headless.rs:188–199`, `winit.rs:104–110`) and say yes;
- udev says yes with GLES on a single GPU (`udev.rs:374–384`);
- pixman, and udev with several GPUs, say no, and the buffer stays until
  Smithay lets it go.

Smithay's `Buffer` is untouched by Aizome's release. It still holds the
buffer, and still releases it when its last reference goes.

## The problems

### 1. Every early-released buffer is released twice

Aizome's release is the first. Smithay's, when its last reference to that
attachment drops, is the second. `docs/compositor.md` already notes this. It
breaks rule 2 for every shm buffer Aizome releases early.

Often a client survives it. GTK 3, for one, frequently destroys a buffer it
has been given back, and libwayland then drops the late release. A trace of
Firefox's "Open Firefox in Troubleshoot Mode?" dialog shows exactly this:
`discarded wl_buffer#78.release()`. But a client that keeps its buffers in a
pool and trusts every release, as the protocol allows, is told a buffer is
free while that buffer is still on screen.

### 2. Two paths that must agree on the surface tree

`fresh_shm_buffers` repeats Smithay's decision about which buffers a commit
makes current. When the two disagree, a buffer is released twice in one
commit. That is what crashed Firefox (#355):

- Smithay applies a sync subsurface's state with its parent's commit, and
  calls the commit handler for the child and then for the parent.
- Smithay's handler skips the child and takes its buffer on the parent's
  turn, but Aizome released it on both.
- GTK took the first release as "draw into this buffer again" and freed the
  buffer on the second, while still holding it. The next paint hit a cairo
  assertion.

#355 makes Aizome skip sync subsurfaces too. The rule now holds because two
pieces of code mirror each other, and nothing checks that they keep doing so
when Smithay changes how it applies commits.

### 3. On real hardware, the buffer goes back before it is read

On the udev backend (the installed session), `import_shm_copies` calls
`GpuManager::early_import` and treats `Ok` as "the pixels are in textures"
(`udev.rs:374–384`; its comment says so). But Smithay's early import does
nothing for shm buffers. Its shm arm reads
`// we just need to upload in import_shm_buffer` and returns `Ok(())`
(`backend/renderer/multigpu/mod.rs:671–674`), and it inserts no texture.
`udev.rs` never calls `import_surface_tree`. The pixels are read only when
the surface is next imported for rendering, in
`MultiRenderer::import_shm_buffer` (`multigpu/mod.rs:1923–1950`), which reads
the client's live mapping, not a snapshot.

So on udev with GLES, Aizome releases every shm buffer at commit time, before
anything has read it. A client that draws into a released buffer straight
away can have half of its next frame shown, or nothing if it destroyed the
buffer. Sidestep does this by design, and GTK reuses a released buffer as its
next staging buffer. This breaks rule 1.

This is from reading the code; it hasn't been reproduced on hardware. Every
test that checks early release runs the headless backend, which does upload
before releasing (`tests/protocols.rs`), so none of them can see it.

### 4. "Has every reader finished?" is answered in the wrong place

Whether a buffer may go back early depends on who might still read it. Today
that answer is spread across three backends' `import_shm_copies` and a GPU
count checked at commit time, and nothing ties it to the upload itself. That
is how problem 3 went unnoticed.

The same gap would bite a later change. Smithay imports from the buffer for
any renderer context that has no texture for the surface yet
(`wayland.rs:503–518`). A context created after an early release would read
memory the client owns again. Aizome draws client surfaces in one renderer
context per run under GLES today (udev renders every output on the primary
GPU, `udev.rs:443–462`), so this isn't a bug now; nothing would warn of it
if that changed.

## The proposal: release by ownership, once per use

The established compositors share one idea (see below): **the release is
sent from one place, once per use.** Everything that may read the
buffer holds it, and the release goes out when the last of them lets go.
Releasing early then means letting go of the "may still read this memory"
hold as soon as a copy exists, rather than sending an extra event.

That removes the problems by construction:

- **No double release.** There is one release site, and it only sends what
  is owed.
- **No mirrored tree walk.** Nothing outside Smithay's buffer tracking
  decides which buffers a commit made current.
- **No release before the read.** The early release is sent by the code that
  did the copy, after it did it.
- **One place for the policy.** "May this buffer go back after upload?" is
  decided where the upload happens.

### What it looks like in Smithay's terms

In Smithay the `Arc` around `InnerBuffer` already tracks who holds an
attachment: the surface state, and any render element built from it. What is
missing is a record of whether the client is still owed a release for its
latest attach. With that record, whichever comes first sends the release (the
upload, or the last holder letting go), and the other finds nothing owed. The
change has three parts.

1. **A release owed, per use.** `InnerBuffer` records that a release is
   owed. Every commit that attaches the buffer sets the record, including a
   commit that attaches the same `wl_buffer` again: Smithay keeps the same
   wrapper then (`wayland.rs:168`), so a flag set once and never reset would
   leave a buffer the client reuses after an early release without any
   release at all. A new `release_early()` and `Drop` each send the release
   only if one is owed, and clear the record. A buffer attached again before
   it was released is owed one release, not two, as the protocol has it
   ("no longer used by the compositor").
2. **Release after the upload that happened.** In `import_surface`
   (`wayland.rs:503–529`): after
   `renderer.import_buffer` has succeeded for an shm buffer and the texture
   is stored, call `release_early()` if the compositor allows early release.
   This is the one place where headless, winit and udev all upload. On udev
   it runs when the surface is first imported for rendering, so the release
   always follows the read.
3. **No reads after an early release.** `import_surface` imports whenever the
   texture entry for its renderer context is empty (`wayland.rs:505`). Once
   the current attach has been released early, it must not. A renderer
   context with no texture for that surface can't draw it until the client
   attaches a new buffer (each new attach clears the textures and owes a
   release again). That is the trade-off, and with one context per run it
   doesn't arise in Aizome today.

The compositor states its side once, as a setting on Smithay's buffer
handling, for example "release shm after upload". Aizome sets it from the
backend it started with:

- GLES, single GPU: on.
- pixman: off, because it reads the client's memory in place.
- several GPUs: off, as now. `backend/mod.rs:109–110` keeps buffers there
  because "several GPUs may each read it". Since udev renders every output
  on the primary GPU (`udev.rs:443–462`), early release may be safe there
  too, but that needs checking on a two-GPU machine.

Aizome then deletes `fresh_shm_buffers`, `release_shm_after_import` and the
three `import_shm_copies`, and its `commit` goes back to calling Smithay's
handler and `early_import`. #355's sync-subsurface guard goes with them,
because the duplicate it prevents can no longer happen.

### What it means for clients

- Under GLES on one GPU, buffers still go back early, now always after their
  pixels are copied. On headless and winit that is at commit, as now. On
  udev it is when the surface is next imported for rendering, later than
  today's release but no longer too early.
- Each use is released exactly once, including a buffer the client attaches
  again after it was given back.
- Under pixman and with several GPUs nothing changes: the buffer goes back
  when the next one replaces it.

### Where the change lives

Parts 1–3 are changes to Smithay's `backend/renderer/utils/wayland.rs`.
Aizome can't make them alone. Smithay's wrapper releases on drop, and the
surface render elements Aizome uses read Smithay's `RendererSurfaceState`, so
Aizome can neither stop that release nor replace the state without
reimplementing Smithay's surface rendering.

So the options are:

**O1. Send it to Smithay, and carry it until a release has it.** Write parts
1–3 against Smithay's `master`, send them upstream, and carry the same change
on a bitemyapp/smithay branch cut at `v0.7.0`, pinned under
`[patch.crates-io]` in `desktop/aizome/Cargo.toml` (as `cursor-plane.md`
weighs for its fix). Cost: a fork of Aizome's largest dependency until a
Smithay release reaches Aizome, and a design Smithay's maintainers may want
shaped differently.

**O2. An Aizome-owned surface buffer state.** Stop using
`on_commit_buffer_handler` and keep Aizome's own buffer, damage and texture
state, released by ownership, as wlroots and Mutter do in their own code.
Cost: Aizome would also have to replace `render_elements_from_surface_tree`,
`WaylandSurfaceRenderElement` and everything else that reads Smithay's state
(blur, capture, thumbnails, focus and the shell's "has a buffer" checks).
That is a large fork of Smithay's renderer utilities kept inside Aizome.

**O3. Keep releasing by event, and fix the udev ordering only.** In udev's
`import_shm_copies`, get `gpus.single_renderer(&self.primary_gpu)`, call
`smithay::backend::renderer::utils::import_surface_tree` with it, and say yes
only if that import succeeds. That is the renderer and context the frame
draws with on the primary GPU: udev renders every output there, through
`single_renderer(&primary)` or, for outputs on another GPU,
`renderer(&primary, &render_node, format)` (`udev.rs:455–462`), and both
have the primary GPU's context. So the frame finds the
texture and doesn't read the buffer again. Keep the several-GPU and pixman
policy as it is. Cost: it fixes problem 3 only; the double release (1) and
the mirrored walk (2) stay.

### Recommendation

O3 now. Problem 3 can show a client's half-drawn frame on the installed
session, and the change is a few lines in `udev.rs`. It can only be tested on
hardware or in a VM with a GPU.

Then O1. It fixes the ownership where the owner already is, removes Aizome's
parallel path, and is small enough to offer upstream. O2 only if Smithay
won't take O1 and carrying it becomes a burden.

## How other compositors do it

The four compositors below each send the protocol event from one place: a
count of holders, and the release when it reaches zero. They differ in when
an shm buffer's last holder lets go.

| Compositor | What holds a client buffer | When an shm buffer goes back | Why it can't be released twice |
|---|---|---|---|
| wlroots (sway and others) | Locks on a `wlr_buffer`: surface state, sync-subsurface caches, renderers, scanout | When the commit is applied: the pixels go into the surface's texture, then the source is unlocked | The texture wrapper the surface keeps doesn't hold the source, so replacing it unlocks nothing |
| Weston | `weston_buffer_reference`s, each either "may be accessed" or "will not be accessed" | When the last "may be accessed" reference goes: once the pixels are in the surface's texture, the GL renderer and the core downgrade theirs, unless several backends or `keep_buffer` need the buffer | Only "may be accessed" references count toward the release; replacement drops passive ones |
| Mutter | A use count on `MetaWaylandBuffer`, taken by transactions and by consumers such as scanout | When the transaction lets go: shm pixels go into a texture while attaching and processing damage, and the surface doesn't hold shm buffers | Replacement only drops a buffer the surface held, and shm isn't held |
| KWin | `GraphicsBufferRef`s: transactions, the surface, scene items | When the last reference goes, often at replacement; no early release found | The release is sent from the count reaching zero |

Sources: wlroots
[`buffer.c`](https://gitlab.freedesktop.org/wlroots/wlroots/-/blob/master/types/buffer/buffer.c),
[`client.c`](https://gitlab.freedesktop.org/wlroots/wlroots/-/blob/master/types/buffer/client.c),
[`wlr_compositor.c`](https://gitlab.freedesktop.org/wlroots/wlroots/-/blob/master/types/wlr_compositor.c)
(`surface_commit_state` unlocks the source after the commit listeners) and
[`wlr_shm.c`](https://gitlab.freedesktop.org/wlroots/wlroots/-/blob/master/types/wlr_shm.c);
Weston
[`compositor.c`](https://github.com/wayland-mirror/weston/blob/main/libweston/compositor.c),
[`surface-state.c`](https://github.com/wayland-mirror/weston/blob/main/libweston/surface-state.c) and
[`gl-renderer.c`](https://github.com/wayland-mirror/weston/blob/main/libweston/renderer-gl/gl-renderer.c);
Mutter
[`meta-wayland-buffer.c`](https://github.com/GNOME/mutter/blob/main/src/wayland/meta-wayland-buffer.c),
[`meta-wayland-transaction.c`](https://github.com/GNOME/mutter/blob/main/src/wayland/meta-wayland-transaction.c) and
[`meta-wayland-surface.c`](https://github.com/GNOME/mutter/blob/main/src/wayland/meta-wayland-surface.c);
KWin
[`graphicsbuffer.h`](https://github.com/KDE/kwin/blob/master/src/core/graphicsbuffer.h),
[`shmclientbuffer.cpp`](https://github.com/KDE/kwin/blob/master/src/wayland/shmclientbuffer.cpp),
[`transaction.cpp`](https://github.com/KDE/kwin/blob/master/src/wayland/transaction.cpp) and
[`surfaceitem.cpp`](https://github.com/KDE/kwin/blob/master/src/scene/surfaceitem.cpp).

What Aizome can take from them:

- **"Held" and "may still be read" are separate.** Weston names the two
  kinds of reference; wlroots and Mutter get the same split from the
  texture wrapper not holding the source. Smithay's `Buffer` has only the
  first; part 1 adds the record that stands in for the second.
- **Early release follows the copy.** Each that releases early does it in
  the code that made the texture, after the copy. None decides it at commit
  time from what a backend expects to happen. That is part 2.
- **Later readers use the texture.** Damage from the next buffer goes into
  the retained texture. A consumer that still needs the client's memory, such
  as scanout, keeps its own reference and delays the release. Weston turns
  early release off when several backends may read a buffer, much as Aizome
  keeps buffers with several GPUs. That is part 3 and the setting.
- **Sync subsurfaces need no special case.** The cached state holds the
  buffer like any other holder until the parent's commit applies it.
- **A buffer used again is owed again.** Mutter handles a buffer attached
  again in its surface-state code; part 1's record is the same case.

Smithay itself, and the compositors that rely on its buffer handling alone
(anvil,
[niri](https://github.com/niri-wm/niri/blob/main/src/handlers/compositor.rs),
[cosmic-comp](https://github.com/pop-os/cosmic-comp/blob/master/src/wayland/handlers/compositor.rs)),
release when the last reference to an attachment drops. Smithay's `master`
still releases unconditionally when `InnerBuffer` drops
([`wayland.rs`](https://github.com/Smithay/smithay/blob/master/src/backend/renderer/utils/wayland.rs)),
so it has no early release that Aizome could switch to today.

## Testing the change

- The existing protocol tests, which after #355 count releases per buffer,
  keep checking one release per use under GLES and none under pixman.
- New: replacing an early-released buffer with another commit must not
  release the first again. Today it does, so this test fails before the
  change.
- New: a client that attaches the same buffer again after an early release
  gets a release for the new attach.
- New: a buffer is not released before the frame that reads it. Headless
  can't show the udev ordering, so this needs the udev backend in a VM with
  a GPU (virtio-gpu with virgl), or a unit test on the import path with a
  counting buffer.
- Firefox and the GTK repro used for #355, on a headless Aizome, as a smoke
  check.

## What has been checked

- Problems 1 and 2, at runtime: the second release after an early release in
  a Firefox trace, and the sync-subsurface double release and crash (#355).
- Problem 3, by reading Aizome at `462dd31f` and Smithay 0.7.0 on
  2026-11-08, confirmed independently by a second reader. Not reproduced: it
  needs the udev backend.
- O3's route: udev draws every output on the primary GPU, through
  `single_renderer(&primary)` or `renderer(&primary, &render_node, format)`
  (`udev.rs:455–462`), and a `MultiRenderer`'s context is its render GPU's
  (`multigpu/mod.rs:1069–1071`), so a texture imported with
  `single_renderer(&primary)` is the one the frame uses. Not built.
- Smithay's `master`, for the release on drop only: it is unchanged. Other
  changes on `master` past 0.7.0 that a patch would have to fit were not
  surveyed.
- The other compositors, from their current source as linked, read on
  2026-11-08. KWin's lack of an early shm release is "not found", not proven
  absent.

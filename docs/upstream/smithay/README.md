# Smithay

Aizome's compositor (`desktop/aizome/crates/aizome-comp-core`) is built on
Smithay 0.7.0 from crates.io (`desktop/aizome/Cargo.toml`). Sekka carries no
patched copy of Smithay: there's no `[patch]`, vendored source or Nix patch for
it. Where Smithay has a bug, Aizome works around it in its own code. This page
lists those bugs, where each stands upstream, and what Aizome can drop once
Smithay has the fix in a release that Aizome uses. Smithay hasn't released
since 0.7.0, so "fixed upstream" here means fixed on `master` only.

Checked on 2026-11-06 against Smithay's `master` at 65aa86fa (2026-11-03),
513 commits after 0.7.0.

| Bug | Aizome's workaround, to drop once fixed | Upstream, 2026-11-06 |
| --- | --- | --- |
| [The hardware cursor copy runs past the cursor plane](#hardware-cursor-copy) | Client cursor surfaces kept off the cursor plane (`render.rs`, in #326, not merged yet) | In review: [Smithay#2265](https://github.com/Smithay/smithay/pull/2265), sent from Sekka |
| [`set_modifier_state` resets the layout](#set_modifier_state-resets-the-layout) | The layout set again after the locks (`keyboards.rs`) | In review: [Smithay#2266](https://github.com/Smithay/smithay/pull/2266), sent from Sekka |
| [Drag sources never hear `target`](#drag-sources-never-hear-target) | None. The drag test can check `target` (`tests/protocols.rs`) | In review: [Smithay#2267](https://github.com/Smithay/smithay/pull/2267), sent from Sekka |
| [Stale input method commits are discarded](#stale-input-method-commits) | Aizome handles `zwp_input_method_v2.commit` itself (`handlers.rs`) | In review: [Smithay#2215](https://github.com/Smithay/smithay/pull/2215), from another compositor's author |
| [xdg-toplevel-icon disconnects Chromium and Electron](#xdg-toplevel-icon) | Aizome's own xdg-toplevel-icon (`protocols/toplevel_icon.rs`) | Fixed on `master`: [Smithay#1963](https://github.com/Smithay/smithay/pull/1963) |
| [An input method's keyboard grab keeps its first keymap](#input-method-keymaps) | A documented limitation (`docs/compositor.md`) | Fixed on `master`: [Smithay#2213](https://github.com/Smithay/smithay/pull/2213) |
| [Releasing an input method's grab ends a menu's](#input-method-grab-release) | Menus never replace the input method's grab (`focus.rs`, from bitemyapp/aizome#77 and bitemyapp/aizome#79) | Reported: [Smithay#2263](https://github.com/Smithay/smithay/issues/2263) |
| [GPU synchronization](#gpu-synchronization) | #281 and #317, both open | Not sent; [Smithay#2264](https://github.com/Smithay/smithay/pull/2264) was closed |

## Sent from Sekka

Each `.patch` here is `git format-patch` output for the commit its pull
request carries, on Smithay's `master` at 65aa86fa, so `git am` applies it
there. The branches are on [bitemyapp/smithay](https://github.com/bitemyapp/smithay).
Every commit says it was developed with AI assistance, as Smithay's
[AI policy](https://github.com/Smithay/smithay/blob/master/AI.md) asks.

### Hardware cursor copy

[cursor-copy-bounds.patch](cursor-copy-bounds.patch), 408ab74e, branch
`codex/cursor-copy-bounds`, sent on 2026-11-06 as
[Smithay#2265](https://github.com/Smithay/smithay/pull/2265).

`DrmCompositor` copies a cursor element straight into the cursor plane's buffer
when it can (`copy_element_to_cursor_bo`). It checked that the element fits the
plane, then copied the element's whole buffer at the plane's stride. A buffer
larger than the element overruns the mapping and panics the compositor. Aizome
hit this in #276: a 72 px Adwaita image drawn as a 64 px pointer on a 64×64
cursor plane crashed every login. The fix requires the whole source buffer to
match the element and fit the plane, and bounds the copy.

#326 (not merged yet) makes every pointer image Aizome draws exactly the size
it is shown, which is right regardless. It also puts client cursor surfaces on the primary plane,
because a client's `wp_viewport` can show part of a larger buffer. That second
part can go once Aizome has the fix.

### `set_modifier_state` resets the layout

[set-modifier-state-layout.patch](set-modifier-state-layout.patch), d3f7407f,
branch `fix/set-modifier-state-layout`, sent on 2026-11-06 as
[Smithay#2266](https://github.com/Smithay/smithay/pull/2266).

`KeyboardHandle::set_modifier_state` passed the effective layout index masked
with `xkb::STATE_LAYOUT_DEPRESSED`, `_LATCHED` and `_LOCKED` to
`update_mask`. Those constants are state component flags (16, 32 and 64), not
masks for a layout index, so all three were 0 and the keyboard went back to its
first layout. The fix passes the current layout through. A new unit test sets
the second layout of a `us,de` keymap, sets Caps Lock, and checks the layout.

Aizome's per-keyboard keymaps (`keyboards.rs`) carry Caps Lock and Num Lock
over with `set_modifier_state`, then set the layout again. The second step is
still needed after a new keymap, so nothing is dropped; the comment about the
order can go.

### Drag sources never hear `target`

[data-source-target.patch](data-source-target.patch), c4d02874, branch
`fix/data-source-target`, sent on 2026-11-06 as
[Smithay#2267](https://github.com/Smithay/smithay/pull/2267).

A drag target's `wl_data_offer.accept` never reached the source client as
`wl_data_source.target`, which clients use to show whether a drop would be
accepted. Smithay's `master` added `Source::accepted` for proxying sources
(Smithay#2146) but `WlDataSource` kept the empty default. The fix sends
`target`, as Weston and wlroots do.

Aizome works around nothing here. `tests/protocols.rs` notes that the source
never hears `target`. With that test also waiting for `target`, it fails on
Smithay 0.7.0 (no `target` after 10 s) and passes with the same one-line change
in 0.7.0's `Accept` arm. Once Aizome has the fix, the test can check `target`.

## Fixed or in review elsewhere

### Stale input method commits

Smithay answers a `zwp_input_method_v2.commit` whose serial isn't the number of
`done` events sent with the text input's `done(0)`, to discard it. Both
protocols say such a commit is applied as normal. IBus 1.5 counts its serial
from zero again whenever a global is announced, so after a display was plugged
in, foot (which drops a `done` whose serial isn't its own) lost everything typed
through IBus. Aizome dispatches `commit` itself and always sends the text
input's own serial. [Smithay#2215](https://github.com/Smithay/smithay/pull/2215)
makes the same change; once Aizome has it, the `Dispatch<ZwpInputMethodV2>`
override in `handlers.rs` can go back to Smithay's delegate.

### xdg-toplevel-icon

`ShmBufferUserData::remove_destruction_hook` in 0.7.0 removed the first hook
whose ID didn't match. An icon's hook stayed, so a buffer destroyed after its
icon, as Chromium and Electron do, raised `no_buffer` on the dead icon and the
client was disconnected. Aizome implements xdg-toplevel-icon-v1 itself.
[Smithay#1963](https://github.com/Smithay/smithay/pull/1963) fixed the
comparison on `master`; with it, `protocols/toplevel_icon.rs` can give way to
Smithay's `XdgToplevelIconManager`, keeping `tests/chromium.rs`.

### Input method keymaps

In 0.7.0 an input method's keyboard grab got the keymap only when it began, so
while IBus or Fcitx 5 held the grab, a keyboard's own keymap never reached it
(`docs/compositor.md`, Keyboards). [Smithay#2213](https://github.com/Smithay/smithay/pull/2213)
sends keymap changes to the grab, so the limitation lifts once Aizome has it.

### Input method grab release

The keyboard has one grab. When an input method destroys its keyboard grab,
Smithay calls `unset_grab()` without checking whose grab is active, so a menu's
grab that replaced the input method's ends too. Aizome's menus no longer replace
the input method's grab: they take the keyboard's focus and set their own grab
once the input method lets go (bitemyapp/aizome#77 and bitemyapp/aizome#79).
[Smithay#2263](https://github.com/Smithay/smithay/issues/2263) reports this,
and Smithay's maintainers want nested grabs rather than a narrow fix
([Smithay#2147](https://github.com/Smithay/smithay/pull/2147)), so Sekka sent
nothing.

### GPU synchronization

Two problems are on Smithay's `master` as in 0.7.0:

- The GLES renderer puts a GL fence after every texture upload and every draw
  that reads a texture. NVIDIA 615.78 leaks every sync object (#317).
- `MultiRenderer` reuses one dmabuf per pair of GPUs for cross-GPU copies and
  doesn't wait for the last copy before drawing into it again, so a second
  output on the other GPU can show the first's pixels (#277, #281).

[Smithay#2264](https://github.com/Smithay/smithay/pull/2264), which fenced
textures only for shared contexts, was closed in favour of one change for both.
That change is on `upstream/texture-and-multigpu-sync` in
[bitemyapp/smithay](https://github.com/bitemyapp/smithay), written for 0.7.0. It
needs porting to `master` before it can be sent. Until then, #281 and #317 are
Aizome's workarounds.

## Not Smithay bugs

These looked like Smithay bugs and aren't, or aren't fixes Sekka has:

- Idle inhibitors: Smithay calls `IdleInhibitHandler::uninhibit` only when the
  client destroys the inhibitor, as its documentation says, so Aizome drops an
  inhibitor with its surface (`idle.rs`).
- Cursor planes in VMs: virtio-gpu, qxl and vmwgfx show cursor planes only to
  clients that set `DRM_CLIENT_CAP_CURSOR_PLANE_HOTSPOT`, which Smithay doesn't
  support ([Smithay#2012](https://github.com/Smithay/smithay/issues/2012)). That
  kept #276's crash out of a VM test.
- ext-image-copy-capture: 0.7.0 has none, so Aizome implements it
  (`protocols/capture.rs`). Smithay's `master` has
  `wayland::image_copy_capture`.
- fifo and commit-timing barriers wait for the compositor to release them
  (`pacing.rs`), by design.

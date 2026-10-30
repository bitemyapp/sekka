# Linux xe: display page tables filled page by page

| | |
| --- | --- |
| Sekka's copy | [`dpt-cursor.patch`](../../../installer/genkan/rust/system/xe-dpt/dpt-cursor.patch), for 7.3 |
| Upstream version | [xe-dpt-cursor.patch](xe-dpt-cursor.patch), for `master` at d89e1d186ec9 |
| Status | Not sent ([Submitting](#submitting)) |
| Fixes | 44e694958b95 ("drm/xe/display: Implement display support"), 216d62bb241a ("drm/xe/display: Add writing of remapped dpt") |

## What goes wrong

xe scans out a tiled framebuffer through a display page table (DPT), a small
buffer object holding one GGTT-format entry per 4 KiB page of the framebuffer.
`intel_plane_pin_fb()` keeps the old plane state's mapping only when the plane
shows the same framebuffer as before. Every page flip to another one, which
is every flip of a compositor's swapchain or of a client's buffer on a plane,
goes through `__xe_pin_fb_vma_dpt()`, which creates a DPT and fills it.

Display version 14 and later (and Alder Lake-P) map every DPT framebuffer
through the *remapped* view, which pads its stride to a power of two
(`intel_fb_needs_pot_stride_remap()`). That view is filled by
`write_dpt_remapped_tiled()`, the normal view by a loop in
`__xe_pin_fb_vma_dpt()`, and both look each page up on its own:

```c
for (column = 0; column < plane->width; column++) {
	u64 addr = xe_bo_addr(bo, offset, XE_PAGE_SIZE);

	iosys_map_wr(map, dest, u64, addr | pte);
	dest += sizeof(u64);
	offset += XE_PAGE_SIZE;
}
```

For a buffer in system memory, `__xe_bo_addr()` starts a cursor at the head of
the buffer's sg table (`xe_res_first_sg()`) and walks the table to that page.
Filling the DPT therefore costs the number of pages times the number of DMA
segments. With the IOMMU in passthrough, segments are physically contiguous
runs, so the cost grows as memory fragments. All of it is in
`drm_atomic_helper_prepare_planes()`, inside the atomic commit ioctl, on the
compositor's thread, nonblocking commits included.

The per-page lookups are as old as each path: `pte_encode_bo()` per page in
44e694958b95 for the normal view, and in 216d62bb241a for the remapped one.
They are the same in 7.2, 7.3 and `master`. The GGTT path, `xe_ggtt_map_bo()`,
already walks a buffer with one cursor.

## What it did in Sekka

On ganymede (ThinkPad X1 Carbon Gen 14, Core Ultra 7 365, 2880 × 1800 OLED at
120 Hz, Linux 7.3, IOMMU groups `identity`) after 2 days 21 hours up,
`aizomectl stats` gave eDP-1 a p50 of 3.1–4.2 ms and a p95 of 4.7–6.9 ms
against Aizome's 4 ms budget. The compositor's main thread had spent
10,888 s in the kernel and 1,990 s in user space. `/proc/buddyinfo` showed no
free order-10 blocks and 13 of order 9 in the Normal zone.

Aizome renders into 4-tiled CCS buffers (`I915_FORMAT_MOD_4_TILED_LNL_CCS`,
`AB30`), so every flip took the DPT path:

- `perf record -g` of the compositor for 15 s: about 90% of its cycles were in
  one 41-byte loop, `__xe_bo_addr+0x116` to `+0x13f` in `xe.ko`, which is the
  inlined `__xe_res_sg_next()` walk. Each sample's call chain went through
  `drm_mode_atomic_ioctl` → `intel_atomic_commit` → `intel_plane_pin_fb` →
  `xe_fb_pin_dpt_pin`.
- `perf ftrace profile -G drm_mode_atomic_ioctl` for 5 s: `__xe_pin_fb_vma`
  was 94% of the commits' time, with about 4,200 `xe_bo_addr()` calls per
  commit (tracing inflates the absolute times).

That puts the fill at close to 3 ms per flip: hundreds of segments per
buffer, walked about 4,000–5,000 times.

## The fix

**drm/xe/display: Look up a DPT's pages with one cursor**. A `dpt_cursor`
holds an `xe_res_cursor` over the buffer and the offset it is at. Asked for a
later offset, it moves forward with `xe_res_next()`. Asked for an earlier one,
as when a remapped view moves on to its next plane, it starts again from the
head. It makes the same split `__xe_bo_addr()` makes: `xe_res_first_sg()` over
the sg table for system memory, and `xe_res_first()` over the resource plus
`vram_region_gpu_offset()` for VRAM and stolen memory. The normal view and
both remapped writers (`write_dpt_remapped_tiled()` and
`write_dpt_remapped_linear()`) look pages up through it, and each entry is the
same value as before. The rotated view's offsets go back at every step, so
`write_dpt_rotated()` keeps `xe_bo_addr()`, as does the GGTT path's
`write_ggtt_rotated()`.

## Sekka's copy

[`installer/genkan/rust/system/xe-dpt/dpt-cursor.patch`](../../../installer/genkan/rust/system/xe-dpt/dpt-cursor.patch)
is the same change on 7.3. It differs from the upstream version only in
context: on `master`, `__xe_pin_fb_vma_dpt()` tests the view with
`intel_gtt_view_is_normal()`. DEVELOPERS.md ("Intel page flips") explains how
Sekka builds xe alone with it.

The first version of the patch changed only the normal view's loop. Booted on
ganymede, every flip still went through the old lookups, about 3,700
`xe_bo_addr()` calls per pin, all from the remapped view. The commit history of
bitemyapp/sekka#153 has both versions.

## Testing

On 2026-10-29, on wx-workstation:

- Sekka's copy builds xe for Nixpkgs' 7.3 with no compiler warnings: 294
  objects, BTF generated, 1,553,176 bytes compressed against the kernel's own
  1,546,232. Its version magic (`7.3.0 SMP preempt mod_unload`), `retpoline`
  and dependencies match the kernel's own xe.
- `checks.x86_64-linux.xe-dpt` passes. Its NixOS VM resolves `xe` to
  `updates/xe.ko.xz`, loads it with no symbol or version errors, and the module
  taints the kernel only `O` and `E`.
- `checkpatch.pl --strict` passes the code (see [Submitting](#submitting)).

On ganymede, the first version (normal view only) was booted through a
specialisation of its own system, with the module built on wx-workstation for
the same kernel. modprobe loaded `updates/xe.ko.xz`, the desktop came up on it,
and it tainted the kernel `O` and `E` only. Its commits still made about 3,700
`xe_bo_addr()` calls per pin: every flip used the remapped view. Commit times
were low (p50 0.26 ms, p95 1.0 ms), but only because memory had just been
booted (3,121 free order-10 blocks in the Normal zone, against none before),
so each walk was short. That is what led to this version.

This version was booted on ganymede the same way (module
`hjbx3db4…-linux-xe-dpt-cursor-7.3`, the same code as Sekka's copy, whose
commit message has since gained these results). Profiled a few minutes after
boot:

- `perf ftrace profile -G drm_mode_atomic_ioctl`: no `xe_bo_addr()` calls in
  30 pins. `dpt_cursor_addr()` was called 150,637 times, about 5,000 per pin,
  and is a function of its own in this build. Tracing each of those calls is
  most of the 0.62 ms per pin that ftrace shows for `write_dpt_remapped()`.
- `perf record`, untraced: the DPT fill was 11 of the compositor's 387 samples
  over 8 s (2.8%), about 0.14 ms per pin. The first version's boot, equally
  fresh, had 31 of 780 (4.0%), about 0.57 ms per pin. Both counts are small.
- `aizomectl stats` over the profile: p50 0.25 ms, p95 1.5 ms, p99 2.1 ms.

Not yet tested: a run long enough for memory to fragment. On the old lookups
that took close to 3 ms per flip. With the cursor, the fill is linear in pages
and segments, so after a few days `aizomectl stats` should stay near the
figures above.

## Submitting

The patch has no `Signed-off-by`: the kernel's
`Documentation/process/coding-assistants.rst` leaves that to the person who
sends it, who certifies the Developer's Certificate of Origin. Before sending,
add it, check that the commit message says what was tested on hardware, and
send it to `intel-xe@lists.freedesktop.org`, copying the xe and display
maintainers `scripts/get_maintainer.pl` names (Matthew Brost, Thomas
Hellström, Rodrigo Vivi, Jani Nikula as of `master` above).
`checkpatch.pl --strict` finds nothing in the code. It warns only about the
missing `Signed-off-by` and the `Co-Authored-By` trailer, which the kernel
doesn't define; the commit message describes the assistance instead, as
`Documentation/process/generated-content.rst` asks.

## When Sekka's copy goes

Drop `installer/genkan/rust/system/xe-dpt` once Nixpkgs' latest kernel has
this fix or another that makes filling a DPT linear. `checks.x86_64-linux.xe-dpt`
fails when Nixpkgs moves past 7.3, which is the time to check.

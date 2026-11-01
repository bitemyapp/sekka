# Upstream patches

Fixes Sekka makes to other projects' code, as sent to those projects. Sekka
builds with its own copy of each fix until a release that has it reaches
Nixpkgs. These records keep the version sent upstream, what it fixes, how it
was tested and where it stands, so that Sekka's copy can go once upstream has
the fix.

Each `.patch` file here is `git format-patch` output for the upstream commit
its document names, so `git am` applies it there. Sekka's other local patches
are described in [DEVELOPERS.md](../../DEVELOPERS.md) with the component they
change.

| Project | Patch | What it fixes | Upstream, 2026-10-25 |
| --- | --- | --- | --- |
| Linux | [xe-dpt-cursor](linux/xe-dpt-cursor.md) | Milliseconds of kernel time in every page flip on Intel xe graphics, filling a display page table page by page | Not sent (2026-10-29) |
| Plymouth | [keyboard-removal](plymouth/keyboard-removal.md) | A crash when the GPU's driver takes over, from a keyboard Plymouth had freed | In review: [plymouth!519](https://gitlab.freedesktop.org/plymouth/plymouth/-/merge_requests/519) |
| Plymouth | [console-owner](plymouth/console-owner.md) | Nothing typed reaching Plymouth after the GPU's driver takes over, Esc included | In review: [plymouth!520](https://gitlab.freedesktop.org/plymouth/plymouth/-/merge_requests/520) |
| Plymouth | [abandoned-prompts](plymouth/abandoned-prompts.md) | A question left on the splash after whoever asked it has gone | Not sent: fixed differently on Plymouth's `main` |

[plymouth/README.md](plymouth/README.md) explains where the three Plymouth bugs
come from, how the fixes were tested, and how they are being submitted.

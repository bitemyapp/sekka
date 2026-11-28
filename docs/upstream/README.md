# Upstream patches

Fixes Sekka makes to other projects' code, as sent to those projects. Sekka
builds with its own copy of each fix until a release that has it reaches
Nixpkgs. These records keep the version sent upstream, what it fixes, how it
was tested and where it stands, so that Sekka's copy can go once upstream has
the fix.

Each `.patch` file here is `git format-patch` output for the upstream commit
its document names, so `git am` applies it there. Sekka's other local patches
are described in [DEVELOPERS.md](../../DEVELOPERS.md) with the component they
change. Aizome takes Smithay from crates.io and carries no copy of it, so for
Smithay it works around each bug in its own code instead;
[smithay/README.md](smithay/README.md) says which workaround each fix retires.

| Project | Patch | What it fixes | Upstream |
| --- | --- | --- | --- |
| Plymouth | [keyboard-removal](plymouth/keyboard-removal.md) | A crash when the GPU's driver takes over, from a keyboard Plymouth had freed | In review since 2026-10-25: [plymouth!519](https://gitlab.freedesktop.org/plymouth/plymouth/-/merge_requests/519) |
| Plymouth | [console-owner](plymouth/console-owner.md) | Nothing typed reaching Plymouth after the GPU's driver takes over, Esc included | In review since 2026-10-25: [plymouth!520](https://gitlab.freedesktop.org/plymouth/plymouth/-/merge_requests/520) |
| Plymouth | [abandoned-prompts](plymouth/abandoned-prompts.md) | A question left on the splash after whoever asked it has gone | Not sent: fixed differently on Plymouth's `main` (2026-10-25) |
| Smithay | [cursor-copy-bounds](smithay/README.md#hardware-cursor-copy) | A compositor panic when a cursor buffer is larger than the cursor plane | Open since 2026-11-06, not reviewed (2026-11-10): [Smithay#2265](https://github.com/Smithay/smithay/pull/2265). A maintainer cited Smithay's AI policy on its description |
| Smithay | [set-modifier-state-layout](smithay/README.md#set_modifier_state-resets-the-layout) | The keyboard layout going back to the first whenever the modifiers are set | Open since 2026-11-06, not reviewed (2026-11-10): [Smithay#2266](https://github.com/Smithay/smithay/pull/2266). A maintainer cited Smithay's AI policy on its description and the unticked DCO box |
| Smithay | [data-source-target](smithay/README.md#drag-sources-never-hear-target) | Drag sources never hearing which type the target accepted | Open since 2026-11-06, not reviewed (2026-11-10): [Smithay#2267](https://github.com/Smithay/smithay/pull/2267). A maintainer cited Smithay's AI policy on its description and the unticked DCO box |

[plymouth/README.md](plymouth/README.md) explains where the three Plymouth bugs
come from, how the fixes were tested, and how they are being submitted.
[smithay/README.md](smithay/README.md) lists the other Smithay bugs Aizome
works around and where each stands, and says
[what the maintainer wrote](smithay/README.md#sent-from-sekka) on the three
pull requests.

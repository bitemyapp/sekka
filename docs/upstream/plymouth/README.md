# Plymouth

Sekka's boot splash is a Plymouth script theme (DEVELOPERS.md,
[Boot splash](../../../DEVELOPERS.md#boot-splash)). On a disk Genkan encrypted,
the splash asks for the passphrase in the initrd. NixOS's initrd has no GPU
driver, so Plymouth asks on the firmware's framebuffer through simpledrm
(`UseSimpledrm=2`). Once root is open, the installed system loads the GPU's DRM
driver, the kernel replaces simpledrm with it, and Plymouth moves to the new
device: it frees simpledrm's renderer, and the keyboard that came with it, and
makes new ones. Two of the bugs below are in that handover. The third is in how
plymouthd treats a request whose sender has gone, which a mistyped passphrase
brought about.

`checks.x86_64-linux.encrypted-boot-splash`
(`installer/genkan/nix/encrypted-splash-test.nix`) found all three. Sekka
builds Nixpkgs' Plymouth 26.134.222 with a fix for each, from
[`installer/genkan/rust/system/boot-splash/plymouth/`](../../../installer/genkan/rust/system/boot-splash/plymouth):

| Sekka's patch | Upstream version | Status, 2026-10-25 |
| --- | --- | --- |
| [keyboard-removal](keyboard-removal.md) | [keyboard-removal.patch](keyboard-removal.patch), two commits | Ready to send |
| [console-owner](console-owner.md) | [console-owner.patch](console-owner.patch), one commit | Ready to send |
| [abandoned-prompts](abandoned-prompts.md) | None: Plymouth's own f3461296 and e7fb0732 | Fixed on `main`, in no release yet |

## Upstream versions

The two patches here are for Plymouth's `main` at 074cc389 (2026-10-22), 618
commits after 26.134.222. Since that release `main` has moved plymouthd's input
handling from `src/main.c` into `src/daemon/`, so these are ports of Sekka's
copies rather than the same files. Each document lists the differences. To make
the branches again from a clone of
<https://gitlab.freedesktop.org/plymouth/plymouth>:

```sh
git switch -c keyboard-removal 074cc389
git am path/to/sekka/docs/upstream/plymouth/keyboard-removal.patch
git switch -c console-owner 074cc389
git am path/to/sekka/docs/upstream/plymouth/console-owner.patch
```

## Submitting

Each branch goes to Plymouth as its own merge request, and each request names
the other, since Sekka's check needs both. As of 2026-10-25 neither is open.
The account sending them, bitemyapp on gitlab.freedesktop.org, can't fork
projects until freedesktop's administrators verify it, which
[freedesktop/freedesktop#4333](https://gitlab.freedesktop.org/freedesktop/freedesktop/-/work_items/4333)
asks for. When the requests are open, their links go in the table above and in
each patch's document.

All three commits carry a `Co-Authored-By: Claude` trailer, and both merge
request descriptions say they were written with Claude Code.

## Testing

On 2026-10-25, each branch alone and the two merged were built on 074cc389 with
Nixpkgs' build inputs for Plymouth:

- `meson test`: 58 passed, 1 skipped and 3 failed, the same as `main` with
  nothing applied. The three that fail (`script-set-default-theme`,
  `script-generate-initrd` and `script-populate-initrd`) run scripts that start
  with `#!/bin/bash`, which NixOS doesn't have.
- `meson test` on a build with `-Db_sanitize=address,undefined`, run as
  Plymouth's `fedora-sanitizers-x86_64` CI job runs it
  (`ASAN_OPTIONS=detect_leaks=0:halt_on_error=1:abort_on_error=1`,
  `UBSAN_OPTIONS=halt_on_error=1:abort_on_error=1:print_stacktrace=1`): the same
  58, 1 and 3. With leak detection left on, 8 more tests abort, on `main` as well.
- `scripts/check-format.sh` (uncrustify) found no new style problems on either
  branch.

`encrypted-boot-splash` installs an encrypted system with Genkan's storage code
on UEFI and on BIOS and boots it in QEMU. It then goes through the prompt:
typing, a wrong passphrase, Caps Lock, Esc to the text view and back, a
passphrase asked after the GPU's driver (bochs-drm) has taken over, a question
whose asker goes away, restarting, and the login prompt. On 2026-10-24 it passed
all of its 24 steps at the time with Plymouth built from 074cc389 and both
branches, and none of Sekka's patches: Nixpkgs' three packaging patches only.
The check has 26 steps now, since ad618d6 made it wait 100 seconds at the
prompt, and that version hasn't been run against `main`.

To run it against an upstream tree, export the tree with `git archive` and
change the `overrideAttrs` in
`installer/genkan/rust/system/boot-splash/default.nix` to build from it instead
of adding Sekka's patches:

```nix
      (old: {
        version = "${old.version}-main";
        src = /path/to/exported/plymouth;
        hardeningDisable = [ "fortify" ];
      });
```

That is how the 2026-10-24 run built it. Then:

```sh
nix build .#checks.x86_64-linux.encrypted-boot-splash --no-update-lock-file
```

## When Sekka's copies go

When Nixpkgs moves to a Plymouth release, check each patch against it. Remove
a patch from `boot-splash/default.nix` once the release has its fix, upstream's
version or another, and run `encrypted-boot-splash`. If a patch stops applying
before then, port it; don't drop it.

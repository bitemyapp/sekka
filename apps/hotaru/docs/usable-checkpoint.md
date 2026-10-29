# Usable Aizome checkpoint

The user narrowed this effort to a usable application checkpoint and deferred
further porting and optimization. The checkpoint retains the existing CPU
renderer, bounded user copy/paste, shell sessions, tabs/splits, settings, search,
semantic metadata and automatic links. It contains no GTK frontend or macOS
application code. It is not a complete Ghostty replacement or a claim of
whole-application performance parity.

On an Aizome installation built from this branch, launch **Hotaru** from the
application menu or run `aizome-hotaru`. The existing default terminal remains
unchanged. The focused package is also available through `nix run .#hotaru`
from Sekka's root.

For this workstation session, **Hotaru (development)** is installed as a
user-local launcher on the laptop. Its executable can also be launched directly:

```sh
/home/callen/work/sekka-hotaru/result-hotaru/bin/hotaru
```

The local Nix daemon rejected importing unsigned workstation builds, and no
trust configuration was changed. Instead, the exact verified executable,
resources and fonts were copied into the worktree's ignored
`artifacts/hotaru-ready/`. Only the wrapper's package/font locations changed.
Existing Nix runtime dependencies have user-owned garbage-collection roots
inside that directory. Keep this worktree while using this development launcher.
Its desktop entry is
`~/.local/share/applications/org.aizome.Hotaru.Development.desktop`.

## Qualification

All compilation and application testing ran through `ssh wx-workstation`.
The Nix package passed **73 native tests**, with seven existing intentional
ignores. Six real private Wayland suites passed: large copy/paste and keyboard/
mouse input, font scaling, search, shell/terminfo behavior, explicit links and
automatic links. Clipboard checks include more than 1 MiB of plain/HTML copy
and paste, stalled/cancelled readers, pane-close races and continued UI input.
The relocated wrapper separately passed font-scale and shell-resource tests on
the workstation. The local copy was checked by file hashes, without executing
builds or tests on the laptop.

The final source check matched 325 local files. Of those, 319 match the Nix
prepared source; the six expected absences are the standalone Cargo lockfile
and five Sidestep patch inputs already applied to that prepared source.
Ten dependency notices and the packaged fonts/resources were verified.

The package build exposed an old C declaration style in retained Oniguruma
6.9.9 that modern C23 defaults reject. Its build script now explicitly selects
GNU C17. The dependency sources are unchanged; the failed attempt is preserved.

- Package: `/nix/store/1ig7cazh7bfyiz46gc09rv6yvxmz3722-hotaru-0.1.0`
- ELF SHA-256: `d54f359151e2b484953daaaeffda1de942ba079d169674d4878a2787ca10b2a4`
- Evidence SHA-256: `37495700648850af0e99e3770f7e2deba3a227797008c809603b747746435f73`

The [package receipt](../upstream/usable-checkpoint/package-checkpoint.json),
[runtime receipt](../upstream/usable-checkpoint/package-runtime.json),
[local launcher receipt](../upstream/usable-checkpoint/local-install.json) and
[compressed evidence](../upstream/usable-checkpoint/evidence.tar.gz) preserve
identities, logs, source checks, captures and reproduction scripts. This is
application qualification in private Sway; an Aizome VM, installed ISO and
physical desktop were not newly qualified.

## Deferred work

The [prioritized next work items](next-work.md) record acceptance gates and
prototype recovery details. Later commits reconciled Sekka main's Sukumo
migration and turned OSC 52 on; the package evidence here qualifies only the
preserved source.

At this checkpoint OSC 52 was disabled in the application; ordinary user
copy/paste was enabled.
Full Ghostty compatibility, Kitty graphics/clipboard, broader shell/tmux
integration, remaining sprites, accessibility, history memory and performance
work remain unfinished. Remote hosts need the bundled terminfo or the documented
`TERM=xterm-256color ssh host` fallback.

The following isolated work is preserved on both machines and excluded from
this usable checkpoint:

- Margin correctness fixes and the promising C0 optimization:
  `/home/callen/work/hotaru-margin-attribution-20261028/README.md`.
  Its full 40-case follow-up benchmark was never started.
- Native GPU prototype and remaining review/runtime gaps:
  `/home/callen/work/hotaru-gpu-native-isolated/artifacts/GPU-HANDOFF.md`.
- OSC 52 codecs and ordered event plumbing:
  `/home/callen/work/hotaru-osc52-native-isolated/artifacts/PLAN.md`.
  Native permission/transport integration remains unimplemented.

No experimental implementation above was merged into this checkpoint. No
background build, benchmark or test job is being left running.

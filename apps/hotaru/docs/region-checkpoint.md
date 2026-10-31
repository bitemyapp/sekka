# Braille and partial-scroll package checkpoint

The combined product at `0a62d49` passed its release Nix build and all eight
native Wayland suites on `wx-workstation`. It contains 434 exact-raster sprites,
the decorated-cursor correction and bounded partial-region pixel reuse.

The output is `/nix/store/dkqws66ci8la92b6k2zsvkjy20a1yr71-hotaru-0.1.0`.
Its ELF SHA-256 is
`1d4e649540eed06b5ec4b43e756e1d9b5b8bdbb9a715487903f3613963d0b262`;
the wrapper hash is
`dd780e65784fa7c85f9d2839c6d57f76e2ec36d4c25ef5f6f32e79c659e7bfb6`.
The prepared source is `/nix/store/xyr4ajfj1ra27rd2ywrixg6v3wkj4zn5-hotaru-source`.
Both paths have workstation GC roots under
`/home/callen/work/hotaru-region-package-20261028/`.

Nix passed 62 native tests, with seven existing explicit ignores. All 236
runtime source-manifest entries match the integrated worktree; 230 match the
prepared source, with only the standalone lockfile and five applied Sidestep
patches intentionally omitted. Installed resources, fonts and eight application
license notices match. The [package record](../upstream/nix-region-checkpoint.json)
retains the derivation, source NAR and installed inventory.

Clipboard/input, fonts, Find, shell integration, links, cursor, pixel scrolling
and sprite suites all pass against the exact installed wrapper with ambient
font/library overrides cleared. Clipboard checks retain exact transfers over
1 MiB, stalled/cancelled readers and closed-pane races. All 42 copy-versus-redraw
full-window RGBA captures match. All 1,736 sprite instances at scales 1, 1.25,
2 and back to 1 match the pinned Ghostty ReleaseFast raster oracle, comparing
5,593,392 RGB bytes. The [Braille audit](../upstream/sprites/braille/README.md)
documents why its unchanged ReleaseSafe oracle encounters an overstrict
upstream assertion and preserves that failure separately.

The [runtime reports](../upstream/native-region-checkpoint/package-runtime.json)
and [validation record](../upstream/native-region-checkpoint/validation.json)
retain exact identities and hashes. Full logs/captures remain in local
`artifacts/hotaru-region-package-20261028/` and the workstation directory above.
The initial command-line typo was corrected before the build; its log remains
in the raw evidence. Previous frozen packages and their measurements are unchanged.

This is combined component qualification, not physical-desktop or performance
acceptance. The application still uses the CPU/SHM renderer. GPU scene work is
isolated; the two ASCII-subrun experiments were rejected and the qualified
terminal core is unchanged. The earlier 60 Hz software scrolling observation,
hardware presentation, history memory and full Ghostty parity remain open.

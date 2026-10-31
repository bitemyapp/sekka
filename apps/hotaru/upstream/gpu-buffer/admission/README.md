# Isolated complete-scene admission proof

Base: historical `3d10197261512c92ccc5816dfdd60c88566c4150` native fixture tree
with the archived native-scene and remote-profile overlays. Current product core
has advanced independently. This archive changes no production renderer.

Final remote qualification:

- 39 worker/library tests and strict library Clippy pass.
- 9 GPU preparation/index tests pass, including padded asset/count admission.
- 18 native scene fallbacks match every RGBA byte and the original fixture files.
- A private Sway pixman capture matches all 2,988,000 RGB bytes of the 1,200×830
  Find scene at (40,65), with detach/release/disconnect and owned process-group
  cleanup. GPU unavailability is simulated after real scene preparation here.
- Final native test binary SHA256:
  `44fb5e7c385ed9bf3f5800a6446b8c4bb7ea00abc4ea237d6522806d17e9b520`.
- Final lifecycle source `worker/src/lib.rs` SHA256:
  `dc0d0b8b4e1915989c56530918d242f27ecf6a0eb6ef937743161e9c028039c1`.

`source-manifest.json` records 976 source/manifests/assets, exact font files,
compiler, host and test binary. Every listed local source hash was compared with
the workstation snapshot. `manifest.json` describes this compact archive itself.
Raw captures/builds/failure attempts remain at
`wx-workstation:/home/callen/work/hotaru-gpu-admission-isolated`.

## Source reconstruction

The patch bases are the frozen preceding experiment sources, not current product
files. In a dedicated copy of the historical tree:

1. Overlay `../native-scene/preparation/source/` at the Sekka root and apply
   `../remote-profile/preparation.patch` there.
2. Apply `native.patch` at that root.
3. Create `worker/` from `../native-scene/source/`, apply
   `../remote-profile/compute.patch` inside it, then apply `worker.patch` there.
4. Copy `wayland-admission.py` to the dedicated root. The workstation script uses
   explicit historical font/fixture paths and a private headless compositor.

`worker.patch` includes the standalone native-feature lock; `native.patch`
includes the corresponding Aizome lock and diagnostic test dependency. The
production workspace is untouched. The patch replay check separately verifies
all resulting changed source files.

## Commands and test boundaries

All commands ran over SSH under `/home/callen/.cache/hotaru-workstation.lock`.
The final target is `/home/callen/.cache/hotaru-admission-native-target`; using a
shared target across old relative-path mirrors produced an incompatible cached
Sidestep artifact, so the final native target was built fresh.

From `worker/`:

```sh
cargo +1.95.0 test --features native --lib
cargo +1.95.0 clippy --features native --lib -- -D warnings
```

From the isolated root:

```sh
cargo +1.95.0 test --manifest-path desktop/aizome/Cargo.toml \
  -p sidestep-engine --lib gpu_scene::
cargo +1.95.0 test --manifest-path desktop/aizome/Cargo.toml \
  -p aizome-hotaru --test scene_inventory --no-run --message-format=json \
  > artifacts/native-build.jsonl
python3 wayland-admission.py
```

The initial WRITE-sealed memfd failure is preserved in `write-seal-failure/`.
The final worker uses size seals and immutable ownership, since Wayland SHM
servers may request a writable mapping.

The live UI complete-capture handoff, calloop worker wake/timer wiring, bounded
async protocol imports, actual EGL scene driver integration and removal of its
diagnostic readback are still outstanding. See
`../../../docs/gpu-scene-admission.md` for ownership and accounting limits.

# Native scene composition diagnostic

This directory preserves the isolated GPU milestone described in
[gpu-native-scene.md](../../../docs/gpu-native-scene.md). It changes no production
renderer. The original-resolution [capture](capture.png) is a direct PNG encoding
of the successful RGB capture; it is not resized or retouched.

- `source/`: exact standalone Rust/GLSL source and lock for the Intel runs.
- `result.json`, `probe.log`: 36 exact offscreen renders, input/source/ELF hashes,
  loaded libraries, zero warm asset uploads and complete cleanup.
- `wayland-result.json`, `wayland.log`: last of three successful private hardware
  presentation runs. `repeated-wayland/` retains all three results and logs.
- `failed-loader-teardown/`: the preceding SIGSEGV report and compact core stack
  metadata. The large raw core stays outside Git.
- `preparation/source/`: the added/modified scene recorder and safe preparation
  sources, over `preparation/BASE_REVISION`. `inventory.json` records all 18
  native scenes. Sources are snapshots, not an already integrated toolkit patch.
- `remote-validation/`: wx-workstation test/Clippy logs, compiler identity, and
  a subsequent lint/comment-only patch. The patch is validated remotely but was
  not applied to the Intel-tested source snapshot.
- `qualification.json`, `manifest.json`: compact scope and exact file identities.

The glyph pipeline is the existing Sidestep harfrust/swash path; the complete
scene is composed by compute, not uploaded as a CPU-rasterized window. The
software oracle, tiny-skia small asset preparation, and native recorder are
explicitly retained. tiny-skia and Sidestep notices accompany those sources.

Full original artifacts remain at
`/home/callen/work/hotaru-local-gui-20261028/gpu-scene`. The complete isolated
preparation tree and input fixtures are at
`/home/callen/work/hotaru-gpu-scene-isolated` on wx-workstation and locally.
`run.py` and `wayland.py` preserve their original task-specific namespace paths;
they depend on the existing task-owned Nix closure and the frozen frame-matrix
subreaper/pidfd helper (`889c1151…`). They are diagnostic runners, not installers.
The namespace was unprivileged and touched no live desktop or device settings.

All builds and software checks ran on wx-workstation. Intel hardware results
predate the user's instruction to move all builds, tests and benchmarks back
there; no new local execution is authorized by this evidence. Archive inspection
and source hashing do not rerun the GPU. Reproduction must honor that current
constraint. The standalone selected-target tests use Rust 1.95.0:

```sh
CARGO_TARGET_DIR=/home/callen/.cache/hotaru-gpu-transport-target \
  cargo +1.95.0 test --locked --lib --bin scene-worker-probe --bin scene-wayland-probe
CARGO_TARGET_DIR=/home/callen/.cache/hotaru-gpu-transport-target \
  cargo +1.95.0 clippy --locked --lib --bin scene-worker-probe --bin scene-wayland-probe -- -D warnings
```

Take `/home/callen/.cache/hotaru-workstation.lock` around remote checks. Apply
the archived lint-only delta to the archived source for the strict Clippy gate.
The native software fixture uses `preparation/inventory.sh` in its isolated tree;
its regenerated file hashes equal every fixture in the hardware report.

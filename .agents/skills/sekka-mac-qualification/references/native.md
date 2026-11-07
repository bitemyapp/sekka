# M3: native macOS

On a Mac with the recorded toolchain, at the candidate revision:

```sh
cargo test --locked --manifest-path installer/genkan/macos/Cargo.toml
installer/genkan/macos/packaging/build-app.sh --engine ENGINE --icon ICON --out dev
"dev/Sekka Installer.app/Contents/MacOS/sekka-installer" --screenshots shots
```

Inspect every screenshot, light and dark, for every scenario (M3-04). Then,
with the release build:

- M3-02: re-sign a copy of the app ad hoc and show the helper refuses it;
  replace the helper with an ad hoc copy and show the app refuses it.
- M3-03: first launch registers the helper; macOS asks for approval in
  Login Items; the app waits and continues. Update the app while an
  installation record exists and show the record is kept.
- M3-05: VoiceOver reads every screen, the disk divider is a slider, and
  the whole journey works with the keyboard alone; larger text and the
  smallest window still show everything.
- M3-06: the catalog and the downloads with the network off and on again
  and through a proxy; downloads with too little space, cancelled and
  resumed.

`--simulate SCENARIO` shows any path without touching anything; use it to
review failure screens the hardware run shouldn't reach on purpose. A
simulated screen is M3-04's evidence only: each scenario's real path has
its own rows, and a scenario with no passing real-path row is a path the
candidate hasn't shown.

| Scenario | Real path |
| --- | --- |
| `normal` | M4-03 to M4-06, M5-01 |
| `unsupported` | M0-03, M4-01 |
| `not-ready` | M0-08, M4-12 |
| `offline` | M3-06 |
| `low-space` | M0-08, M4-11 |
| `wrong-password` | M0-09, M4-13 |
| `download-fails` | M3-06 |
| `resize-fails` | M4-02 |
| `install-fails` | M0-09, M4-16 |
| `startup-disk-fails` | M0-09, M4-17 |
| `waiting-for-recovery` | M0-09, M4-15 |
| `existing` | M0-08, M4-14 |
| `needs-approval` | M3-03 |

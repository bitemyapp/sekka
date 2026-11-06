# M0: components

Run the portable suites at the candidate revision and attach their output.

```sh
cargo test --locked --manifest-path installer/genkan/macos/Cargo.toml
cargo clippy --locked --all-targets --manifest-path installer/genkan/macos/Cargo.toml -- -D warnings
cargo clippy --locked --all-targets --target aarch64-apple-darwin --manifest-path installer/genkan/macos/Cargo.toml -- -D warnings
python3 -I -m unittest discover -s installer/genkan/macos/packaging/tests
nix build .#packages.aarch64-linux.mac-engine-tests   # the engine adapter's fixtures
```

Map the rows to the tests that show them:

| Row | Tests |
| --- | --- |
| M0-01 | `canonical::tests::schema_is_checked_first`, `trust::tests::unknown_schemas_and_altered_encodings_are_refused`, the engine's schema tests |
| M0-02 | `sekka-mac-trust` tests (signature, key, channel, window, clock, expiry, rollback, conflict) |
| M0-03 | `profile::tests::other_macs_and_firmware_fail_closed`, `plan::tests::blocking_issues_and_other_macs_stop_planning`, the engine's board tests |
| M0-04 | `sekka-mac-stage` tests (substitution, ranges, links, owners), the helper's `a_tampered_download_stops_before_the_engine` |
| M0-05 | `plan::tests::a_changed_layout_changes_the_plan`, the helper's `a_changed_mac_needs_a_new_review`, the engine's stale-plan tests |
| M0-06 | `sekka-mac-journal` tests, including every interruption of an append |
| M0-07 | the helper's busy refusals and `journal::tests::one_installation_at_a_time` |
| M0-08 | `plan::tests::free_space_needs_no_shrink`, `plan::tests::blocking_issues_and_other_macs_stop_planning`; the engine's `test_free_space_is_offered_beside_the_shrink`, `test_free_space_needs_no_shrink`, `test_without_resize_limits_no_shrink_is_offered`, `test_power_filevault_snapshots_and_disks`, `test_an_existing_sekka_is_recognized_and_stops_installation`, `test_a_partial_installation_stops_without_a_guess`; the app model's `a_mac_on_battery_is_not_ready`, `offline_and_low_space_stop_early`, `an_existing_installation_is_recognized_not_replaced` |
| M0-09 | the app model's `a_wrong_password_is_asked_for_again_in_place`, `reopening_reconnects_to_an_installation`, `an_interrupted_write_is_never_retried_by_the_app`, `only_the_startup_disk_is_retried` |

Component results say nothing about macOS APIs or real disks; those rows
pass here only for what they claim.

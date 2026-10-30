# Jev experiment recipes

These authored recipes were developed against the public `20261022-ed92471`
ISO. Read the [experiment report](../../docs/test-results/jev-2026-10-27/README.md)
and [driver guide](../../docs/jev-harness.md) before running them. They are
examples and retained failure cases, not a passing unattended suite.

The run used a disposable `jev` account with password `sekka-test-2026` and
synthetic documents. Do not reuse these credentials outside a test VM.
Scenarios assume the US keyboard layout and the visible state described in
their precondition. They do not reset a guest to that state.

## Installation and session rotation

1. `live-welcome.json` and `maximize-installer.json` establish the installer
   window. `install-all-six.json` combines the account, disk, desktop selection,
   review and confirmation recipes. Its combined form has not been replayed
   uninterrupted. The recorded installation used separately corrected/resumed
   recipes, retained in the private evidence directory.
2. `install-shutdown.json` uses the installer's normal shutdown. After process
   exit, `boot-disk` launches the installed disk with no ISO attached.
3. `gdm-user.json` opens the selected user's password prompt. GDM's small
   session gear needed separately recorded assisted input. The
   `gdm-login-*.json` recipes have differing starting states: inspect each
   precondition. `gdm-sessions.json` is an uncertain locator experiment, not a
   reliable replacement for the assisted step.
4. The rotation was Aizome, Tatami, GNOME, Plasma, Xfce, vanilla Hyprland,
   and back to Aizome. Use each desktop's launcher recipe and
   `firefox-local-page.json` for the synthetic offline page. Separate logout
   dialog/confirmation recipes preserve intermediate evidence.

The Aizome calculator and Sumi editor recipes exercise real app input, saving
and opening. `tatami-terminal.json` reads the saved document after logout.
Native Aizome text controls use Super+A for select-all; browser controls use
Ctrl+A. Tatami logout requires both selecting its menu entry and activating it.
The final two-action recipe was assembled from separately executed steps.

`gnome-lock*.json` and `gnome-unlock.json` retain experiments with display
blanking and a transient wrong-password error. They did not complete as one
automated sequence. Hyprland's first-login and launcher recipes also needed
assistance. `aizome-browser.json` expects a restored page and stopped at the
actual Firefox session-restore error. Fresh navigation and field edits were
visible, but Aizome's text assertions remained incomplete. The final shutdown
is independent of those assertions. Do not reinterpret a later successful step
as completing an earlier failed scenario.

## Calibration and coverage

`calibration.json` uses seven historical screenshots already in the repository.
`live-calibration.json` uses four original captures from this experiment;
`fixture-provenance.json` records their hashes. Both are small tuning sets
reviewed by Codex, not held-out benchmarks or independent human labels.

`coverage.json` is a source-derived planning inventory, with 1,551 entries that
remain unexecuted. `candidate-inventory-diff.json` records the differences
between its newer source revision and the tested public ISO. Adding entries to
this inventory does not create executable recipes or certify their coverage.

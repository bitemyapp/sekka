# Bug reports from the Jev prototype experiment

These reports accompany [draft PR #128](https://github.com/bitemyapp/sekka/pull/128).
They describe retained observations, not confirmed regressions in current
`main`. The [experiment report](README.md) records the complete scope and
limitations. No guest fix or new ISO was built for these reports.

All guest observations used public ISO `20261022-ed92471`, SHA-256
`98d5174b40b2a552eae8586ad2578eb93b989e9ff1f7116c38cf5f002de0f591`,
Sekka `ed92471d8cd92e80267bb69df1225da28054a91f`, and Aizome
`ca4e289e7bdc9ffe9371f71416c7c3f04e325344`. The installed VM used UEFI,
KVM, software virtio graphics, 8 GiB RAM, a 100 GiB NVMe disk, all six
desktops, Firefox and the disposable `jev` account. Tests after installation
booted without the ISO. The VM is stopped and its disk is retained.

Scenario identifiers below resolve under the private evidence directory
`/home/callen/work/sekka-jev/.work/jev-evidence/uefi/`; their recipe/report
hashes and outcomes are indexed in [summary.json](summary.json). Screenshots
are original QMP captures with hashes in [screenshots.json](screenshots.json).
Review and assistance were performed by Codex, not an independent human.

| ID | Finding | Status |
| --- | --- | --- |
| JEV-01 | User services enter failed state at desktop logout | Observed; isolated reproduction and ownership pending |
| JEV-02 | Firefox shows a session-restore error after desktop rotation | Observed once; cause and minimal reproduction pending |
| JEV-03 | Stable-state assertion misses a transient authentication rejection | Confirmed harness limitation; open |
| JEV-04 | Jev confidently rejects a visible installer page under broad wording | Retained model/prompt failure; open |

## JEV-01: User services enter failed state at desktop logout

**Impact:** normal desktop logout left portal and notification services marked
failed in a surviving user manager. Interactive Firefox checks had completed;
this does not establish that portals failed during those interactions.

Observed sequence:

1. Install all six desktops and log into Aizome through GDM.
2. Exercise the editor, log out normally, then rotate through Tatami, GNOME,
   Plasma and Xfce using the same account. A diagnostic tty3 login remains
   active during part of this sequence, keeping the user manager alive.
3. In Xfce, open Firefox, render the synthetic page and edit its input.
4. Log out through Xfce's normal dialog, then inspect the user manager and
   journal from the diagnostic tty.

Expected: session services stop cleanly during normal logout; services needed
by a later session start without a stale failed state.

Actual: `xdg-desktop-portal-gtk.service`, `xdg-desktop-portal-xapp.service`
and `xfce4-notifyd.service` exited with status 1 at 22:00:58 America/Chicago
on October 27. The journal also shows GTK portal exiting with a display
broken-pipe error at the earlier Aizome logout, followed by successful
restarts. No failed system units or coredumps were visible in the inspected
diagnostics.

Evidence: [failed-unit listing](session-failures.png),
[logout journal](session-journal.png), [successful Xfce input](xfce-browser.png).
Xfce logout is `scenario-1793156458138-717278`; the diagnostic captures are
`screen-1793156546274-717674.png` and `screen-1793156629641-718577.png`.

Next reproduction: test Xfce alone with a fresh user manager, then repeat
rotation both with and without a persistent tty login. Capture complete
unit definitions and journals around logout and the following login. Repeat
against an exact current-source image before attributing this to Sekka,
Aizome, a desktop's service policy or an upstream portal implementation.
Do not suppress exit codes merely to make the check pass.

## JEV-02: Firefox session restoration fails after desktop rotation

**Impact:** launching Firefox after returning to Aizome showed its recovery
page instead of the previously open synthetic test page. Fresh navigation
still rendered the page and accepted input.

Observed sequence:

1. In the same installed account, use Firefox across Tatami, GNOME, Plasma,
   Xfce and vanilla Hyprland. The offline page and input are defined in
   [firefox-local-page.json](../../../tests/jev/firefox-local-page.json).
2. In Hyprland, edit the restored page's field to `Hyprland verified 1723`.
3. Leave Firefox open and use Hyprland's normal Super+M session exit.
4. Log into Aizome and launch Firefox through its command palette.

Expected for this recipe: the previous synthetic page is restored, as it had
been in Hyprland. Firefox's actual restoration policy and shutdown handling
must be checked before treating that recipe expectation as a product promise.

Actual: Firefox displayed “Sorry. We're having trouble getting your pages
back.” Jev stopped the restored-page assertion. A separate Ctrl+L navigation
loaded the page; the retained screenshot shows `Aizome verified 1723` entered
in its field. The model's later text assertion remained incomplete.

Evidence: [restore error](firefox-restore-error.png),
[fresh navigation and input](aizome-browser.png).
Hyprland exit is `scenario-1793157299828-726847`; the failed restoration recipe
is `scenario-1793157761109-740566`. The preserved disk contains the profile.

Next reproduction: start with a fresh Firefox profile, a local page, and a
single Hyprland-to-Aizome transition. Compare closing Firefox before logout
with leaving it open. Retain session-backup files and shutdown journals before
reopening or modifying the profile. The experiment did not exercise the
Restore Session button or establish data loss, corruption, or the component
responsible. Reproduce on a current image before filing an upstream defect.

## JEV-03: Stable-state assertions miss transient authentication errors

**Impact:** the harness can report an incomplete test even when the required
transient UI event occurred and Jev recognized it. This is a harness assertion
limitation; it is not evidence that GNOME accepted the wrong password.

Reproduction sequence on the retained candidate:

1. Enter GNOME, lock the session, wake the display and open the password prompt.
2. Type the synthetic wrong password `deliberately-wrong` and submit it once.
3. Assert that an authentication failure or request to try again is visible,
   using the driver's existing requirement for two fresh positive observations.

Expected: an assertion explicitly intended to detect a transient event records
that event once it is observed, while stable-state assertions keep their
separate persistence requirement.

Actual: one captured frame showed rejection and received probability 1.0.
The message disappeared before the required second positive observation, so
the step timed out. The failed step remains incomplete. A later correct
password returned to the browser in a separate scenario.

Evidence: [rejection frame](gnome-rejection.png),
`scenario-1793155586271-706793`, step `Q3.gnome-lock-reject`;
the original response and request metadata are retained in
[bug evidence](bug-evidence/manifest.json).

Proposed resolution: add an explicit bounded transient-event assertion type
that retains its matching frame, response, action and time window. Keep
stable-state behavior unchanged. Regression cases should cover one-frame
events, absent events and unrelated/stale error messages; recognition alone
must not stand in for proving authentication enforcement.

## JEV-04: Broad page wording produces a confident wrong judgment

**Impact:** a high model probability is insufficient to qualify a visual
assertion. Escalating only uncertain answers would miss this case.

Replay the retained tuning corpus with `jev-1.14.0`:

```sh
result-tools/bin/sekka-jev benchmark tests/jev/live-calibration.json \
  .work/jev/replay-live-failures /absolute/path/to/.env
```

The output directory must be new and its parent must exist. A replay is a new
model measurement and is not guaranteed to return the recorded probabilities.

Expected label, reviewed by Codex: the installer shows its GitHub configuration
page, including a GitHub username field and the heading “GitHub, SSH and Git.”

Actual retained response: “Does the installer show its GitHub page” received
0.07, an accepted negative answer at the 0.90 threshold. The narrower visible
field question received 1.0 and the heading question 0.99. The broad wording
may be ambiguous; this establishes a model/prompt failure against the authored
label, not an independently adjudicated API defect.

Evidence: [original screenshot](../../../tests/jev/fixtures/github.png),
[exact corpus questions and labels](../../../tests/jev/live-calibration.json),
and [original request metadata and response](bug-evidence/manifest.json).

Next validation: retain this failure without relabeling it to fit the response;
use atomic visible facts, separate held-out negative controls and independent
system evidence. Region-specific assertions also need evaluation: Aizome's
visible browser field text received probabilities below 0.90 in
`scenario-1793157895512-741831` despite the [retained text](aizome-browser.png).
Do not lower thresholds solely to turn these examples green.

## Additional observations and fixes already in the prototype

- Hyprland's launcher received `ffirefox` once when the authored input was
  `firefox` (`scenario-1793156965128-723430`). An assisted edit corrected it.
  Reproduce under lower host contention and inspect key event timing before
  assigning the cause to the harness or launcher.
- Native Aizome's Super+A select-all chord was missing from the original fill
  recipe, so Ctrl+A appended a filename. Explicit per-recipe chords and
  `fill_keeps_desktop_specific_select_all_chord` now cover that harness error.
- Normal QEMU exit exposed a short zombie/empty-command-line interval in the
  ownership check. The prototype now checks process start identity before
  accepting exit; `unreaped_exit_is_stopped_but_pid_reuse_is_rejected` covers it.
- Tiny GDM controls still needed assisted clicks. Locator cropping improves
  some observations but does not establish reliable unattended session choice.

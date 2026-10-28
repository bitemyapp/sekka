# Jev experiment coverage inventory

`coverage.json` is a source-derived **planning ledger**, not executable scenarios,
test results, or a release report. It describes Sekka commit
`bf4c4f5e4264eb5b3ce986cef274ea62587e4f78` and binds no ISO. All 1,551 entries
inherit `not-run`, except publication,
which is `not-applicable` because this experiment does not authorize publication.
The model cannot turn this plan into qualification evidence by answering yes.

The coordinator's live experiment uses public ISO revision `ed92471`, SHA-256
`98d5174b40b2a552eae8586ad2578eb93b989e9ff1f7116c38cf5f002de0f591`.
This inventory describes a different revision. It is **not guaranteed to match
that ISO**; `comparison_experiment` records the mismatch, not a candidate binding
or test result. Reconcile both source and installed-image differences first.

The pinned-source comparison is retained in
[`candidate-inventory-diff.json`](candidate-inventory-diff.json), with exact
commits, source URLs and content hashes. The public candidate pins remote Aizome
`ca4e289e7bdc9ffe9371f71416c7c3f04e325344`; its app inventory has **14 apps**, while
the current inventory has **15**, adding Koe. Its **151** literal Settei entries
precede the current **154**: General's Startup and Boot splash entries and
Desktop's auto-hide Deskbar entry are absent. Candidate Tatami uses the
**Audio** menu label and **wiremix/bluetui** terminal panels; current Tatami has
the Sound and Bluetooth GTK windows. The six-desktop list and 36-app installer
catalog are byte-for-byte unchanged between the two Sekka revisions. These are
source comparisons, not installed-image feature tests or pass results.

The source inventory contains six installer desktops, 15 native Aizome apps
(plus foot), 154 literal Settei settings entries across 19 panes, 210 literal
Aizome menu labels, 51 static Tatami menu entries, 175 documented Tatami binding
declarations, and 36 optional catalog applications. Source file hashes make
staleness detectable. Literal labels and declarations include distinct contexts;
they are not counts of independent product features or completed tests.

The ledger covers Q0–Q8, a storage matrix, normal GUI install cases, session
rotation, native apps, settings, Tatami entrypoints, Yukimi's two frontends,
each catalog application in each desktop, and separate hardware obligations.
The source extraction includes Shashin, Annai and Koe, which are absent from
the older example app list in the qualification skill.

Three install selections matter:

- Default Aizome only, with password login and the real Aizome greeter.
- Aizome, Tatami, Plasma, Xfce and Hyprland together, again using Aizome's greeter.
- All six desktops, including GNOME: Sekka deliberately chooses GDM so GNOME
  locking works. This cannot replace either Aizome-greeter case.

Native 4K BIOS storage is an expected pre-write rejection, not a boot success.
Fixture storage checks remain supplemental. A changed image, patched guest,
altered installer settings or replaced greeter does not inherit exact-image
qualification.

Schema version 2 normalizes repeated text and unrun metadata. Each `checks`
tuple is `[id, source, contract?]`; omitting the third value selects `feature`.
`source` references the `sources` map, optionally followed by `#L<number>`.
The four literal inventory tables use the column order in `inventory.columns`.
Every original check ID and literal inventory item is retained. The matrices
document the storage ID grammar and the three exact desktop selections.

To expand into a runtime ledger:

1. Copy `row_defaults` into a fresh record for each check. Add the tuple's ID
   and resolved source path/line, then select its contract.
2. Set `input_action` from the contract's `action` and `expected_result` from
   its `expected`. Apply any explicit contract `status`, `reason`, `recipe`
   or `evidence_class_required` override. Only publication changes the initial
   status; it remains outside this experiment's authorized work.
3. Resolve the contract's variant profile. Interpret matrix dimensions from
   the documented ID grammar; resolve labels/chords/settings from the literal
   inventory tables. Catalog IDs name the app and desktop explicitly.
4. Bind the exact candidate hash and VM identity in a durable run directory.
   Author concrete sample data, preconditions, bounded input recipes and
   independent expected observations. Expand applicable variants into separate
   subcase IDs, preserving the parent ID as their inventory reference. Record
   a concrete reason for any inapplicable subcase.
5. Leave observed results and evidence empty until execution. Fill each
   subcase's screenshots, input trace and diagnostics before assigning a result.
   Never promote a generic parent feature to a pass while required subcases
   are missing. Persistent changes need reopen, logout and cold-boot evidence.

An action completing, a window opening, or a high-confidence Jev answer does not
prove the behavior. Check the result and retained input/screenshot/log evidence.

Reconcile against the actual installed image before treating the inventory as
complete. In particular, collect its desktop entries; Settei's
`--list-settings --json`; native app menus including shared kit commands;
dynamic Tatami menus generated by `tool/src`; and actual boot entries. The
literal source extraction cannot enumerate device-dependent network entries,
printers, accounts, shortcut customizations, package-provided launchers or
runtime-generated menu labels. Preserve every discovered addition as unrun.

The catalog rows need application-specific recipes. For example, a browser
should render a known page and play local media; an editor should save/reopen
known content; a terminal should accept and display known text. Account-only
features need dedicated test identities and remain untested without them.
Never supply the host's personal credentials. The Jev key stays on the host
and must not enter a VM, screenshot, prompt, evidence file or command line.

Wi-Fi, Bluetooth, GPU, lid, battery, backlight, touchpad, camera, audio, printer
and physical USB behavior require suitable devices and explicit evidence.
Missing-device UI and software rendering are useful checks but do not establish
hardware operation. Do not silently mark those obligations inapplicable.

The experiment should measure model classification/localization accuracy and
false positives separately from product pass rates. Retain the original image,
question/answer, model version, confidence, latency and token usage for each
decision. Include wrong-screen and absent-target controls. Use Jev for narrow
observations and target localization within an authored finite action space;
stop on uncertainty, unrecognized screens, repeated actions or budget limits.
Human review and nonvisual assertions remain necessary for claims such as
correct package contents, disk independence and persistence.

# Jev vision and real Sekka VM experiment — 2026-10-27

Jev is promising as an inexpensive screenshot observer inside an authored
Rust state machine. This experiment installed the actual public ISO through
Genkan, booted its disk with the ISO removed, and exercised real desktop and
application interfaces. It required recipe corrections and Codex assistance;
it is not an unattended replay or ISO qualification.

The implementation and usage are in [the harness guide](../../jev-harness.md).
The complete private evidence remains at
`/home/callen/work/sekka-jev/.work/jev-evidence/`: original captures, exact
requests/responses, scenario versions, input events, manifests, QEMU arguments,
firmware variables and the disposable disk. Failed attempts remain present.
[The summary](summary.json) indexes the recorded runs and measured API costs.
The private `coordinator-ledger.json` retains each scenario step's candidate,
precondition, action, expected result and observation. No production
publication, guest patch, autologin, SSH, repository share or host device was used.

## Candidate and scope

| Item | Exact value |
| --- | --- |
| Public candidate | `20261022-ed92471`, 12,167,124,992 bytes |
| ISO SHA-256 | `98d5174b40b2a552eae8586ad2578eb93b989e9ff1f7116c38cf5f002de0f591` |
| Sekka revision | `ed92471d8cd92e80267bb69df1225da28054a91f` |
| Lock SHA-256 | `4562c4a233005aa03c30075190532f6a4cf5d23c80595cf2b97fa2035754d601` |
| Sidestep revision | `dae957e913376a26e642d6abee533eb755fb9092` |
| Candidate Aizome | `ca4e289e7bdc9ffe9371f71416c7c3f04e325344` |
| VM | QEMU 11.1.2, KVM, UEFI, read-only USB ISO, 100 GiB NVMe, 8 GiB RAM, 4 CPUs, software virtio GPU |
| Installed selection | All six desktops; Firefox; ext4; synthetic account; normal password login; America/Chicago |
| Jev | `jev-1.14.0`, official System One endpoint, experimental vision header |

The ISO was downloaded from the public immutable build URL and checked against
its public manifest. It was not rebuilt from this topic branch. The current
source inventory describes a newer commit: its 15 native apps include Koe,
whereas this candidate has 14. Three settings and Tatami's Sound/Bluetooth
frontends also differ. See the [revision comparison](../../../tests/jev/candidate-inventory-diff.json).
The 1,551-entry [planning inventory](../../../tests/jev/INVENTORY.md) is neither
1,551 executable tests nor completed coverage.

## What the retained evidence establishes

- Normal GUI installation selected all six desktops and reached completion.
  The confirmation/preparation/install scenario took 645.73 seconds and 275
  API calls. Its model estimate was $0.0363. Earlier installer stages were
  developed and resumed separately; `install-all-six.json` assembles them but
  has not been replayed as one uninterrupted scenario.
- UI shutdown stopped the installer VM. A new QEMU process booted the same
  disk with no ISO or share. `/` was `/dev/nvme0n1p2`, ext4, and the inspected
  systemd diagnostic showed zero failed system units.
- GDM listed exactly the six selected sessions, with Aizome initially selected.
  This is the intended GNOME coexistence case, not the required Aizome-greeter
  installation case.
- In Aizome, Jev drove palette search, Soroban arithmetic (`17 × 23 = 391`),
  Sumi typing, Save As and the Open picker, and normal logout. The saved
  document was independently read through Tatami's terminal after session
  rotation, establishing persistence across that logout/login.
- In Tatami, the normal terminal and browser shortcuts worked. Terminal
  output identified `Hyprland:Tatami`; user-unit and coredump diagnostics were
  empty at that point. Jev drove Firefox through a known offline HTML page and
  input field, then the system menu and logout activation.
- GNOME and Plasma authenticated normally, launched Firefox through their
  normal launchers, and completed the same page/input recipe. GNOME's lock
  rejected the deliberately wrong password; the correct password returned to
  the browser. The transient rejection was visually reviewed in its retained
  frame, but the automated two-observation assertion remained incomplete.
- Xfce authenticated through GDM and passed the same Firefox rendering/input
  recipe. Its normal logout returned to GDM. One launcher attempt was
  interrupted before its Enter action and resumed in a separately recorded run.
- Vanilla Hyprland authenticated and reported `XDG_CURRENT_DESKTOP=Hyprland`.
  Its new user manager had no failed units or visible coredumps. Jev dismissed
  the welcome dialog but abstained on its disappearance and on desktop/browser
  identification. A recorded assisted terminal shortcut and correction of one
  `ffirefox` launcher query were needed. Jev then launched Firefox, which
  restored the previous synthetic page, but abstained on the browser-window
  assertion. A separate fresh field edit to `Hyprland verified 1723` succeeded.
  The unexplained duplicated initial character needs reproduction under lower
  host contention before attribution to the app or QMP timing.
- Returning to Aizome authenticated normally. Firefox opened its session-restore
  error page, so the expected restored-page assertion stopped the recipe.
  A separate fresh navigation rendered the synthetic page and typed both
  `Sekka vision test` and `Aizome verified 1723`, visibly confirmed in retained
  captures. Jev's text probabilities remained below 0.90, so both automated
  field assertions stayed incomplete. Session restoration and these text
  recognition failures need separate regression cases.
- The final Aizome palette and Shut Down confirmation completed through Jev.
  The process-identity check confirmed that QEMU exited. The VM is stopped;
  its disposable disk and original evidence are retained.

Post-Xfce diagnostics found failed `xdg-desktop-portal-gtk`,
`xdg-desktop-portal-xapp` and `xfce4-notifyd` user units, all exiting with status 1
at the Xfce logout timestamp. The retained journal also shows an earlier GTK
portal broken-pipe exit at Aizome logout, followed by successful restarts in
subsequent sessions. There were no visible coredumps or failed system units.
This is a retained session-lifecycle finding, not evidence that the interactive
browser test failed. Reproduce with a fresh user manager and no diagnostic tty
before attributing it to a current-source regression.

These are narrow checks. Opening a desktop does not establish every app,
setting, portal, lock mode or session service. A diagnostic tty3 remained
logged in during part of the rotation; this run cannot establish complete
session-service isolation. All screenshot review and assisted input were
performed by Codex, not an independent human auditor. Early events named
`human-guided` are legacy labels for the same Codex assistance; newer events
correctly say `operator-assisted`.

## Accuracy, cost and failure cases

Across calibration, probes, successful segments and failed attempts, 736
recorded API responses used 2,271,487 input tokens, for an estimated **$0.0954**.
All recorded responses were HTTP 200 with usage present. Median HTTP latency
was 238 ms; the nearest-rank 95th percentile was 456 ms. This excludes VM
runtime and Codex supervision and is not the cost of an autonomous test run.

There were 85 scenario segments: 49 completed and 36 incomplete, with 125
model-observed steps, 36 blocked steps and 23 later steps left unrun. These are
development/recovery segments with overlapping coverage, not independent tests
or a product pass rate. Event logs contain 143 authored Jev input intents and
39 assisted inputs, including diagnostics and recovery.

At a 0.95 Noul threshold and 0.6 Choice/Box confidence threshold, the
seven-image historical calibration had 21 accepted correct judgments,
zero accepted wrong judgments and zero abstentions; all six box centers landed
inside their labeled click regions. Median API latency was 262 ms and measured
input was 22,630 tokens. This was a tiny tuning set reviewed by Codex, not a
held-out accuracy estimate.

At a 0.90 Noul threshold and the same confidence threshold, four additional
live-failure images produced six accepted correct judgments,
one accepted wrong judgment and five abstentions. The broad statement that
the installer was on its GitHub page was confidently rejected even though its
GitHub input was visible; asking about that input directly worked. Median
latency was 369 ms with 13,514 input tokens. High confidence is not calibrated
correctness. Neither corpus is a benchmark against a larger model.

The price estimate uses the documented $0.042 per million input tokens;
outputs are free under the [published model pricing](https://docs.typesafe.ai/models)
checked on this date. `summary.json` counts unique recorded HTTP responses,
including failed scenarios and experiments, rather than double-counting cached
observations. VM runtime, local CPU/GPU work and Codex supervision are excluded.
No end-to-end speedup or fully autonomous cost reduction has been measured.

Important observed limits and fixes:

1. Small text and dark controls caused frequent abstentions. GDM's tiny gear
   still required assisted clicks. A bounded, unscaled crop improved some
   controls, but did not make this reliable. Authored keyboard routes were
   substantially easier to use.
2. Box returns plausible coordinates for absent controls. Separate presence,
   uniqueness, occlusion and disabled-state checks prevented those clicks.
   Correct boxes sometimes had confidence below the chosen threshold.
3. Aizome's native select-all shortcut is Super+A. Ctrl+A appended a filename;
   the postcondition stopped the run. The fill action now supports an explicit
   per-recipe chord, with a regression check.
4. Tatami's menu click selected Logout; Return activated it. The original
   click-only recipe remained incomplete, and the corrected recipe retains
   both actions. GNOME logout completed through its normal countdown after
   the focused-button assertion abstained.
5. Display power saving can invalidate a screen assumption while authoring.
   Wake input and fresh preconditions are necessary. Identical screenshots
   reuse answers, but blinking cursors/clocks still generate fresh requests.
6. Two fresh positive observations are useful for stable states but miss
   transient messages. The wrong-password frame is retained without promoting
   the failed automated assertion to a pass. A future assertion type should
   explicitly distinguish transient events from stable state.
7. Actual QEMU shutdown exposed the short empty-command-line/zombie interval
   in `/proc`. Process ownership checks now handle exit without accepting PID
   reuse; a regression test reproduces an unreaped child.

## Remaining work

The next useful milestone is an unattended replay of a frozen install and
desktop smoke suite, against an unseen ISO, with a held-out failure corpus and
an explicit cost/time comparison. Add adaptive progress polling, screen-region
assertions for small text, transient-event assertions, and an explicit bounded
escalation path for uncertain observations. Keep release decisions independent
of Jev's confidence. Confident errors also need held-out negative controls and
independent assertions; escalating only low-confidence answers is insufficient.

The coordinator retains the full qualification scope:

| Gate | Full-gate status | Evidence and remaining scope |
| --- | --- | --- |
| Q0 provenance/capability | blocked | Exact public bytes and revision comparison retained; additional VM capacity unavailable. |
| Q1 live media | not-run | UEFI USB default boot observed; optical/BIOS and other boot entries remain. |
| Q2 installation | not-run | All-six GUI install observed; storage, encryption, cancellation and failure matrix remain. |
| Q3 boot/greeter | not-run | Disk-only GDM rotation and shutdown observed; Aizome greeter and full authentication/lifecycle cases remain. |
| Q4 Aizome | not-run | Calculator, editor and browser slices observed; complete apps/settings coverage remains. |
| Q5 Tatami | not-run | Terminal, browser, menu/logout slices observed; full feature and lifecycle cases remain. |
| Q6 apps/updates | not-run | Narrow Firefox checks across sessions; catalog apps, updates, rollback and recovery remain. |
| Q7 fix/repeat | not-run | Harness defects corrected; retained guest findings need isolated reproduction before product fixes/new-ISO reruns. |
| Q8 distribution | not-applicable | Publication was not requested; no publication action or gate approval. |

`not-run` here refers to the complete qualification gate. The narrow
observations above do not satisfy it.

Default-only Aizome-greeter installation, BIOS/optical installation, storage and
encryption matrices, complete native app/settings coverage, catalog apps,
updates/rollback, recovery, and physical hardware remain unqualified. A separate
BIOS/optical case was prepared but could not start under the workstation's
concurrent memory pressure; that is a resource block, not an ISO failure.
No publication gate is satisfied by this experiment.

Harness validation: formatting, 31 Rust tests, Clippy with warnings denied, and
the root flake's `.#tools` package build passed. These are component/package
checks, distinct from the real ISO observations above.

# Backlog status, 10 November 2026

Where the work through Sekka's open pull requests and issues stood at 16:45 UTC
on 10 November 2026, when it paused. Written so it can be picked up from another
machine. It is a working record, not user documentation: delete it once the
backlog is through.

The order of the whole backlog, with each entry's state, is in the tracker:
<https://claude.ai/artifact/XxWoyjzhr5XpKJBfjBwuq6> (private to the owner's
account). This document repeats what is needed to carry on without it.

`main` was at `aebca32e` (the nightly's inputs update). 78 pull requests and 41
issues were open.

## Ready to merge

Five pull requests are reviewed, fixed and green on the heads named. Merging is
by hand, as a squash, after checking that `Trusted PR / build and tests` is
green on that exact head.

| PR | What | Head | Know before merging |
|---|---|---|---|
| [349](https://github.com/bitemyapp/sekka/pull/349) | Automatic login through Sekka's own setting | `3e4ee4e9` | The Plasma Login guard (`7693a198`) is a workaround: see "Decisions". The live image's greetd unit changes with this; no ISO was booted. |
| [380](https://github.com/bitemyapp/sekka/pull/380) | The menu bar shows which apps keep the screen on | `7f8b9202` | The review's fixes are in. Not run: a real Flatpak app, GLES, hardware. |
| [293](https://github.com/bitemyapp/sekka/pull/293) | Yuki hardening | `f797e225` | Merging deploys nothing. Deploying is a separate step: follow the checklist in the PR and leave the deployer off. |
| [302](https://github.com/bitemyapp/sekka/pull/302) | Wi-Fi passwords kept off disk (the join window) | `7dcacf12` | Pairs with 304. |
| [304](https://github.com/bitemyapp/sekka/pull/304) | Wi-Fi passwords kept off disk (Walker) | `ea3b39c2` | Pairs with 302. |

Order:

- **302 and 304** merge in either order without conflict. After the first, rebase
  the second (`gh pr update-branch <n> --rebase --repo bitemyapp/sekka`) and wait
  for its run, so the combination is tested.
- **349 before 368.** They conflict in `apps/yukimi/yukimi-helper/src/main.rs`.
- 349 and 293 were tested merged with `main` at `aebca32e`. 380, 302 and 304 were
  tested with `main` at `40694fef`, one commit earlier; that commit changed only
  the `llm-agents.nix` and `oh-my-pi` inputs.

What each run covered, so the greens are read for what they are:

- CI builds the flake's checks on wx-workstation and reuses anything already in
  that builder's store. For 349 and 293 its pass reused builds the fixing agent
  had just made of the same derivations.
- The 30 Aizome VM checks under `legacyPackages` (`aizome-checks`) run nowhere
  automatically. Where a PR's own VM check matters it was run by hand, and the
  PR description says so.
- Nothing in this batch was tried on an ISO, an installed system, or hardware.

### What review found and what was done, in short

- **349.** Two independent reviews. The first found that a failed automatic
  login left no login screen; that is fixed for Aizome's login screen (greetd is
  started again and shows the greeter), for GDM and for Plasma Login. Eleven
  account states were run on each of the three in disposable VMs: every state
  that cannot log in by itself ends at a login screen. Also fixed: the maximum
  password age was off by a day; with an initial session greetd now stops after
  five starts in three minutes when its greeter cannot start; Settei reads the
  choice again before removing someone. Open: a password locked after the choice
  still logs in automatically; other PAM refusals still leave GDM and Plasma
  Login without a login screen; no CI check boots GDM or Plasma Login with
  automatic login.
- **380.** Review found a hold of Keep Awake's kind taken by another program was
  shown nowhere, a sandboxed app was listed as "Portal", Keep Awake fell back to
  a weaker hold when the compositor answered late, and one step of the VM check
  could not fail. All fixed, each with a test that fails without it. Open: turning
  Keep Awake off from a terminal may show its service as a row for about two
  seconds (reasoned, not seen).
- **293.** Two independent reviews. Fixed: the deployer only goes forward and only
  deploys commits on `main`; the webhook receiver no longer takes a slot to read a
  body; each unit carries its own address filter; a flaky test and a test hang;
  eight tests for behaviour nothing held in place. Two limits of the new receiver
  are stated in the PR and in `DEVELOPERS.md` and left as they are: see
  "Decisions". Never exercised: real cloudflared, GitHub, Discord, a real deploy.
- **302 and 304.** In VMs against real NetworkManager the password is in no
  process's arguments or environment and in no file of Sekka's. Also fixed:
  passwords with a backslash or a leading or trailing space, and enterprise joins
  sending the username (302). Not tried: real Wi-Fi hardware.

## Decisions waiting on the owner

1. **349, the Plasma Login guard.** Plasma Login reads no settings from `/run`, so
   the guard is a declared drop-in in `/etc/plasmalogin.conf.d` that links to a
   file in `/run` which only its unit writes. Suggested: keep it. To drop it,
   revert `7693a198` and say in the docs that Plasma Login is not covered when the
   password must be changed or has aged out.
2. **293, which connection the receiver closes when it is full.** Today it closes
   the one that has gone longest without verifying, which can be a real delivery
   that is still arriving, and of more than 128 deliveries at the same moment the
   excess are closed. Neither is a regression: the receiver running now does far
   worse in both cases. Suggested: leave 293 as it is and change the rule in a
   follow-up with its own review, since the obvious alternative (closing by time
   since the last byte) can be gamed by a sender that trickles bytes.
3. **293, the rest.** A separate Discord application for the monitor, which still
   shares the receiver's bot token. The deploy itself.
4. **[337](https://github.com/bitemyapp/sekka/pull/337)** is replaced by the merged
   372 and its author has been answered. Close it.
5. **[Issue 351](https://github.com/bitemyapp/sekka/issues/351)** stays open: the
   reporter's player was never established. Ask them, or close it against 375.
   Related: music alone in a Firefox tab now keeps the screen on; say if it
   should not.
6. **[333](https://github.com/bitemyapp/sekka/pull/333), per-application Aizome
   packages.** Rebased onto the merged 366 with its rebuild bug fixed, it beat 366
   in every measurement (cold build 5:37 against 6:10; a one-app edit 0:21 against
   3:44), at the cost of 91 derivations. Its own head (`c7cca9d7`) is slower and
   should not merge as it is. The rebase exists only on the desktop: see "Only on
   the desktop". Whether to push it as 333's new head is undecided.
7. **wx-workstation.** Its NVIDIA GPU has reported "GPU requires reset" since
   about 06:51 UTC on 10 November; test compositors there are being run with
   software rendering meanwhile. Separately, the CI job image needs a rebuild and
   both runner supervisors a restart before CI uses nix-fast-build (merged in 365);
   until then it falls back to `nix flake check`.
8. **Smithay [#2265](https://github.com/Smithay/smithay/pull/2265),
   [#2266](https://github.com/Smithay/smithay/pull/2266) and
   [#2267](https://github.com/Smithay/smithay/pull/2267)** wait on descriptions
   written by hand and ticked DCO boxes. Smithay does not accept AI-written text
   on its tracker.
9. **Whether the nightly should run some of `aizome-checks`.** The settings check
   had been failing on `main` since PR 176 with nobody told (fixed in 378).

## Next in order

Each rebase of someone's PR branch has needed the owner's go-ahead for that PR.

| # | Entry | PRs and issues | What to do |
|---|---|---|---|
| 17 | Yukimi update preflight | 368 | Rebase after 349, rerun, squash-merge. Its red run was `main` moving while it waited; nothing was built. |
| 18 | Utsushi train | 238 to 244 | Rerun the first four and merge in order; rebase the last three. 238 and 241 failed on a flake fixed since; 239 and 243 were never built. Blocks 268. |
| 19 | Cross-GPU copy wait | 281, issue 277 | Rebase over `main`'s rewritten `udev.rs`, act on the eleven review findings recorded in the branch, then merge. First among the PRs that touch `udev.rs` (317, 232, 328). |
| 20 | Incremental Sekka updates | 348 | Green on component checks only. Needs an ISO build with a VM install, and a VM update from an existing install through two updates, before merging. Then close 233 and rebase 182. |
| 21 | No GL fences on NVIDIA | 317 | Needs a rebase since 327. Headless only so far: run it as a real session for a day, then merge. |
| 22 | Shared-memory buffer ownership proposal | 356 | A draft holding one document. Read and decide. |

After those, in the tracker's order:

| # | Entry | PRs and issues | What to do |
|---|---|---|---|
| 23 | Stacking choices and tiling | 309 | Review, rerun, merge. |
| 24 | No idle settings polling | 332 | Review. |
| 25 | Kit fallback fonts | 263 | Rerun, merge. |
| 26 | Hotaru scrollback slowdown | 261 | Rerun, merge. |
| 27 | Lag recorder | 289 | Review, merge. |
| 28 | State-machine catalog | 282 | Squash-merge. |
| 29 | Tier 1 typestate | 290 | Review, rerun installation in VMs on the final head, merge. |
| 30 | Tier 2 typestate | 298 | After 290: rebase, first CI run, rerun its VM checks, answer its six questions. |
| 31 | Local model evaluations | 292 | Review, squash-merge. |
| 32 | Training scenarios | 299 | After 292: retarget to `main`. |
| 33 | Tasuke | 297, 322, 323 | After 292: retarget, first CI, review (243 files). |
| 34 | Panel Self Refresh switch | 328 | Retarget onto `main`. |
| 35 | Chanoma | 232 | Rebase (three conflicts), get a first real build, review. |
| 36 | Per-application Aizome packages | 333 | See "Decisions", item 6. |
| 37 | Memory-safety hardening | 256 | Rebase one hunk, CI, merge. |
| 38 | Session teardown | 260 | Rebase against `main`'s `session.rs`. |
| 39 | Signal keyring | 47 | Trivial rebase, run its check. |
| 40 | Yukimi evaluation reuse | 182 | After 348: rebase, dropping the capture removal 348 already does. |
| 41 | Yukimi storage accounting | 331 | After the Yukimi PRs: rebase and mark ready. |
| 42 | Yukimi specialisations | 166 | Sign off its five design choices, then rebase. |
| 43 | xdg-mime | 268 | After the Utsushi train: rebase. |
| 44 | GNOME apps out of Aizome | 236, issue 215 | Retarget to `main`. |
| 45 | Deskbar shortcut | 283, issue 209 | Try a build by hand, then review. |
| 46 | Agent skills | 156 | Rebase and review its new root path. |
| 47 | External monitor brightness | 280 | Rerun, then test with the Studio Display. |
| 48 | Hotaru windows and selection on scroll | 196, 221, issue 211 | Answer the contributor's 5 November offer to rebase 196; re-review 221. |
| 49 | Native-app mark | 357 | Give the design verdict. |
| 50 to 54 | Bugs to fix | issues 273, 345, 219, 231, 113 | Small to medium each. |
| 55 | Papercuts | issues 342, 295, 210, 214, 208, 339, 212 | Small each. |
| 56 | Small features | issues 341, 343, 218, 216, 217, 229, 230 | As time allows. |
| 57 | Larger requests | issues 344, 224, 223, 222, 320, 335, 124 | Decide which to plan. |
| 58 | Buck2 shared builds | 291, 334, 271 | Defer. Decide whether Bazel stays and whether 334 folds into 291. |
| 59 | Apple Silicon | 164, 336 | Defer until the hardware is available. |
| 60 | NVIDIA gaming and the rebuilt xe module | 137, 153 | Defer until their hardware checklists are run. |
| 61 | Koe captions | 294 | Defer. |
| 62 | Documents and experiments | 118, 117, 245, 201, 254, 262, 181, 99, 122, 130, 352 | Keep or drop each. |
| 63 | Superseded and finished PRs | 269, 251, 189, 265, 119, 42, 138, 233 | Close. Close 233 only once 348 merges. |
| 64 | encrypted-storage passphrase prompt | issue 112 | Close. |

Fork PRs get no CI: the trust gate admits only non-draft PRs from this
repository by authors with write access. Most older greens are for an older
`main`.

## Merged in this round

9 and 10 November, each as a squash: 369 (`a98a4ac7`), 366 (`c0d5e235`),
365 (`e97b52f7`), 363 (`df751e6c`), 353 (`24b14257`), 372 (`8cc35259`, in place of
337), 374 (`61d89963`), 376 (`c98a2582`), 325 (`c77d8366`), 340 (`b5a71fd4`),
330 (`3a17e7f5`), 350 (`876dc3ce`), 375 (`181bebe4`), 378 (`ed8027a5`),
327 (`40694fef`). Issues closed with them: 207, 228, 278, 319.

Left over from merged work, none of it started:

- 375: the portal's GTK backend sometimes keeps a hold for a caller that leaves
  at once.
- Follow-ups named in the merged PRs' descriptions: overlapping index walks
  (330), Apply keeping every display in Tatami (325), and the Super+/ gap.

## How the work has been run

For each PR: rebase onto `main`, an independent review that builds and tests on
wx-workstation, small findings fixed on the PR's branch as new commits on top
(never rewritten, never force-pushed), CI again, then the owner merges. A fix
that touched a boot path, the production service or a large feature got a second
review of the fix commits alone.

Standing instructions from the owner during this round:

- Nothing is built on the desktop (runnymede) while its benchmarks run. Builds
  and tests go through CI or `ssh wx-workstation`.
- Small review findings on a PR under review are fixed without asking, then
  reported. Design choices, rewrites of history and pushes to someone else's fork
  are asked about first. Merging stays the owner's.
- No merge commits; rebase instead. The release workflow stays inhibited.
- Yuki's production units on wx-workstation are never touched.
- No disk mutation or installer fixtures against host devices; disposable VMs
  only.

## Only on the desktop

Every PR branch above is pushed, and the worktrees for them are clean at the
pushed heads. These exist only on runnymede:

- Two local branches in `/home/callen/work/sekka`, not pushed:
  `ci/aizome-app-packages-on-366` (`568e5239`, the measured rebase of 333) and
  `fix/standalone-aizome-keyring` (`d5adea1f`, its keyring change, meant for a PR
  of its own).
- The triage notes, the 366-against-333 measurement and the review evidence, in
  the session's notes folder under `~/.config/Claude/side-session-notes/`.
- The review agents' logs and scripts on wx-workstation, under `~/sekka-work/`
  (`pr349-delta-review-20261110`, `pr293-delta-review-20261110`,
  `pr293-fixes-20261110`, `screen-holders-20261110`).

## Picking this up with Claude elsewhere

A new session has none of this round's context. Something like this gets it
started:

> Read `docs/backlog-status-2026-11-10.md` in bitemyapp/sekka and `AGENTS.md`.
> Check each PR under "Ready to merge" against its head and CI and tell me what
> has changed since. Then carry on from "Next in order", one PR at a time: ask
> before rebasing someone's branch, review independently, fix small findings on
> the branch as new commits, and leave merging to me.

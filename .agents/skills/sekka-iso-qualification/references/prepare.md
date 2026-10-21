# Q0 — prepare and inventory

1. Inspect current worktree, remote `main`, open relevant PRs, live QEMU/build
   processes and public `https://downloads.sekka.computer/nightly/builds.json`.
   Continue confirmed live handles; do not restart a build because a tool's
   observation timed out. Preserve other people's working changes in an
   isolated topic worktree.
2. Pin a clean source commit and all inputs. Read `flake.lock` for the actual
   Aizome and Sidestep source; check those repositories' `main` for relevant
   fixes, without assuming the pinned branch is `main`. Build with
   `nix build .#iso --no-update-lock-file --out-link result-iso`. Store the
   build log and resulting store path. Do not change dependencies mid-run.
3. Hash the single ISO with `sha256sum`, record `stat` size and lock hash, and
   verify public metadata when testing a download. A locally reconstructed
   image is not evidence about public bytes until its hash matches.
4. Inspect KVM access, RAM, disk space and existing workloads. A full catalog
   needs an ample disposable disk (at least 100 GiB is a practical starting
   point), 8–16 GiB guest RAM and 4–8 cores. Record actual values; test lower
   advertised memory separately. Leave enough resources for the host and other
   agents. Do not clean unrelated caches or kill unrelated VMs.
5. Build `.#tools` and `.#vm-fixture` separately. Read the current
   `respin-tools qemu-test --help`, `rust/src/vm.rs` and `rust/src/gui_input.rs`.
   Existing fixtures are supplemental; inspect their desktop list and bypasses
   before counting coverage. They may lag Aizome or silently exercise Plasma.
6. Expand the feature ledger from pinned Aizome `docs/apps.md`, `docs/shell.md`,
   settings source and app/menu entrypoints, and Sekka's
   `desktop/tatami/README.md`, `tool/src/main.rs`, `config/hypr/hyprland.lua`,
   `config/elephant/menus/`, and first-party `.desktop` files. Add newly shipped
   functionality even if this skill's examples omit it. Record inventories as
   evidence, including every offered session, app, setting and shortcut.
7. Record public releases and every object in the R2 nightly prefix, including
   orphaned builds absent from the index. Test each retained downloadable ISO;
   an older image does not inherit the newest image's results.

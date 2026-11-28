# Tatami file search refresh handoff

**Status: unfinished fix, draft review only.** This branch preserves the
Elephant watcher changes and test fixtures. The combined provider check fails,
and the GUI fixture exits before exercising file operations. Resolve the test
issues below and validate the behavior before marking the PR ready.

The published ISO `20261022-ed92471` excludes this work. Its
[qualification report](https://downloads.sekka.computer/nightly/builds/20261022-ed92471/qualification/report.json)
discloses that Tatami Files search may require reload after creation, rename or
deletion. Release [PR #98](https://github.com/bitemyapp/sekka/pull/98) is merged.
This follow-up does not change that ISO or enable automatic publication.

## Reproduce the user problem

In Tatami, open the normal launcher with Super+Space and use `.` followed by a
filename to search Files. After the initial index has loaded:

1. Create a uniquely named document in a visible directory under the user's home.
2. Search for its name without restarting Elephant. The original provider can
   omit the new file.
3. After refreshing the index, rename and then delete the document using the
   file manager. Search for both names after each operation; stale results are
   part of the reported issue.
4. Repeat with a new directory containing a document, then rename and trash the
   directory. The desired result is that children follow the current paths and
   disappear when deleted, without restarting the backend.

The provider fixture reproduces the missing-new-file case in a private home:
the original control misses `Documents/new-note.txt`, restarts its daemon, and
then finds it. This is the demonstrated workaround in the fixture; the GUI
sequence still needs a complete passing run. The intended refresh delay uses
Elephant's existing 2,000 ms debounce, so poll for results rather than asserting
immediately after a save.

## Implementation being handed over

Elephant defaults to `watch = false`. Enabling it alone also leaves weaknesses
in the old implementation: the default home root is absent from the configured
`SearchDirs` watch loop, deletion and update events use separate queues, and
subtree deletion uses SQL `LIKE` with a path prefix. The draft addresses these
along with directory moves and consistency with the initial `fd` scan.

| File | Purpose in this draft |
| --- | --- |
| [files.toml](../desktop/tatami/config/elephant/files.toml) | Enable Files watching for Tatami. |
| [files-watch.patch](../desktop/tatami/elephant/files-watch.patch) | Patch Elephant 2.22.0: watch roots and discovered directories, reconcile final filesystem state through one debounced queue, and replace subtrees transactionally with literal path boundaries. |
| [package.nix](../desktop/tatami/elephant/package.nix) | Apply the watcher patch alongside the existing application-search patch. |
| [test_launcher_files.py](../tests/test_launcher_files.py) | Exercise the real provider socket in a private home, including moves, deletes, ignore changes, path boundaries and injected scan failures. Also run an original-code negative control. |
| [launcher-files-vm-test.nix](../nix/launcher-files-vm-test.nix) | Exercise Walker, Nautilus and a test-only Mousepad in a Tatami VM; capture screenshots and require the Elephant PID to remain unchanged. |
| [flake.nix](../flake.nix) | Expose `launcher-files` and `launcher-files-vm` checks. The original control removes only `files-watch.patch` and supplies no Files configuration. |

The patch uses NUL-delimited `fd` output to preserve spaces and newlines. It
aims to retain old index entries and watches after a failed recursive scan,
retry that scan, continue pending child updates, and prune obsolete watches
only after success. These are design intentions with partial component
evidence, not a completed release guarantee. In particular, the initial full
index still calls `dropAll()` separately from insertion; the transactional
claim applies to subtree replacement.

## Recorded checks and blockers

The recorded runs are from October 21, 2026. They cover successive local WIP
iterations, not a clean committed release. Run them again after the fixes.

| Check | Recorded result |
| --- | --- |
| Elephant package | Built successfully in `package-build1.log`. |
| Earlier provider check | `check-build1.log` produced a check output; this predates the expanded failing check below. |
| Latest combined `launcher-files` check | **Failed** in `check-build2.log`. Its Nix build log reaches the patched-provider completion marker, then fails in the original control on `spaced name`. |
| `launcher-search` | Passed in `check-build2.log`. |
| GUI driver build | Built in `gui-build3.log`; building `.driver` does not execute the VM test. |
| GUI execution | **Failed** in `gui-fixed1/driver.log` at the initial Elephant service check, before the create/save/search workflow. |
| ISO containing this fix | None qualified or published. |

### Provider test branch error

The failure is:

```text
AssertionError: ('spaced name', [], ['/build/tatami-files-zbpd8rec/home/Documents/ spaced name '])
```

In `tests/test_launcher_files.py`, the atomic replacement and unusual-filename
cases are currently inside `if original:`. The whitespace and newline files
are created after the original daemon restarts, with watching still disabled,
so expecting them to appear live contradicts the negative control. Move these
positive behavior checks into the patched-provider branch and keep the
negative control focused on missing a new save until restart. If testing the
original scanner's filename handling separately, restart after creating those
fixtures and explicitly define its expected behavior.

The patched branch's final log message currently claims atomic-replace
coverage even though that operation is in the other branch. Do not count the
message as proof of that case. The current evidence does establish execution
of create/rename/move/delete, literal sibling boundaries, ignore changes and
scan failure/recovery assertions before the original control fails.

### GUI readiness check aborts instead of retrying

The recorded output is `inactive`, followed by:

```text
RequestedAssertionFailed: ... systemctl --user is-active tatami-elephant.service ... failed (exit code 3)
```

`wait()` calls a predicate through `user()`, which uses `machine.succeed()`.
An inactive unit therefore raises immediately rather than returning false
and allowing the intended polling period. Use a retry-capable command check
or inspect the exit status without throwing inside the predicate. Then, if
activation still fails, inspect the user journal and the UWSM session targets
in [module.nix](../desktop/tatami/module.nix). The existing failure alone does
not establish a broken production service. Preserve normal session activation
and capture the actual provider executable, loaded `files.so`, configuration
and PID before proceeding with GUI assertions.

## Resume efficiently

1. Work from `fix/launcher-files-watch`. Its original base is
   `4387dc6702d7c6481af0e00d38f0ce25e344f149` (PR #96), before release commit
   `ed92471d8cd92e80267bb69df1225da28054a91f`. Reconcile with current `main`
   before final review; preserve newer release and lockfile changes.
2. Correct the provider test branching and its completion message. Run the
   provider check first; investigate any failures in the now-reached atomic
   replacement and unusual-filename cases.
3. Correct the GUI readiness wait, then run the actual VM test. Review its
   screenshots and require create, rename, directory move and trash to update
   the launcher without an Elephant restart. Autologin and software graphics
   make this component evidence only.
4. Review the watcher patch for event ordering, watch lifecycle, error recovery
   and hidden/ignored-path behavior. Keep scan-failure injection and literal
   path-boundary tests. Run the existing application-search regression check.
5. Update the README and developer notes to match verified behavior, then
   request review. A future ISO containing the merged fix needs its own
   qualification; do not overwrite the already published immutable build.

Run these from the repository root, one stage at a time:

```sh
nix build -L .#checks.x86_64-linux.launcher-files
nix build -L .#checks.x86_64-linux.launcher-files-vm
nix build -L .#checks.x86_64-linux.launcher-search
```

## Evidence locations

On the originating workstation, the worktree is
`/home/callen/work/sekka-launcher-files-watch` and the logs are under
`/home/callen/work/sekka-iso-evidence/2026-10-21/launcher-files-watch/`.
These are local paths, not artifacts available from a fresh clone.

- `check-build2.log` records the failed combined check and successful
  application-search check. The fuller provider log is still available via
  `nix log /nix/store/hmb6lz3wy633nkad6bhxy29a67559ji7-tatami-launcher-files-tests.drv`
  on that workstation; store artifacts may be garbage-collected.
- `gui-fixed1/driver.log` records the premature service-readiness exception.
  `gui-build3.log` records a later driver build, not a passing execution.
- `provider-fixed1.log`, `provider-diagnostic.log` and
  `provider-diagnostic.py` are earlier development evidence, not substitutes
  for rerunning the committed fixtures.
- `elephant-source/` is a local working source tree with patch edits; do not
  treat it as a pristine upstream reference.

The draft preserves the implementation without repairing these blockers.
Repository whitespace review must account for unified-diff context lines in
`files-watch.patch`: their leading context space before Go indentation is
patch syntax, not an indentation edit to the Go source.

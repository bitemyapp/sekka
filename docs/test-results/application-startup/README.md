# Application startup: screenshots and mocks

Captured on 2026-10-29 for [PR #144](https://github.com/bitemyapp/sekka/pull/144).
All PNGs retain their original pixels and dimensions. No crops, scaling,
compositing, or generated artwork were applied.

## Runtime screenshots

These are real `grim` captures of a private Aizome compositor with a
1280 × 800 output, headless backend and Pixman renderer. The shell was built
from `f44342e693678bc28956ec59a594bb9d5a5388b2`; the installed compositor,
control client and integrated Koe were used alongside it. Exact executable
paths and observed outcomes are in [result.json](result.json); binary and
image hashes are in [manifest.json](manifest.json).

The test uses temporary home, configuration, cache and runtime directories
and disconnected D-Bus addresses. The failure entries are deliberate test
applications. This is component evidence, not an ISO installation or
physical hardware qualification, and it does not show the user's original
Koe failure being reproduced.

| Capture | What actually happened |
| --- | --- |
| [Failure and journal output](startup-details-live.png) | A process started, wrote `startup-fixture: daemon did not become ready` to stderr, and exited with status 23. The notification offers Show Details; the window shows the executable, lifecycle and actual journal entry. |
| [Missing executable](startup-missing-live.png) | The executable could not be found. The failure appears in history with its exact path and an explicit explanation that there was no application output. |
| [Integrated Koe window](koe-window-live.png) | The actual installed Koe application opened. Dictation stayed off. This fresh test profile has no models; it does not reflect the user's preserved model cache. |
| [Successful launch in history](startup-success-live.png) | The compositor observed Koe's window. The outcome is Window opened, alongside both failed attempts. Dismissing the failure notifications did not remove their launch history. |

The temporary desktop entry for Koe is called **Startup Window**. Opening
it neither enables dictation nor downloads a model. The two failure cards
were dismissed through their visible buttons before capturing Koe and the
successful launch history.

To reproduce from the repository root:

```sh
nix develop .#aizome -c cargo build --locked \
  --manifest-path desktop/aizome/Cargo.toml -p aizome-shell
nix shell --inputs-from . nixpkgs#python3 nixpkgs#grim -c python3 \
  desktop/aizome/scripts/test-startup.py \
  --shell desktop/aizome/target/debug/aizome-shell \
  --evidence .work/startup-evidence/pr144
```

The script accepts `--compositor`, `--ctl` and `--koe` for explicit binary
paths. By default it resolves the installed commands from PATH. It checks
the real palette launch path, failure notification, journal text, missing
executable and new Koe window, and shuts down its private processes.

## UI mocks

These 900 × 600 images are rendered by the actual Application Launches UI
using the toolkit's test harness. **The lifecycle events, PID, timing and
output are synthetic.** They illustrate the implemented states; they are
not screenshots of a running application or compositor and do not measure
startup duration. Both images say `Koe (mock)` and label their output.

| Mock | Intended state |
| --- | --- |
| [Waiting](mock-waiting.png) | A process exists, but no window has been confirmed after the 15-second check. The attempt remains Starting. |
| [Window opened](mock-window-opened.png) | A later window event changes the same attempt to Window opened and adds the observation to its lifecycle. |

To regenerate the mocks using the existing lifecycle test:

```sh
AIZOME_STARTUP_SCREENSHOTS="$PWD/.work/startup-evidence/pr144" \
  nix develop .#aizome -c cargo test --locked \
  --manifest-path desktop/aizome/Cargo.toml -p aizome-shell --lib \
  startup::tests::process_creation_is_not_window_readiness_and_slow_start_is_not_killed \
  -- --exact
```

The private-compositor regression and the lifecycle test both passed for
these captures. The capture additions change test and documentation code;
the runtime implementation is unchanged from the recorded source commit.

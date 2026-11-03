# Hotaru OSC 7 directory fallback: local latency

The directory fallback fix showed no measurable startup or per-command slowdown
in this local comparison. Without OSC 7 reports, the baseline returned the stale
startup directory in all 160 palette checks; the candidate returned the live
directory in all 160. Both versions passed all folder checks with reporting
Bash and Fish.

The change reuses the pane's existing validated-report state. Before the first
accepted OSC 7 report, Go to Tab checks `/proc/PID/cwd` on its background worker
when opened. A valid report takes precedence on subsequent openings, including
when the first report arrives late. Clearing the report restores the live
lookup. The last known directory remains a fallback if the process is gone.
No startup wait, capability probe, timer, or per-command polling was added.

## Results

Measured on **ganymede**, this local workstation, on November 2, 2026 Chicago
time (November 3, 04:35:16–04:36:27 UTC). The run took 71.2 seconds, including
warm-ups. Existing desktop applications were left running; compilation and
other test suites had finished before timing began.

All values below are milliseconds. Startup values are medians across 16 fresh
application processes per version and scenario. Command and palette values are
medians of the 16 per-run medians.

| Shell fixture | Startup baseline → candidate | Command-to-prompt baseline → candidate | Palette facts baseline → candidate |
| --- | ---: | ---: | ---: |
| Bash with Hotaru's OSC 7 hook | 47.796 → 47.777 | 8.701 → 8.671 | 0.146 → 0.135 |
| Bash without OSC 7 reporting | 47.825 → 38.390 | 8.595 → 8.573 | 0.165 → 0.206 |
| Fish with Hotaru's hook and native reporting | 40.786 → 34.580 | 8.359 → 8.212 | 0.189 → 0.229 |

Startup varied enough that the lower candidate medians are not evidence of a
startup speedup. The table below pairs runs within each ordering block. Each
estimate is the mean candidate-minus-baseline difference between block means;
the brackets are 95% bootstrap intervals over eight complete blocks, with
10,000 resamples and a fixed seed. For commands and palette opening, each run
contributes its median, rather than treating its individual samples as
independent repetitions.

| Shell fixture | Startup change, ms [95% interval] | Command change, ms [95% interval] | Palette change, ms [95% interval] |
| --- | ---: | ---: | ---: |
| Bash, reports | −1.039 [−5.346, +3.534] | −0.023 [−0.050, +0.004] | +0.009 [−0.033, +0.057] |
| Bash, no reports | −2.911 [−7.624, +1.928] | −0.022 [−0.036, −0.008] | +0.020 [−0.023, +0.059] |
| Fish, reports | −2.310 [−6.863, +1.809] | −0.055 [−0.181, +0.074] | +0.016 [−0.019, +0.058] |

The largest upper interval endpoint was +3.534 ms for startup and +0.074 ms
for command-to-prompt latency. The no-report palette's additional live lookup
had a paired estimate of +0.020 ms; its interval crossed zero. These are
uncertainty estimates for this workload and machine, not universal latency
guarantees. The tiny apparent command improvement is not an intended speedup:
the command-processing path did not change.

The harness also waited for the font worker to prepare the current snapshot.
Paired command-to-shaped-snapshot changes were −0.019, −0.024, and −0.052 ms
for reporting Bash, non-reporting Bash, and Fish respectively; all three
intervals included zero. Application process CPU time per command, including
the harness and background workers but excluding shell CPU time, also had
intervals crossing zero in every scenario. Individual samples, p95 values,
and these secondary metrics are retained with the results.

## Method and scope

- CPU: Intel Core Ultra 7 365; Linux 7.3.0, glibc 2.44.
- Battery discharging, `powersave` governor; no CPU affinity or frequency lock.
- Bash 5.3.20 and Fish 4.9.3 from the local system.
- Root-flake Aizome development shell, rustc 1.99.0, locked dependencies.
- Cargo `release`: optimized, no LTO, four codegen units; debug information
  and incremental compilation disabled, four build jobs. Both binaries used
  the identical formatted benchmark source and build flags.
- Baseline: `6c27fb0c40a5f19c730d6e599b6aca9708aabbff`.
- Candidate: `c19b9789702c90e506ddaa43f99faa13ec64d89d`.

The [Rust harness](../../desktop/aizome/apps/hotaru/examples/cwd_latency.rs)
starts real interactive shells through Hotaru's PTY, parser, application
runtime and font worker, using the native toolkit's headless test platform.
Bash uses a disposable rcfile; Fish uses `--no-config`. Reporting fixtures
source Hotaru's shipped integration hooks. No user shell configuration is
loaded or changed. The non-reporting Bash fixture installs no hook.

Startup is measured from constructing Hotaru and its native runtime until the
first prompt reaches the application's terminal snapshot. A second metric
waits for the corresponding font-shaped snapshot. This does **not** measure
the CLI's configuration/instance handling, dynamic-loader time, Wayland
window mapping, compositor presentation, or physical display/input latency.
It is a local component measurement of the shell-startup and command paths.

Every process runs ten warm-up commands and sixty measured commands. Each
command changes between two directories, prints a unique acknowledgement and
returns to a fixed prompt. This exercises an actual directory change and OSC 7
processing on every reporting-shell command. Timing ends when that prompt is
present in the current snapshot; reporting scenarios also require the expected
reported directory. The font-shaped metric waits for the current snapshot's
shaped result. The runtime is polled with 100 μs sleeps, and Hotaru retains its
existing 8 ms frame pacing. Results include scheduling and polling overhead.

After the command sequence, each process opens and closes Go to Tab ten times,
measuring the wait for asynchronously gathered facts and checking the folder.
It then exits its shell and removes its disposable fixture directory.

The [Python runner](../../desktop/aizome/apps/hotaru/tests/cwd_latency.py) warms
each binary/scenario once, then runs eight blocks, alternating ABBA and BAAB
(A = baseline, B = candidate). Scenario order rotates between blocks. This
produces 96 measured application starts, 5,760 measured command round trips,
and 960 palette checks. Each version/scenario has 16 starts, 960 commands,
and 160 palette checks. Cache warm-ups are excluded from the results.

To reproduce, build the example at each revision with the same command below,
copying the candidate's `examples/cwd_latency.rs` into the baseline checkout
first, and preserve the two executables outside the Cargo target directory:

```sh
nix develop .#aizome --no-update-lock-file -c env \
  CARGO_BUILD_JOBS=4 CARGO_PROFILE_RELEASE_DEBUG=0 CARGO_INCREMENTAL=0 \
  cargo build --locked --release --manifest-path desktop/aizome/Cargo.toml \
  -p aizome-hotaru --example cwd_latency

# Inside the Aizome shell, with Python 3 available:
python3 desktop/aizome/apps/hotaru/tests/cwd_latency.py \
  /absolute/path/to/baseline /absolute/path/to/candidate \
  apps/hotaru/resources /absolute/path/to/evidence \
  --bash /absolute/path/to/bash --fish /absolute/path/to/fish --blocks 8
```

## Correctness and checks

On the baseline, three tests failed by returning the startup directory:
`go_to_tab_names_the_program_in_front_and_its_folder`,
`go_to_tab_tracks_cd_without_osc7_reports`, and
`go_to_tab_accepts_a_delayed_first_osc7_report`. The immediate-report test
already passed. All four pass with the candidate. Tests also cover a cleared
report and a missing live process; valid reports retain precedence over the
PTY owner's directory, as needed for reporting nested shells.

Candidate checks, run locally through the root flake:

- Hotaru library: 65 passed, three existing diagnostic/helper tests ignored.
- Native integration: 47 passed, eight existing fixtures ignored.
- Cursor integration: two passed. Pixel-scroll integration: five passed.
- Package formatting and `git diff --check` passed.
- Release Clippy on all Hotaru targets passed with `-D warnings`, allowing
  `deprecated` and `clippy::chunks_exact_to_as_chunks` for pre-existing
  Rust 1.99 diagnostics. No unrelated source was changed to silence them.

This is component validation, not a VM, installed-system or physical-display
qualification. The separate clipboard-routing and narrow-tab-counter concerns
on PR #250 remain open.

## Evidence

- [Summary, machine metadata and binary hashes](hotaru-osc7-latency/summary.json)
- [All measured samples, compressed JSON](hotaru-osc7-latency/samples.json.xz)
- [Build command, revisions and source hashes](hotaru-osc7-latency/build.json)

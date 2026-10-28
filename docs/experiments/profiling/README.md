# Profiling research fixtures

These small fixtures support [the options report](../../profiling-options.md).
They do not alter Aizome, exercise an actual desktop interaction, or certify
battery cost. Run them on the workstation, not the laptop. No root access,
kernel policy changes, app injection or disk-device operations are required.

The checked-in JSON files are observations from 27 October 2026 on
`ssh wx-workstation`, under
`/home/callen/work/sekka-profiling-research-20261027`. `sekka/` is a separate
checkout of root commit `4dc885718ce48fd5555adff48908ab517efb62fe`;
`probes/` holds these fixture sources; `results/`, `perfetto-results/` and
`tracy-lifecycle-results/` hold their binaries and raw captures. The source
snapshot used for the first root-flake evaluation was identical tracked content.

## Inputs and scope

* Tracy **v0.14.1**, matching the root flake's profiler package version.
  Archive: `https://codeload.github.com/wolfpld/tracy/tar.gz/refs/tags/v0.14.1`.
  SHA-256: `bf4af567e9c7524d07f3caa745fad02fb33bd5694f11910750382d1efbb251c1`.
* Perfetto **v56.1**, upstream `perfetto-cpp-sdk-src.zip` release asset.
  SDK file hashes are in `perfetto-results.json`. Its `trace_processor`
  launcher came from `google/perfetto` at the same tag and fetched its pinned
  host executable. This was a deliberately fixed test version, not a claim
  that v56.1 is the latest release.
* Standalone probes used workstation `rustc 1.94.1`, GNU C++ and binutils;
  versions and build commands are in `workstation-results.json`.
* `nix-symbols.nix` used Nixpkgs
  `f20cd90e28fe0403873f473b29e42662cd86e8c4` from Sekka's root flake, including
  that input's Rust compiler and split-debug hook. It does not substitute an
  external first-party flake or modify any package source filter.

All compiler output is outside the repository. The scripts create result
directories explicitly; use new directories for another run. The idle probes
terminate only their own child process. Running the zone loop on a busy machine
is not a defensible desktop performance comparison; the raw measurements are
retained to show exactly what was done.

## Reproduce on the workstation

Copy this directory to the workstation, enter the remote shell, and prepare
Tracy and the Perfetto SDK in directories outside the checkout. The SDK archive
is distributed as a release asset, not as `sdk/perfetto.h` at the source tag.
For example, from a new experiment directory on the workstation:

```sh
mkdir tracy perfetto
curl -fL https://codeload.github.com/wolfpld/tracy/tar.gz/refs/tags/v0.14.1 -o tracy.tar.gz
sha256sum tracy.tar.gz
tar -xzf tracy.tar.gz -C tracy --strip-components=1
curl -fL https://github.com/google/perfetto/releases/download/v56.1/perfetto-cpp-sdk-src.zip -o perfetto/sdk.zip
unzip -q perfetto/sdk.zip -d perfetto
curl -fL https://raw.githubusercontent.com/google/perfetto/v56.1/tools/trace_processor -o perfetto/trace_processor
chmod +x perfetto/trace_processor

python3 probes/run-probes.py --tracy tracy --output results
python3 probes/run-tracy-teardown.py tracy tracy-lifecycle-results
mkdir perfetto-results
cd perfetto-results
python3 ../probes/run-perfetto.py ../perfetto
```

The first runner checks exact code/unwind preservation and source resolution
after splitting symbols, builds four Tracy configurations, observes each idle
process and interleaves tiny zone loops pinned to one allowed CPU. The lifecycle
runner starts and shuts down Tracy twice without connecting a server, then
observes the stopped state. It does not establish teardown during active
capture or simultaneous trace production.

The Perfetto runner builds the SDK fixture, observes three lifecycle states,
and imports both captures with Trace Processor. It asserts exactly 1,000
`synthetic-stage` slices and no `disabled-between-sessions` slice in each.
The 512 KiB setting caps the **trace buffer**, not total process memory.

For the Nix fixture, from this repository's root on the workstation (after
copying/committing the fixture files):

```sh
nix build --impure --max-jobs 1 --cores 2 --no-link --json --expr '
  let
    root = builtins.getFlake (toString ./.);
    probe = import ./docs/experiments/profiling/nix-symbols.nix {
      pkgs = root.inputs.nixpkgs.legacyPackages.x86_64-linux;
    };
  in [ probe probe.debug ]
'
```

Its `postFixup` checks `.text` and `.eh_frame` equality, build-ID debug-file
discovery, `addr2line` and execution. Query the reported runtime output with
`nix-store --query --requisites` and verify the reported debug output is absent.
`nix-symbol-results.json` records that additional closure check. The first
fixture attempt encountered SIGPIPE from an early-exiting `awk` under
pipefail; the final fixture consumes `nm` completely and passed.

## Interpretation limits

`/proc/PID/smaps_rollup` supplies PSS/RSS; CPU ticks and voluntary/involuntary
context switches are summed over tasks. Observations begin after two seconds
and cover eight seconds. Zero tick deltas are below measurement resolution;
context switches are not measured battery energy. PSS can vary with other
processes sharing pages. These are single idle intervals, not confidence
intervals or measurements of all Aizome apps combined.

The CPU sampling preflight failed under `perf_event_paranoid=4`. The machine's
default `perf` wrapper also lacks tools for its running OEM kernel. The alternate
installed perf binary reached the kernel permission check; that is not a
successful sampling compatibility test. See `sampling-preflight.json`.

No connected Tracy capture, Superluminal recording, native Aizome trace,
browser/Telegram/game integration, GPU timing, full Aizome build, VM install,
physical display test or battery test was performed. The report proposes the
work required to establish those separately.

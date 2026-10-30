# Jev-assisted VM testing experiment

`sekka-jev` is a Rust prototype for testing an unmodified Sekka ISO through
QEMU keyboard and mouse input. Code owns the scenario; Jev answers narrow
questions about screenshots and locates named controls. It does not invent
commands, read host files, choose disks, modify installer settings, or certify
a release. Publication remains inhibited.

The prototype lives in the existing `rust` tools workspace. Other component
workspaces and the tools package's narrow source filter remain separate.
The [October 27 experiment report](test-results/jev-2026-10-27/README.md)
records the actual candidate, assisted runs, accuracy failures and measured cost.
It uses the official TypeSafe endpoint, pinned `jev-1.14.0`, and the required
vision-preview header. These interfaces are experimental:
[images](https://docs.typesafe.ai/images),
[Box](https://docs.typesafe.ai/primitives/box),
[models and price](https://docs.typesafe.ai/models).

## Build and calibrate

```sh
nix build .#tools --no-update-lock-file --out-link result-tools
result-tools/bin/sekka-jev --help

# Or use the optimized local build; hashing a 12 GB ISO in debug mode is slow.
cargo build --release --locked --manifest-path rust/Cargo.toml --bin sekka-jev

mkdir -p .work/jev
result-tools/bin/sekka-jev benchmark tests/jev/calibration.json \
  .work/jev/calibration /absolute/path/to/.env
```

The key is `JEV_API_KEY`, supplied in the environment or an explicit dotenv
file. Dotenv is parsed as data; no shell interpolation is executed. The key
is passed to curl through a pipe, never argv, request artifacts or the VM.
The packaged binary supplies curl. The transport disables `.curlrc`, uses a
fixed HTTPS endpoint, and does not follow redirects. Keep evidence private:
screenshots and recipes can contain synthetic guest credentials and file data.
Use disposable identities only. The source `.env` is ignored, never copied to
the worktree or guest.

The seven-image calibration corpus consists of historical screenshots already
in this repository. Labels authored and visually reviewed by Codex include screen classes, completion
and absent-target controls, plus clickable rectangles. The report separately
records wrong answers, abstentions and click-center accuracy. This tiny tuning
set is not a held-out accuracy estimate, a benchmark against another model,
or evidence about a currently booted ISO. Extend it with actual failures and
reserve separate unseen cases before tuning thresholds further.

For a single screenshot, `probe IMAGE.png QUESTIONS.json NEW-DIRECTORY [ENV]`
accepts a named map of Choice/Noul/Score/Box questions. Use `screen` to refer
to the image. Each request stores its actual model, image hashes, questions,
HTTP status, response, latency and token usage.

## Start a disposable VM

Resolve QEMU and OVMF through this checkout's root flake:

```sh
nix build --no-update-lock-file --no-link --print-out-paths --inputs-from . \
  nixpkgs#qemu nixpkgs#OVMF.fd
```

Create a VM JSON config with these fields:

| Field | Value |
| --- | --- |
| `iso`, `iso_sha256` | Absolute regular ISO file and verified SHA-256 |
| `source_revision`, `lock_sha256`, `sidestep_revision` | Full source commit, SHA-256 of its lock, locked Sidestep commit |
| `qemu` | Absolute resolved `bin/qemu-system-x86_64` |
| `firmware` | `uefi` or `bios` |
| `ovmf_code`, `ovmf_vars` | Resolved `FV/OVMF_CODE.fd`, `FV/OVMF_VARS.fd`; null for BIOS |
| `media` | `usb` or `optical` |
| `disk_bus` | `virtio`, `nvme`, or `nvme4k` |
| `disk_gib`, `ram_mib`, `cpus` | For example 100, 8192, 4; check host capacity first |

```sh
result-tools/bin/sekka-jev start .work/jev/uefi /absolute/path/to/vm.json
result-tools/bin/sekka-jev capture .work/jev/uefi
result-tools/bin/sekka-jev status .work/jev/uefi
result-tools/bin/sekka-jev run .work/jev/uefi \
  tests/jev/live-welcome.json /absolute/path/to/.env
```

`start` hashes the ISO before launch, requires a new private run directory,
creates a sparse disk exclusively, copies writable firmware variables and
records provenance, resources, tool version, executable hash and full QEMU
argument vector. Only this created regular disk is writable. No host block
device, repository share, user folder or network forwarding is attached.
It uses KVM and a software virtio GPU with a preferred 1280×800 mode.
Firmware/desktop modes can differ. Screenshots retain their original
resolution; an explicitly recorded, aspect-preserving derivative fits Jev's
1568-pixel maximum when needed. Normalized box coordinates map back to the
original capture. Physical GPU behavior is outside this experiment.

Source metadata is operator-supplied, recorded and syntax-checked. The harness
verifies the ISO digest, but does not prove that a particular commit built it.
Bind that claim to the public manifest or retained Nix derivation separately.
The default QEMU user network is available to the guest; there is no SSH or
guest-agent instrumentation in this prototype.

## Scenarios and uncertainty

The examples in `tests/jev` show the schema. Every step has an ID, visible
precondition, fixed action, visible postcondition and timeout. Allowed actions
are `observe`, `click`, `fill`, `key` and `type`. `fill` locates the named input,
clicks, selects its contents and types. Its `select_all` chord defaults to
`["ctrl", "a"]`; native Aizome controls require `["meta_l", "a"]`.
Text is printable ASCII with a US
keyboard layout. QEMU key codes are lowercase (`f10`, `ret`, `ctrl`, etc.).
Unicode, drag/drop, scroll gestures and dynamic discovery are not implemented.

Questions should describe one visible fact. A broad judgment such as
"the desktop works" cannot stand in for an app journey. Clicks require:

- A satisfied precondition and a present control.
- Separate negative checks for disabled or covered controls.
- A Choice result of exactly one matching control.
- A finite, ordered, in-bounds box with sufficient confidence and plausible area.

The default recipes use 0.90 for judgment gates and 0.6 for Box confidence;
these are experimental thresholds, not calibrated guarantees. Box always
returns coordinates, even for absent targets. Presence checks are mandatory.
When the first box has valid geometry but low confidence, or the disabled-style
check is uncertain, the driver makes one
unscaled crop with padding around it and asks the locator and safety questions
again. It retains the crop and its original-image offset. Only a successful
second check can produce a click; the original five-second age limit still
applies. This adds detail without lowering the confidence threshold.
A screenshot older than five seconds cannot cause input. Every action is
logged before it is sent; no action is automatically repeated on a failed
postcondition. Stable after-action observations require two fresh captures,
which are **not independent evidence of correctness**.
This stable-state rule can miss transient messages: the GNOME wrong-password
message appeared in one retained frame but not two. Such a scenario remains
incomplete even when a reviewer can establish that the event occurred.
For a UI shutdown step, `expect_exit: true` replaces the screenshot
postcondition with a check that the owned QEMU process has exited. Zombies are
treated as exited only after verifying their process start time.

Identical image bytes and questions reuse the original answer within a run.
This saves calls and prevents repeatedly sampling an unchanged screen until
it passes. Cursor blinking, clocks and animation can change image bytes; this
cache is not a perceptual equivalence check. A future driver should distinguish
benign pixel changes from actual progress and detect transition loops.

Each scenario caps API attempts, input tokens and elapsed time. Before each
attempt it reserves the full 65,536-token context against the remaining token
budget; unknown usage retains that conservative reservation. HTTP 429/529 have
at most two retries with exponential delay. Other failures stop. A request has
a 45-second transport timeout, so an in-flight request can outlast the scenario
deadline by that much. Metrics distinguish HTTP latency from total run time.
The cost estimate uses the documented October 27 price of $0.042 per million
input tokens; it is not a billing statement and should be updated if prices
change.

## Evidence and disk-only boot

Each scenario gets a new directory with the exact recipe, append-only events,
requests/responses, metrics, per-step results and original screenshot paths.
Interrupted or uncertain runs remain incomplete; later steps remain unrun.
An `observed` result is a model claim requiring review; its qualification status
stays `not-run`. A human must inspect captures and obtain independent facts for
storage, package contents, authentication, persistence and updates. Model
confidence is not a product pass rate.

After installation, shut the guest down through its normal UI, then verify
`status` says stopped. `shutdown` is an ACPI cleanup request with a bounded wait;
it does not count as UI shutdown coverage and does not forcibly kill a hang.

```sh
result-tools/bin/sekka-jev boot-disk .work/jev/uefi
```

This refuses a running VM and starts a fresh process with the existing disk
and firmware variables but no ISO or share. It records another argument vector.
PID start time and command-line identity protect against reusing a stale PID;
disk inode/device identity rejects replacement and symlinks. A process lock
allows only one input driver per run. It is not safe to run other QMP input
clients against the same VM concurrently.

For an explicitly assisted experiment, `manual RUN key ctrl+alt+f3`,
`manual RUN type 'synthetic test text'`, and `manual RUN click X Y` retain
before/after captures and label the input `operator-assisted`. Inspect a fresh capture
before choosing coordinates. Manual input is not Jev-controlled evidence; its
arguments are recorded, so use only disposable credentials and test data.
This can diagnose a stopped scenario without changing its incomplete result.

The full [ISO qualification skill](../.agents/skills/sekka-iso-qualification/SKILL.md)
still owns Q0–Q8. The [coverage inventory](../tests/jev/INVENTORY.md) preserves
1,551 unexecuted planning entries, including all six desktops and app/settings
inventories. It is not 1,551 runnable tests. Success, cancellation, error,
persistence, update/rollback and hardware cases need concrete recipes and
independent evidence. GNOME/GDM coexistence needs a separate installation from
the Aizome-greeter cases. The current experiment binds no publication gate.

## Component checks

```sh
cargo fmt --manifest-path rust/Cargo.toml --check
cargo test --locked --manifest-path rust/Cargo.toml
cargo clippy --locked --manifest-path rust/Cargo.toml --all-targets -- -D warnings
nix build .#tools --no-update-lock-file
```

These check the harness and package. They do not build or qualify an ISO.

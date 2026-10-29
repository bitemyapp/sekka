# Hotaru next work

Recorded 2026-10-28 against usable checkpoint `95bc307f` on
`codex/hotaru-rust-terminal`. The user deferred further porting and optimization.
This handoff records work to resume later; it does not change the qualified
application or establish complete Ghostty compatibility or performance parity.
See [the usable checkpoint](usable-checkpoint.md) for launch instructions,
package identities, test evidence and current limitations.

## 1. Reconcile Sekka main before merging

Inspected `origin/main` at `ff1b15b0`. Commit `ad455d75` moved Aizome's toolkit
into `desktop/aizome/sukumo` and removed the Sidestep submodule and development
patch machinery. This branch still uses the earlier layout. A read-only merge
check found conflicts in:

- `desktop/aizome/Cargo.toml`
- `desktop/aizome/nix/default.nix`
- `desktop/aizome/nix/package.nix`
- `desktop/aizome/scripts/dev-environment` (deleted on main)
- `tests/test_aizome_development.py` (deleted on main)

Keep the PR draft until this integration is complete. Port the behavior in the
five tracked `upstream/sidestep-{clipboard,input,mime,grid-font,sprites}.patch`
files into the first-party Sukumo sources, reconciling changes already supplied
by Utsushi and Omperor. Sukumo retains the `sidestep-engine`/`sidestep-ui` crate
names and `SIDESTEP_*` environment variables; that naming alone is not a reason
to restore the deleted submodule. Preserve main's session teardown changes.

Retain the independent terminal/I/O workspace, the native Aizome application,
the focused Hotaru Nix package, dependency notices and packaged resources.
The locally dirty `vendor/sidestep` is the applied tracked patch series, not a
new submodule commit to stage. Its current gitlink is
`7bfacbbbc9cd70cf3442def70368e254f3c3712d`.

**Gate:** build and test the reconciled source on the workstation, run the
focused package and six native Wayland suites, and publish fresh source,
executable and package hashes. Historical receipts qualify only their recorded
source. Check the combined Aizome package and applicable repository checks too.

## 2. Qualify the installed Aizome application

The checkpoint passed private Sway tests; it has not newly qualified an Aizome
VM, installed ISO or physical desktop. Test the packaged menu entry and command,
fonts/resources, scaling, shell startup and terminfo, tabs/splits, the Settings
window and shortcut recording (`tests/wayland/settings.py`, so far in private
Sway only), search and link activation in Aizome. Exercise large
plain/HTML copy and paste
while output and UI input continue, including stalled readers, cancellation,
pane closure and session shutdown. Keep the existing default terminal until
replacement is explicitly requested and qualified.

**Gate:** preserve the actual Aizome build and Hotaru executable identities,
results and failures. The user-local development launcher depends on ignored
artifacts and runtime roots in the original worktree; it is not a portable
installation supplied by a fresh clone. Remote shells require Hotaru terminfo
or the documented `TERM=xterm-256color ssh host` fallback.

## 3. Finish native OSC 52 without weakening clipboard responsiveness

Ordinary user copy/paste is enabled and qualified. OSC 52 remains disabled in
the shipped application. The isolated prototype contains tested codecs and
ordered core events, but no completed native permission/transport integration.
Use the committed [actual Ghostty reference observations](osc52-reference.md)
to distinguish compatibility from deliberate corrections.

- Reconcile the prototype plan first: its later coalescing checklist predates
  the ordered `feed_until_event` API described at its beginning.
- Bound total queued ownership, request counts and retained bytes. Preserve
  request ordering, raw selection tags and original payloads; reserve complete
  PTY responses so congestion cannot leave a partial escape sequence.
- Implement read/write allow, ask and deny policies, with read-ask/write-allow
  defaults. Bind approval to the original pane, request and bytes. A bounded
  lossy preview is display-only and must not replace the authorized payload.
- Capture the read offer before replacement destroys it; approved reads must
  still return the captured bytes after external clipboard changes. Enforce
  caps before cloning a potentially large cached local clipboard payload.
- Keep pipe I/O and conversion on workers. Implement raw-byte and PRIMARY
  plumbing: writes to `c` use the clipboard, `s`/`p` use PRIMARY; reference reads
  use the standard clipboard and retain the requested response tag. Document
  normalization of unsupported tags from the reference evidence.

**Gate:** test exact bytes, policy outcomes, changing offers during approval,
request floods, stalled peers, bounded retained memory, output/input progress,
cancellation and teardown in the real native application. Preserve invalid
UTF-8 and NUL bytes through approval and reply; this intentionally fixes the
reference GTK conversion behavior. Codec/core tests alone do not enable OSC 52.

## 4. Integrate horizontal margins and validate the C0 fast path

The preserved margin implementation adds DEC mode 69/DECSLRM and associated
scrolling, origin, tab/CR, query, reset and saved-mode behavior. Integrate the
original implementation, wide-character DCH correction and final C0 change as
narrow hunks. The DCH fix erases vacated cells before copying the owned snapshot.
Preserve the current `Row::wrap_continuation` getter; the older prototype lacks
it. Applying only the final C0 optimization does not add margin support.

The prototype passed 167 tests in both debug and release and strict Clippy.
Its 5,184 reference comparisons had no ordinary failures and recorded 30 named
pending-wrap differences where the reference depended on input chunking.
The final eight-case timing sample was promising, but the full 40-case follow-up
was never started. It does not establish broad parity.

**Gate:** rerun correctness on the integrated source, retain the named reference
differences, and run the complete 40-case follow-up with equal workloads and
recorded CPU/load conditions. Reject or revise regressions before advertising
the optimization. Preserve raw timings and failed trials.

## 5. Resume the GPU prototype separately

The usable application uses the CPU renderer. Keep GPU work opt-in while
reviewing scene ownership, admission budgets, worker cancellation, DMA-BUF
import/release and fallback on every failure path. The prototype's 768 MiB
logical reservation is not measured RSS and excludes some driver/font costs.
Port into Sukumo after integration, and overlay the current application: the
prototype predates automatic links and the final packaged checkpoint.

NVIDIA rejected the tested linear AB24 export; SHM fallback passed four scales.
That is not successful hardware DMA-BUF rendering. Historical Intel evidence
comes from an older diagnostic. Scene interpreter comparisons are also not
hardware pixel proof. Rerun the corrected mouse fixture and final CPU-worker
failure recovery with a freshly hashed executable: the former was not rerun,
and the latter has unit/build validation only.

**Gate:** qualify supported format/modifier negotiation, successful actual
DMA-BUF presentation and exact pixels, blocked-driver responsiveness, fallback,
large clipboard traffic, teardown and measured memory. Retain refusal evidence
and stable CPU fallback. Requalify the package before enabling the path by
default; whole-application performance comparisons remain separate.

## 6. Complete compatibility and performance acceptance

Use [`upstream/modules.json`](../upstream/modules.json) as the source inventory;
pending or partial modules must not be counted as a finished port. Remaining
work includes VT modes/replies, Kitty graphics and clipboard, tmux integration,
config/theme compatibility, semantic command navigation and remote shell hooks,
broader terminfo, accessibility, remaining sprites and font behavior. Automatic
links still need full scrollback context and configurable patterns; selection
and live-search equivalence also remain partial.

History RSS remains above the reference despite compression. Revisit page/arena
storage and malformed-input costs using equal retained content and the committed
memory/UTF-8 audits. Measure startup, sustained throughput, input/output tail
latency, frame pacing and resident memory on the same host with equivalent
features and workloads. Parser microbenchmarks cannot establish application
parity. Preserve long-running fuzzing and unsafe-code ownership/layout checks.

**Gate:** close the relevant feature inventory, qualify desktop clipboard stress
and races, and demonstrate the original efficiency requirement with reproducible
whole-application evidence before calling Hotaru a complete replacement.

## Preserved prototypes and recovery

These directories were preserved on both the laptop and `wx-workstation`.
They are **outside this Git branch**; their code and test results do not describe
the shipped checkpoint. Recover and verify them before reuse, or reimplement
from the requirements above. Repository-relative links in this document describe
the tracked application and evidence available to a fresh clone.

| Work | External source / handoff |
| --- | --- |
| Margins and C0 | `/home/callen/work/hotaru-margin-controls-20261028`; `/home/callen/work/hotaru-margin-attribution-20261028/README.md` |
| OSC 52 | `/home/callen/work/hotaru-osc52-native-isolated/artifacts/PLAN.md` |
| GPU | `/home/callen/work/hotaru-gpu-native-isolated/artifacts/GPU-HANDOFF.md` |
| PRIMARY API | `/home/callen/work/hotaru-primary-clipboard-isolated` is a copied baseline only; implementation was not started |

Frozen archive SHA-256 values, checked locally when writing this handoff:

- `hotaru-margin-attribution-20261028/margin-c0-evidence.tar.gz`:
  `9f114aadd818f7aead92046451c52861a10dce9ce23c82689fb2c569c48e82fd`
- `hotaru-gpu-native-isolated/artifacts/source-freeze.tar.gz`:
  `b5c70a2167082b012b1d49a716eef1c78f094b1e04f4c68cbb2c6439c22cbe6e`
- `hotaru-gpu-native-isolated/artifacts/source-freeze.json`:
  `a87b5f66f4f226204af44cc4d140c8a3c50ce8544e9fa8fce86536c5f3e21c4d`

## Execution contract when work resumes

Run builds, tests, benchmarks and GPU diagnostics through `ssh wx-workstation`.
Use `flock -x /home/callen/.cache/hotaru-workstation.lock`, isolated target
directories and explicit source/executable identities to avoid shared-cache
mixups. Serialize this effort's timing trials and builds; unrelated workstation
activity does not require waiting for the whole host to become idle. Pin the
chosen benchmark CPU, record its sibling/load and reject contaminated samples.
Keep raw failures and exact hashes alongside successful evidence. Local source
editing and evidence inspection are fine. No deferred experiment needs to run
to publish this handoff, and publication does not change the desktop default or
deploy a release.

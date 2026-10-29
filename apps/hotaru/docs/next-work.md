# Hotaru next work

Recorded 2026-10-28 against usable checkpoint `95bc307f` on
`codex/hotaru-rust-terminal`. The user deferred further porting and optimization.
This handoff records work to resume later; it does not change the qualified
application or establish complete Ghostty compatibility or performance parity.
See [the usable checkpoint](usable-checkpoint.md) for launch instructions,
package identities, test evidence and current limitations.

## 1. Reconciled with Sekka main's Sukumo (done 2026-10-29)

Main at `a6c6a624` is merged: Aizome's toolkit is Sukumo in
`desktop/aizome/sukumo`, with no Sidestep submodule, flake input, development
patch machinery or `aizome-development` check. The six tracked Sidestep patches
(clipboard, input, MIME, grid font, sprites and the primary selection) were
merged three ways onto Sukumo's `sidestep-engine` and `sidestep-ui` against
their base Sidestep `7bfacbb`, beside Utsushi's and Omperor's changes. Two of
Omperor's clipboard methods used the old clipboard record and now read this
program's own MIME bundle. The patch files are gone; Git history keeps them for
the receipts that name them. `aizome-comp-core`'s build script also finds path
packages patched in `[patch.crates-io]`, such as Hotaru's vendored `simdutf8`,
which the compositor's dependency closure includes. The focused Hotaru package
and the Aizome package build from the in-tree toolkit.

## 2. Qualify the installed Aizome application

The checkpoint passed private Sway tests; it has not newly qualified an Aizome
VM, installed ISO or physical desktop. Test the packaged menu entry and command,
fonts/resources, scaling, shell startup and terminfo, tabs/splits, the Settings
window and shortcut recording (`tests/wayland/settings.py`), programs' clipboard
use, the primary selection and panes ending (`tests/wayland/selections.py`),
both so far in private Sway only, search and link activation in Aizome. Exercise large
plain/HTML copy and paste
while output and UI input continue, including stalled readers, cancellation,
pane closure and session shutdown. Keep the existing default terminal until
replacement is explicitly requested and qualified.

**Gate:** preserve the actual Aizome build and Hotaru executable identities,
results and failures. The user-local development launcher depends on ignored
artifacts and runtime roots in the original worktree; it is not a portable
installation supplied by a fresh clone. Remote shells require Hotaru terminfo
or the documented `TERM=xterm-256color ssh host` fallback.

## 3. Add Kitty's clipboard protocol beside OSC 52

Updated 2026-10-29. OSC 52 is on in the application: writes allowed and reads
asked by default, with Allow, Ask and Never for each in Settings. The core takes
the prototype's raw one-byte selectors, byte payloads, sentinel-inclusive
capture bound and ordered prefix (64 requests and 8 MiB per bounded feed, which
the PTY worker drains after each). `feed_until_event` was not brought over:
nothing in the application needs it while that prefix holds. The PTY worker
decodes writes (padding optional, trailing bits accepted, as Ghostty's) and
prepares their exact bytes; each pane may have 16 requests holding 32 MiB in
waiting, counted by tickets that free their place when a request is answered or
dropped. Reads are captured on the selection worker before anyone is asked,
answer with at most 6 MiB, and go to the PTY as one reserved write. Approval is
bound to the request it shows; the preview is display-only. The toolkit gained
the primary selection (`zwp_primary_selection_device_v1`) and raw-byte offers
and reads in `upstream/sidestep-primary.patch`; `p` and `s` read and write the
primary selection, a deliberate difference from Ghostty, which reads the
clipboard for all three. Unit, harness and private-Sway tests
(`tests/wayland/selections.py`) cover exact bytes with NUL and invalid UTF-8,
both selections, the policies, asked reads allowed and refused, refusal past a
pane's bounds and tag routing. Use the committed [actual Ghostty reference
observations](osc52-reference.md) to tell compatibility from correction.

The stress cases are now tested in the real application (`selections.py`):
the clipboard changing while a question is up (the captured answer is sent), 200
reads at once (one asked, 199 answered empty: past the core's prefix, a pane's
bounds or the queue, or while the question waits), an owner that never sends
(answered empty without a question, the UI working meanwhile), a pane closed and
Hotaru quit with reads pending, and 48 one-MiB writes (the PTY kept being read,
the last write won, resident memory 41 to 50 MiB). An empty or unreadable
clipboard is answered at once without asking. Byte reads wait out a new owner's
read-ahead rather than answering from the previous owner's cached bytes, a race
the stalled-owner case found in the toolkit.

**Gate:** Kitty's clipboard protocol (OSC 5522: several MIME types, chunked
writes, aliases, remembered permission and mode 5522 paste events), which the
pinned Ghostty implements, is the remaining clipboard protocol.

## 4. Recover the last horizontal-margin cost on Unicode text

Updated 2026-10-29. DEC mode 69/DECSLRM is integrated: the preserved candidate's
margins, wide-tail DCH correction and C0 fast path, merged three ways onto this
branch with the `Row::wrap_continuation` getter kept, and `clear_screen` moving
whole lines past a program's margins. On the integrated source the oracle again
makes 5,184 comparisons against the pinned library with no failures and the 30
named pending-wrap reference differences, and the core suite passes. Beside the
candidate's C0 split, `print`, `try_grapheme` and `prepare_wrap`'s wrap now keep
the pre-margin code on the margin-free path, with margin copies out of line, and
`width_effect` is always inlined. Instruction counts match the previous head.

Paired feed timings ([evidence](../upstream/horizontal-margins/README.md)) on wx
put ASCII at parity and styled and cursor-heavy input 1–6 % faster, but mixed
Unicode text 3 % slower in legacy width and 12 % slower with mode 2027, which
native panes use; on the Intel laptop that case is +2 % cycles. Both are
instruction-equal: the Intel counters show more decoded-uop-cache misses, so
the cost is code layout around the bulk UTF-8 and grapheme paths.

**Gate:** find the layout cause (unprivileged `perf` is unavailable on wx; the
laptop allows it) and bring mode-2027 Unicode within noise without moving the
other cases, then run the full 40-case comparison with equal workloads and
recorded CPU/load conditions. Preserve raw timings and failed trials.

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
| OSC 52 | Integrated (section 3); its codec and comparisons remain in `/home/callen/work/hotaru-osc52-native-isolated/artifacts/PLAN.md` |
| GPU | `/home/callen/work/hotaru-gpu-native-isolated/artifacts/GPU-HANDOFF.md` |
| PRIMARY API | Superseded: `upstream/sidestep-primary.patch` implements it in this branch; `/home/callen/work/hotaru-primary-clipboard-isolated` was a copied baseline only |

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

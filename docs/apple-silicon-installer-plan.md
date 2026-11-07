# Apple Silicon installer implementation plan

Status: written on 6 November 2026 and implemented on
`feat/apple-silicon-installer` the same week, as far as source, component
tests, VM evidence and macOS type-checking reach; see
[DEVELOPERS.md](../DEVELOPERS.md#sekka-installer-for-mac) for what each part
has and hasn't been run on. The native Mac builds, the Recovery step and
every hardware gate still need the Mac. Encryption is
[deferred](apple-silicon/encryption.md). Publication remains inhibited.

The Sekka baseline is `4882dc33652612e7475f467f6373d8b607e5407c` on
`fix/apple-silicon-aizome`, rebased onto main
`5d1d471b45af2e0d40b96c70485e34e093ceb5dc`. The Omarchy source reviewed is
`omacom/omarchy-mac-installer` at
`c07e54c2134a8ec1415e7e2b917589ae1d3d7dc9`. External observations below refer
to that snapshot; recheck upstream changes when implementation starts.

## Intended result and scope

A Mac owner downloads a Sekka macOS app, reviews a plan to install alongside
macOS, authorizes the changes, follows a guided Recovery step, and reaches
Sekka's account setup and Aizome login. The installed system has a working
configuration, Yukimi updates, retained boot generations, and useful failure
diagnostics from its first boot. Tatami remains available.

The initial hardware target is the 14-inch MacBook Pro M3 Pro, Mac15,6,
J514S/T6030. Support is an exact board, firmware, boot stack, and image claim.
It does not extend to all M3 machines because they share a chip generation or
firmware version. Additional boards need separate platform work and evidence.

Keep the existing PC installation experience, `calamares.*` compatibility
options, separate Cargo workspaces, narrow source filters, and GUI/helper
boundaries. The macOS app is a companion to Genkan. Kakehashi continues to write
external installation media. First-party source stays local and is integrated
through the root flake; upstream Asahi/Omarchy trees remain pinned external
dependencies. A long-lived upstream fork belongs outside Sekka.

The complete effort includes a production image, first-boot setup, authenticated
delivery, a macOS app and helper, a Sekka engine adapter, native Recovery,
durable journals, and qualification. Encryption needs its own design and tests
before it can be offered. Automatic removal and broad model support follow the
initial installation path.

## Starting point

| Area | Present at the baseline | Work required |
| --- | --- | --- |
| Platform | Root-flake aarch64 packages, pinned Aurora kernel, m1n1, U-Boot and Mesa; J514S graphics and backlight integration | Record and qualify the exact supported combination; retain an escape route to the previous working boot |
| Mac image | Asahi metadata, an ESP tree, and an expanding ext4 `root.img` | Product configuration, per-install identity handoff, integrity manifest, installed flake and first-boot setup |
| Accounts | Developer image with `sekka` / `sekka` | No shared usable password in the product image; create the owner's account through a restricted setup flow |
| Updates | Configuration for repairing the original installation in `nix/apple-silicon/installed/` | Generate and ship the corresponding configuration and exact lock for every new installation |
| Bootstrap | Local shell launcher for SHA-256-pinned Asahi v0.9.2 | Structured inspection/planning, verified assets, native authorization and Recovery guidance |
| Genkan | Immutable parsed plans, separate confirmation, pre-write disk checks, store integrity checks and a privileged helper | Persistent stage evidence and bounded recovery; share validated product choices with first-boot setup |
| Publication | Existing release and qualification controls | Keep publication inhibited; design Mac artifact qualification and promotion without enabling distribution |

The baseline's aarch64 flake evaluation, backlight component check, fourteen
GPU gate fixture cases, and focused x86 derivation evaluations passed during
the rebase. The expanded whole-distro evaluation was stopped incomplete after
evaluator problems and lengthy PC configuration evaluation. These results are
not a complete system build, new Mac installation, Recovery test, or physical
qualification. The running Mac was not rebuilt as part of the rebase.

The authoritative source map is [DEVELOPERS.md](../DEVELOPERS.md#apple-silicon).
The main implementation points are:

| Existing source | Responsibility in this plan |
| --- | --- |
| [image.nix](../nix/apple-silicon/image.nix) | Image, ESP and Asahi package construction |
| [installed configuration](../nix/apple-silicon/installed/configuration.nix) and [flake](../nix/apple-silicon/installed/flake.nix) | Starting point for an updatable Mac installation; remove assumptions specific to the original developer image |
| [platform module](../nix/apple-silicon/module.nix) and [graphics integration](../nix/apple-silicon/graphics.nix) | Boot, firmware, audio and matched graphics stack |
| [bootstrap](../nix/apple-silicon/install-sekka.sh) | Existing local developer entrypoint; retain as a documented developer path while replacing its product role |
| [installed lock builder](../nix/installed-lock.nix) and [flake template](../templates/flake.nix.in) | Exact source pinning, followed inputs and generated-system compatibility |
| [plan](../installer/genkan/rust/src/plan.rs), [disk](../installer/genkan/rust/src/disk.rs), [install](../installer/genkan/rust/src/install.rs), [helper](../installer/genkan/rust/src/helper.rs) and [session](../installer/genkan/rust/src/session.rs) | Existing Linux validation, preparation, execution and privilege boundary |
| [configuration generator](../installer/genkan/rust/src/config.rs) and [Genkan UI](../installer/genkan/ui/src/app.rs) | Reusable validated choices and user journey; no assumption that the Wayland UI runs on macOS |
| [Mac image VM harness](../nix/apple-silicon/boot-image-in-vm.sh) | Supplemental tests of the real packaged userland, with explicit limits on Apple boot/hardware coverage |

## What to reuse from Omarchy

At the reviewed snapshot, Omarchy has a SwiftUI macOS app, a signed support
catalog, a privileged XPC helper, a pinned Asahi v0.9.2 engine with a Python
overlay, and a separate native Recovery app. The app stages verified assets;
the helper executes a layout-bound plan; Recovery enrolls startup software;
Linux completes first-boot setup. Its source provides useful patterns for
inspection, authorization, journals, error presentation and Recovery retry.
[Product flow](https://github.com/omacom/omarchy-mac-installer/blob/c07e54c2134a8ec1415e7e2b917589ae1d3d7dc9/README.md),
[engine provenance](https://github.com/omacom/omarchy-mac-installer/blob/c07e54c2134a8ec1415e7e2b917589ae1d3d7dc9/Engine/source-lock.json).

Its payload validator requires EFI, `boot.img`, and `root.img`, an Omarchy
target identifier and volume icon, and its own Recovery declarations. Sekka's
payload has an ESP and `root.img`; its kernel and initrd live on the ESP.
These counts exclude the Apple stub volume group, which Asahi manages. Build
a Sekka target adapter with explicit invariants rather than adding an unused
boot partition to satisfy Omarchy's validator.
[Payload contract](https://github.com/omacom/omarchy-mac-installer/blob/c07e54c2134a8ec1415e7e2b917589ae1d3d7dc9/Engine/overlay/src/omarchy_asahi.py#L2099-L2187).

Prefer contributing a narrow target-adapter seam upstream. If that cannot land
in time, pin a reviewed external patch/fork and keep the Sekka-specific adapter
and product configuration local. Track imported files, licenses, original
revisions and local modifications in the repository's provenance records.
Review each component's license; preserve the upstream notices when adapting
code. Do not inherit Omarchy's signing identity, trust key, production URLs,
support claims or release automation.

## Architecture and user journey

| Component | Boundary |
| --- | --- |
| Nix image producer | Builds the Linux closure, initial boot files, source/lock metadata and payload; does not need the Mac owner's credentials |
| macOS app | Unprivileged SwiftUI companion; inspection, allocation preview, downloads, progress, diagnostics and handoff instructions |
| macOS helper | Dedicated authenticated privileged service; verifies the approved release and plan again, owns mutations and private state |
| Asahi engine and Sekka adapter | Pinned external engine plus an explicit Sekka payload/firmware/boot contract; structured inspect, plan and execute operations |
| Recovery app | Separate native application for the selected install's paired Recovery; enrollment, verification and controlled restart |
| Linux setup | Restricted Aizome setup interface with a narrowly scoped helper; account and configuration completion before ordinary login |
| Genkan Linux helper | Retains PC storage ownership and its current guards; gains its own persistent installation evidence in a separate change |

Suggested new first-party locations are `installer/genkan/macos/` for the Swift
package, `installer/genkan/macos/recovery/` for its separately built Recovery
target, and `installer/genkan/first-boot/` for Linux setup. Finalize boundaries
in phase 0. Keep new packages independent of the existing Rust workspaces.
Expose source preparation, image/engine packaging and native build entrypoints
through the root flake. Building the Linux image needs an aarch64-linux builder;
the SwiftUI and Recovery builds need a recorded macOS/Xcode/SDK toolchain.
Record those native inputs rather than claiming that Linux builds certify them.

The intended normal journey is:

1. Open the app. Inspect model, firmware, macOS, power, FileVault and APFS layout
   without changing disks. Reject unsupported or ambiguous states before asking
   for an administrator password.
2. Select a qualified release and allocation. Show retained macOS space, Sekka
   space, minimum/recommended sizes, download staging needs and required human
   Recovery steps. Present a backup acknowledgement and a precise review.
3. Download and verify the complete release. Make offline reuse of already
   verified assets possible; clearly separate preparation from installation.
4. Authorize the signed helper and the selected plan. Recompute disk identity,
   layout, model support and asset binding in the privileged process.
5. Shrink APFS when required, create the stub and Sekka partitions, write and
   verify the payload, extract local Apple firmware, and prepare Recovery.
6. Show the exact shutdown, power-button and volume-selection instructions.
   Report this state as waiting for Recovery, never as a completed Linux boot.
7. In paired Recovery, verify the target and enroll its boot software using the
   owner's authorization. Persist the outcome and verify startup selection.
8. Boot the installed disk into restricted setup. Create the owner's Linux
   account, confirm locale/keyboard/timezone and default desktop, and finalize
   the installed configuration. macOS credentials are never reused as Linux
   credentials automatically.
9. Reach Aizome's real greeter and log in normally. Offer Tatami as installed;
   defer optional downloads until the system has a working account/network.
10. Use Yukimi to change applications and update. Retain a working generation
    and explain how to select macOS, Recovery or an older Sekka generation.

## Invariants to establish before destructive implementation

- A board allowlist and firmware policy gate both app and helper. A familiar
  marketing model name, matching `supported_fw`, or successful VM boot is
  insufficient. Unknown identifiers fail closed with actionable diagnostics.
- A confirmation binds the device, physical store, APFS source group, layout
  digest, target extent, image/engine/metadata identities and required human
  steps. The helper derives current values independently. A changed plan needs
  a new review; `/dev/diskN` and `disk0s2` are never persistent identities.
- Adopt a reviewed per-install identity handoff before freezing the schema.
  The first boot must identify its own root and ESP even when another Sekka
  install or duplicate image labels are visible. Prototype the GPT UUID,
  filesystem identity and boot-entry/initrd handshake; do not copy the original
  developer image's fixed identities into a general-purpose installed config.
- Keep passwords, Wi-Fi secrets and usable password hashes out of catalogs,
  Nix store content, public manifests, journals and diagnostic exports. Use
  private IPC and the existing root-only credential-file pattern. Root-owned
  state alone does not make arbitrary engine commands acceptable.
- Authenticate both app and helper; independently validate client identity and
  allow only narrow operations. Private staging must reject symlink/hardlink,
  ownership, path traversal and archive-member substitution attacks. A release
  executable must not enable simulation or arbitrary engine-path overrides.
- Retain firmware extraction on the owner's Mac. Per-install firmware and
  target data stay outside signed application bundles and are bound by their
  own inventory and hashes. Do not add non-redistributable firmware to caches.
- Verify downloaded artifacts before disk changes and define installed-byte
  verification separately. Preserve Genkan's live-store content checks,
  copy-time integrity checks, device locking and filesystem/UUID revalidation.
  Neither a valid catalog signature nor a successful write proves read-back.
- Persist mutation intent before a write and completion evidence after verified
  success. An interrupted or ambiguous destructive operation is not replayed
  automatically. A journal records evidence; it does not override real disks.
- All storage fixtures use disposable disk files/VMs. No fixture accepts a host
  block device. Real installation is a distinct owner-operated hardware step
  with its own backup, restore plan and explicit authorization.
- Keep release publication inhibited. Adding build outputs, test artifacts or
  a proposed catalog format does not enable a public channel or scheduled upload.

## Implementation phases

### 0. Freeze contracts, provenance and the hardware profile

Deliver a reviewed design change before privileged execution work:

- Record the exact J514S model, board, system firmware, selected OS firmware,
  kernel, configuration, m1n1, U-Boot, Mesa and Sekka revision. Separate facts
  observed on this computer from historical reports and untested capabilities.
- Audit the reviewed Omarchy app, engine, helper and Recovery source against
  current upstream. Produce a reuse/patch inventory and license/provenance map.
- Define versioned release, inspection, plan, installation-record, Recovery and
  first-boot handoff schemas. Specify canonical serialization, digest coverage,
  size limits, version rejection and migrations. Separate public release data
  from private per-install state and secrets.
- Resolve root/ESP identity before first boot, paired-Recovery target binding,
  the source of trusted Apple restore inputs, and minimum supported macOS/SDK.
  Choose explicit helper registration and upgrade semantics after reviewing the
  upstream implementation and the selected macOS baseline.
- Establish read-only inventory fixtures and a simulation that cannot call the
  live helper, disk tools, firmware enrollment or credential stores. Define
  negative fixtures for unsupported boards and ambiguous APFS arrangements.

Exit: approved schemas, source inventory, one explicit hardware profile, and a
simulation/fixture boundary. Any unresolved first-boot identity or target-binding
question blocks execution work. No hardware support is newly certified here.

### 1. Produce an updatable Mac image

Refactor `nix/apple-silicon/image.nix` around a common installed-system module,
using the repaired configuration as a starting point. Keep Aizome as the default
desktop/greeter and Tatami available. Preserve `stateVersion`, platform options,
speaker safety, vendor firmware handling and the matched graphics environment.

Ship `/etc/nixos/flake.nix`, its exact `flake.lock`, generated configuration and
the metadata required to reconstruct the installation. Parameterize architecture
and update tracking without changing the six followed inputs checked by Genkan's
real template parser. Keep implementation-only dependencies nested under Sekka.
Derive the installed lock from the image's own clean source revision. Seed and
retain sources needed for the advertised offline first-boot/configuration path.

The Apple branch remains the development update source until its platform has
been reviewed and merged. A public update source must be backed by aarch64
checks, platform qualification and cache coverage; it cannot silently switch to
PC-only nightly outputs. Define how a tested platform stack is held while
desktop/security updates move, and requalify any platform-pin change.

Design boot-chain updates separately from ordinary NixOS generation changes.
The ESP's `m1n1/boot.bin` is shared boot infrastructure; selecting an older
kernel generation does not by itself restore that file or Apple's boot policy.
Define verified backup, replacement, synchronization and recovery rules for
ESP boot artifacts, and test compatibility with both retained generations.

Implement the phase-0 per-install identity handoff, including the configuration
used on the first rebuild. Preserve the ESP plus ext4 root layout. Generate a
manifest of source/lock identity, image sizes/digests, boot-file inventory and
minimum allocation. Add image integrity checks before packaging and verify the
store registration and required closure against actual image bytes.

Exit: the exact packaged image boots twice in a disposable VM without an
installer share; its store is registered once; root growth and firmware handoff
work with fixture firmware; `/etc/nixos` evaluates with its shipped lock; a
no-change rebuild and a controlled update/rollback work. Test two similar image
identities to expose accidental label-based selection. Record Apple boot and
driver behavior as untested by this VM.

### 2. Replace the developer account with first-boot setup

Add a dedicated setup mode on Aizome's greeter surface, using a locked,
unprivileged setup identity and a helper limited to account/configuration
completion. The product image has no shared usable account password and no
ordinary desktop autologin. This mode must work offline with the default image.

Reuse Genkan's validated account, hostname, locale, keyboard, timezone and
desktop/application choices through small platform-neutral interfaces. Do not
make its Linux storage executor build on macOS to obtain those choices. Check
application availability against the image's actual aarch64 package inputs;
unsupported choices are explained, and optional network work is deferred.

Treat completion as a recoverable transaction: validate choices, stage the
configuration and private credential material, create/verify the account and
system generation, then mark setup complete. Define reconciliation if a crash
occurs between account creation, configuration activation and the completion
marker. Retrying must preserve the existing user's identity and password.
Existing developer installations must not be reset into setup or have their
passwords replaced by an update.

Exit: headless tests cover validation and interruption boundaries; a GUI-driven
VM run creates the account, rejects a wrong login password, logs in correctly,
locks/unlocks, logs out, selects Tatami, and preserves settings after reboot.
Tests prove no credentials appear in store paths, journals or exports, and no
usable default password remains in the product image.

### 3. Add a signed catalog and verified asset preparation

Define Sekka-owned trust roots, product identifiers and allowed delivery
locations. Bind a release to its source/lock, exact payload/metadata/engine
sizes and hashes, board and firmware policy, minimum compatible app/protocol,
and Recovery contract. Reject unknown schema versions, bad signatures, expired
catalogs and prohibited rollback; design key rotation and clock-error behavior.
A locally retained highest accepted catalog identity must not be silently reset
to bypass rollback protection.

Adapt the catalog validation and private artifact stager. Support cancellation,
bounded network retries, correct range validation, part-level restart and final
whole-file verification. Calculate peak staging space, including multipart
assembly, and distinguish it from APFS shrink limits. Rehash cached content and
revalidate it after the privileged handoff. Bind a locked channel/release choice
to the plan; downloads cannot change that choice mid-installation.

Model Apple restore/firmware downloads as explicit required inputs. Use the
engine's reviewed authenticity rules and provenance instead of inventing trust
from a checksum downloaded beside an unauthenticated file. Disclose inputs that
cannot be fully staged in advance, and block mutation until required inputs are
ready under the agreed contract.

Exit: local HTTPS/transport fixtures cover corrupt, truncated, stale, wrong-board,
wrong-version and substituted assets; disk-full, cancel/resume and network loss;
unsafe filesystem/archive entries; and helper-side revalidation. The normal
verified-download path performs no disk mutation. Catalog signing is tested
with test keys only; no public catalog or production signing is enabled.

### 4. Implement the Sekka engine adapter and privileged execution

Provide structured inspection and planning over the pinned Asahi integration.
Replace the bootstrap's hardcoded free-space display with actual APFS inventory.
Plan only a recognized internal physical store and supported source group/free
extent. Explain FileVault, snapshots, minimum shrink bounds, required power and
existing/partial installations. Recognize unsupported layouts without guessing
how to repair or delete them.

Keep resize failures, including the previously reported `-69606`, visible with
the underlying Apple diagnostic. Reinspect the real layout before describing
what changed. Do not automatically delete snapshots, disable FileVault or erase
unknown volumes to make a resize succeed.

Implement an explicit Sekka payload contract for the ESP, ext4 root, firmware,
systemd-boot and first-boot metadata. Enforce the reviewed board/firmware profile
in the engine as well as the UI. Preserve Asahi's stub and ownership mechanisms;
do not reimplement APFS resizing or Apple's boot-policy tools in the GUI.

Add a dedicated macOS helper with authenticated XPC, independent release/plan
verification, private imports, a single active installation lock and explicit
timeouts. Registration, replacement and retirement must preserve or explicitly
migrate unfinished installation records. Limit the privileged environment and
commands; no client-supplied shell fragments or unrestricted executable paths.

Implement the journal rules below before enabling execution. After each stage,
read actual target identities, geometry and content evidence. Distinguish source
hashing, flushed writes, full read-back and boot verification in records. Choose
and measure the read-back policy explicitly; do not describe source hashing
during a write as installed-byte verification. At completion, verify the result
against its installation record and retain private diagnostics.

Exit: injected engine tests cover changed layout, stale confirmation, concurrent
requests, credential cancellation, resize/write/flush/read-back failures and
every journal boundary. Run real storage operations only on marked disposable
VM disks. macOS native helper/signature tests are separate from Linux fixtures;
APFS/boot-policy behavior that cannot be represented safely remains a physical
qualification requirement. The new product execution path stays disabled until
its required gates pass.

### 5. Build the native macOS companion

Adapt the SwiftUI flow around Sekka's identity and release configuration. Keep
slow inspection, hashing, downloads and engine activity off the UI thread. Add
model-aware allocation review, explicit backup confirmation, administrator
authorization, phase progress, bounded selectable logs, cancel semantics and a
clear next action after failure. Include keyboard access, accessibility labels,
scaling, small-window layouts and both appearance modes used by the app.

Explain when cancellation is safe. Before mutation, cancelling leaves disks
unchanged. Once an operation cannot be interrupted safely, keep the app
responsive and explain the pending boundary rather than killing disk tools.
Closing/reopening the app reconnects to recorded state or shows a reconciliation
path; it never starts a second installation implicitly.

Provide debug-only simulation and screenshots of real app previews with honest
provenance. Release builds reject simulation, arbitrary catalogs and untrusted
engine overrides. Retain the developer bootstrap until the new path has been
qualified; document which path each test exercised.

Exit: macOS debug/release tests, UI/state tests and visual review cover normal,
unsupported, offline, low-space, authorization-denied, download-failed,
installation-failed and Recovery-waiting states. Simulations prove the normal
journey and failure presentation without touching disks, helpers or accounts.

### 6. Add native Recovery and bounded authorization retry

Build a separate native Recovery target using its required deployment SDK and
runtime. Give it Sekka-owned identifiers and code requirements. Keep the signed
application immutable; place per-install configuration, firmware baseline and
hash inventory outside the bundle. Bind that data to the selected stub group,
ESP and boot payload, and verify it in paired Recovery before enrollment.

Port only the reviewed enrollment/selection flow. Ask for the appropriate owner
credential through a native prompt, never save it, verify selected startup
identity after authorization, and distinguish enrollment from a successful Linux
boot. Clear credential state promptly without promising complete allocator or
framework memory erasure.

An authorization retry is allowed only after rechecking completed payload
checkpoints and target identities; it cannot resize, repartition or rewrite the
Linux system. If enrollment may have happened before a timeout, reconcile the
actual policy and retain the matching boot payload. File rollback is not proof
that an Apple LocalPolicy change has been undone.

Treat independent Recovery accounts as a later, explicitly scoped feature.
Initial setup uses the existing owner's authorized path. A future independent
account must prove native ownership and a real paired-Recovery login before it
can be offered as a replacement for that path.

Exit: native debug/release builds and fixture tests validate configuration,
target binding, signature/inventory rejection, credential lifetime, interruption
and retry restrictions. Actual Recovery launch, owner authorization, startup
selection and restart remain mandatory physical checks on the exact candidate.

### 7. Bring durable evidence and safe retry to Genkan on PCs

Apply the same design principles in a separate backend change, preserving
Genkan's immutable `InstallPlan`, typed confirmation and helper session protocol.
Do not serialize a validated Rust type and then treat deserialization as proof
of validation. Introduce a versioned, secret-free record with explicit parsing
and revalidation at the privileged boundary.

Record preparation, the confirmed configuration identity, storage intent and
verified result, closure transfer, activation/bootloader, flush/unmount and final
verification. Preserve current pre-erase store checks, low-memory/deferred-build
behavior and filesystem/UUID checks. Define where records survive loss of the
live session and when it is safe to copy them to the target; a `/run` log alone
does not provide reboot persistence. Export only a redacted support record.

Initially offer diagnostics and retry for stages proven safe to repeat, such
as a validated completed copy or bootloader completion on the same target.
Never resume an uncertain format/partition stage automatically. Extending retry
coverage requires a stage-specific proof, failure tests and GUI behavior.

Exit: existing backend/UI tests and disposable UEFI/BIOS storage tests pass;
negative cases prove changed targets and incomplete/forged records are refused.
At least one normal UI install per required firmware, disk-only boot, Aizome
login and Yukimi update/rollback must be repeated on the exact resulting ISO.
Follow the full repository qualification scope for any published PC image.

### 8. Design and qualify Mac encryption

Do not transplant Omarchy's first-boot in-place encryption into a prebuilt
NixOS image without a separate design. Compare a disposable installer environment
that provisions LUKS before copying the final system with resumable in-place
conversion. Evaluate temporary space, interruption, power failure, secret
lifetime, initrd unlock, recovery and update/rollback for both routes.

Prefer provisioning an encrypted target before installing its final contents
if the prototype can do so within the supported Mac boot path. Adopt in-place
conversion only with a reviewed recovery protocol and convincing failure
evidence. Encryption must finish before ordinary user data is accepted.

Keep the firmware and boot artifacts needed before unlock available to the boot
chain, and explain the resulting security boundary. Validate password entry in
the actual Mac initrd, wrong-password handling, retries, keyboard layout,
encrypted swap policy and old-generation boot. Never imply that Linux disk
encryption changes Apple's boot-policy or macOS FileVault guarantees.

Exit: an approved design, disposable interruption matrix, exact-image encrypted
VM tests where representative, and real J514S unlock/reboot/update/rollback
evidence. If encryption is deferred, record that product decision and disclose
the limitation; do not offer a nonfunctional toggle or describe the initial
unencrypted release as completing this phase.

### 9. Qualify the exact product and its updates

Before a hardware run, add a Mac qualification runbook that extends the
[existing coordinator/evidence contract](../.agents/skills/sekka-iso-qualification/SKILL.md).
The ISO skill's PC branches remain mandatory for PC installer regressions and
publication. A Mac ZIP is not an ISO qualification result. Reconcile the ledger
below against every advertised app, desktop, setting, device and recovery path.

Use the actual DMG/app, catalog, engine, metadata, Recovery app and Linux payload
that the owner would receive. Freeze source and lock, hash every artifact and
retain failed candidates. Use a dedicated spare test Mac or a separately
approved owner-operated install with a verified backup and restore route.
Fixtures never mutate this workstation's disks.

Exit: all required ledger entries pass on the same candidate set, an independent
agent audits the evidence before publication, and unsupported capabilities are
explicitly scoped. A blocked installation, Recovery, boot, login or update case
prevents a support claim. A code/payload change starts a new candidate and the
affected full qualification run; evidence is not inherited from a patched guest.

### 10. Package and prepare manual distribution

Add reproducible source/engine/image preparation, native app/Recovery build
records, SBOM/provenance and immutable artifact manifests. Audit every embedded
native executable and runtime before signing/notarization, including binaries
inside archives. Debug/ad-hoc artifacts cannot satisfy release requirements.
Use Sekka-owned app/helper/Recovery code requirements and test upgrades from
earlier helpers without losing unfinished records.

Design a Mac-specific qualification report and uploader; the existing ISO
publication command does not automatically accept a DMG or ZIP. Keep artifacts
unindexed until validation is complete. Production signing/notarization uses the
reviewed Sekka identity and release process; public publication requires an
explicit user request. Preserve the
workstation qualification/publication boundary for installable images and keep
CI free of public image-upload credentials and triggers; do not copy Omarchy's
automatic edge-publication policy.

When authorized, promote the exact qualified bytes without rebuilding them,
using immutable paths and a reviewed signed catalog update. Refuse concurrent
or stale promotion. Independently download and verify public bytes, manifests
and the rendered download page. Test installer update/minimum-version behavior,
channel selection and withdrawal of a bad catalog without losing recovery data.

Exit: an independently audited candidate and a tested dry-run publication path.
Actual publication is a later authorized action, not an effect of merging this
plan or its implementation PRs.

### 11. Add removal, replacement and more boards separately

First recognize an existing or partial Sekka install and stop with an accurate
explanation. Later add a narrowly recognized removal/replacement plan bound to
the exact stub, ESP, root and APFS source identities. Require a fresh destructive
confirmation, durable before-write records, interrupted-removal reconciliation,
and separate validation of partition deletion and macOS container growth. A
failed growth must report unallocated space without deleting the same partitions
again. Uninstalling the macOS app does not remove Linux data.

Add new boards only through an explicit platform profile, boot/firmware policy,
matched driver stack and the complete hardware qualification matrix. Reusing the
app or image producer does not transfer J514S evidence to another Mac.

## Journal and recovery contract

Persist a versioned record in a private root-owned directory. Bind the release,
plan digest, installation ID, platform profile, physical/APFS/partition UUIDs,
selected extent and resulting content evidence. Keep a bounded diagnostic log
separate from the authoritative state. Validate lengths, sequence numbers,
schema and inventory; reject torn, conflicting or externally substituted state.

Use durable intent records, atomic checkpoint publication, and file plus parent
directory synchronization. Keep previous records until reconciliation completes.
Define and test which state is authoritative after power loss. Multiple journal
copies must agree under a documented reconciliation rule; choosing whichever
copy says the most progress is unsafe.

On the Mac the helper's journal is the only authoritative copy. Recovery keeps
its own receipt on the stub, which nothing trusts as evidence: anyone who can
write the stub can write it. Any other copy of the records may lag the
helper's, record for record, but never differ from it or go further, since the
chain's digests have no key (`sekka_mac_journal::reconcile`). The helper learns
what Recovery did, or that an installation was removed, only from its own
inspection as root with the installation's own engine (`Request::Reconcile`).
It inspects and records under the install lock, which every engine started
for an installation also holds until it exits, even past the helper that
started it, so nothing is recorded while an engine may still be changing the
disk. It records nothing when the inspection can't see the plan's disk or is
ambiguous.

| Observed state | Permitted next action |
| --- | --- |
| Inspected/planned; no mutation intent | Reinspect and prepare; a changed layout requires a newly reviewed plan |
| Verified assets; no mutation intent | Revalidate/reuse assets or cancel without disk changes; a stage that changes nothing and was cut off by a restart is recorded as interrupted, then cancelled |
| Mutation started; completion not proved | Inspect actual state; stop for reconciliation if its effect is ambiguous. Setting the startup disk is the exception: once the helper holds the install lock, so no engine is still in that stage, the helper records the interruption, then reverifies target and content and retries authorization only, as after it failed |
| Payload checkpoint verified; startup authorization failed | Reverify target/content and retry authorization only |
| macOS's part complete; waiting for Recovery | Finish in Recovery. Once the helper's inspection shows the installation complete (the stub's boot policy names its boot object) and made of the stub, ESP and root its records name, at the offsets and sizes its stages recorded, record both Recovery steps as policy read back, with the inspection as their source |
| Recovery enrollment uncertain | Read actual policy/selection and preserve matching boot software; never blindly restore an older payload |
| Waiting for Linux setup | Boot the installed target and resume only that installation's setup transaction |
| Account/configuration partly committed | Reconcile the existing account and generation; never reset a user's password as a retry |
| Installation complete | Verify booted target and configured system; ordinary updates use Yukimi/NixOS generations |
| Record missing/corrupt or target changed | Refuse destructive resume; provide redacted diagnostics and a manual recovery path |
| Stopped, and nothing of it on its plan's disk as the helper inspects it | Record it removed (what was checked: no partition in the plan's extent other than a macOS container from before it, the one it shrank or the one holding its macOS, grown back over the space; no installation naming it; nothing unidentified there); then reinspect and plan afresh |

Checkpoint names and exact write order are phase-0/4 schema deliverables. Test
process termination, short writes, `ENOSPC`, failed sync and power-loss models
at every boundary. Test models establish ordering properties; real hardware
interruption tests remain a separately reviewed activity.

## Validation and evidence ledger

Store run directories outside tracked source. Record candidate SHA-256 and size,
source revision, lock hash, toolchain, workstation/VM identity, firmware,
commands, timestamps, preconditions and evidence locations. Each row has an ID,
input/action, expected and observed result, status and issue/PR when failed.
Use `not-run`, `running`, `pass`, `fail`, `blocked`, or `not-applicable` with a
concrete reason. An untested feature is never a pass.

| Gate | Required coverage | Evidence and limit |
| --- | --- | --- |
| M0 contracts and trust | Schema/version rejection, bad signature/expiry/rollback, wrong board/firmware, swapped assets, changed layout, malformed records, concurrent requests | Portable tests and recorded fixtures; no claim about macOS APIs or physical disk safety |
| M1 image | Store contents/registration, source/lock, closure completeness, boot inventory, root growth, unique target identity, first-boot idempotence | Build-time inspection plus two boots of the exact packaged image in disposable QEMU; fixture firmware is labeled |
| M2 Linux experience | Normal setup, account/login failures, Aizome greeter, lock/unlock, Tatami/session rotation, every advertised app/setting, locale/network persistence | Normal UI input and inspected screenshots/logs, rather than commands that bypass the greeter |
| M3 native macOS | Swift debug/release tests, helper/client identity, registration/upgrade tests, preview/simulation, accessibility, transport and staging failures | Recorded native toolchain and real app captures; Linux tests do not cover this gate |
| M4 Asahi/Recovery | Supported/unsupported inventory, APFS limits and failures, payload and firmware writes, target binding, paired Recovery, enrollment, retry and startup selection | Disposable fixtures where representative; owner-operated exact-candidate J514S evidence for real APFS/Apple policy |
| M5 hardware | m1n1 → U-Boot → systemd-boot, native panel resolution/refresh, GPU and fallback, brightness, keyboard/trackpad, Wi-Fi/Bluetooth, speakersafetyd/audio, camera/video, USB-C/external displays, lid/suspend/resume, cold boot/restart | Exact model/firmware and per-feature success/error/limitation inventory; a missing device or unsupported port is recorded honestly |
| M6 updates/recovery | Offline defaults, no-change rebuild, Yukimi UI and CLI update, network failure/retry, aarch64 cache coverage, new-generation boot, previous-generation rollback, macOS boot and return to paired Recovery | Before/after revisions, source/lock, boot selections, installed files and persisted settings; no production branch manipulation for test scenarios |
| M7 encryption | Provisioning/conversion interruptions, wrong/correct passphrase, real initrd input, cold boot, update/rollback and recovery | Required before encryption is offered; an explicitly deferred phase is not a passing encrypted installation |
| M8 PC regression | Existing backend/UI/component checks; storage/encryption matrix, actual ISO UI installation, disk-only boot, desktop/session and update inventories | Follow Q0–Q8 and relevant references in the ISO skill; component checks cannot certify the changed ISO |
| M9 promotion | Exact artifact bindings, independent evidence audit, native signatures/notarization where required, dry-run uploader, remote-byte verification after authorized publication | A report for the complete candidate set; no automatic support claim from a build or signed catalog |

Start with known controls: accepted/rejected host fixtures, a clean spare test
machine, a known-working macOS boot and Recovery route, and disposable user
credentials. Include FileVault enabled/disabled where claimed, existing free
space versus resize, low space, interrupted downloads, and partial installation
states. A native API case unavailable in a VM remains a hardware gate.

Existing commands are starting points for component work, not a certification
script. Read current package/test interfaces before running them:

```sh
nix flake check --no-build --no-update-lock-file
nix eval --raw --no-update-lock-file .#packages.aarch64-linux.installed-j514s.drvPath
nix build .#packages.aarch64-linux.m3-gpu-session-env-test --no-update-lock-file
nix build .#checks.x86_64-linux.t6030-backlight --no-update-lock-file
nix build .#packages.aarch64-linux.mac-installer-j514s --no-update-lock-file --out-link result-mac
```

The packaged GPU gate test can build its matched Mesa dependencies; account for
that cost before scheduling it. The raw gate source contains Nix substitutions
and is not directly executable as a finished gate. The x86 check needs an
x86_64-linux builder. A no-build flake evaluation does not execute checks and
normally omits incompatible systems; name the systems actually evaluated in
the report. Run the VM image harness only
on its documented macOS host with disposable disk files. Never run the install
bootstrap as a validation shortcut on the developer workstation.

For changed Rust components, use their existing locked Cargo tests inside the
root development shell. For native targets, add focused Swift/engine tests,
formatting and real SDK builds. For PC installer regressions, load the ISO
skill's relevant phase references when that phase begins. During broad runs,
delegate independent cases as the skill requires, with separate VMs/ledgers;
one coordinator reconciles the results and an independent agent audits them.

## Review sequence and completion criteria

Implement focused topic branches/PRs from current main, coordinating with this
Apple integration branch until its platform is merged. Rebase integration work
as required, preserve working changes, and squash-merge reviewed changes. This
document commit itself belongs on the requested Apple branch.

| Review unit | Depends on | Reviewable result |
| --- | --- | --- |
| A. Contracts and provenance | None | Phase 0 schemas, profile, source inventory, identity prototype and safe fixtures |
| B. Installed image/configuration | A | Phase 1 image and update path with actual packaged-image VM evidence |
| C. First-boot setup | B | Phase 2 restricted setup and normal greeter/login evidence |
| D. Catalog and preparation | A, B | Phase 3 trust/staging implementation with test keys and no live mutations |
| E. Engine/helper/journal | A, B, D | Phase 4 Sekka adapter, authenticated helper and complete interruption tests |
| F. Native companion | D, E; simulation may start after A | Phase 5 app journey and native debug/release evidence |
| G. Recovery | E, F | Phase 6 separate Recovery app and constrained retry; hardware gate remains explicit |
| H. Genkan journal | A; independent of the Mac UI | Phase 7 PC evidence and qualification for any resulting public ISO |
| I. Encryption | B, C, E, G | Phase 8 design and implementation, or an explicit deferred product decision |
| J. Qualification and packaging | B–G, relevant H, and resolved I scope | Phases 9–10 exact candidate report, independent audit and dry-run distribution |
| K. Removal and new boards | Qualified initial install and updated contracts | Separate phase 11 plans, negative tests and hardware evidence |

The first implementation slice is A → B → C: make the existing Mac image
updatable and safe to personalize, then prove the installed experience. D → E →
F → G builds the guided macOS/Recovery path around that image. Keep engine
adaptation and native packaging reviewable separately from account setup and
PC storage changes.

Completion means the initial target can traverse the normal app, Recovery,
setup, login, update and rollback path using the exact qualified artifacts, with
all supported features accounted for and no usable default password. Every
failure path identifies what changed and the next safe action. Encryption,
removal and additional boards have their own completion gates and may not be
implied by completion of the unencrypted J514S path. Public distribution remains
a separate explicitly authorized operation.

## Reference material

These are implementation references at the reviewed Omarchy commit. They are
evidence of mechanisms and constraints, not qualification of Sekka's stack.

- [Engine producer and provenance](https://github.com/omacom/omarchy-mac-installer/blob/c07e54c2134a8ec1415e7e2b917589ae1d3d7dc9/Engine/README.md).
- [Catalog signature and model validation](https://github.com/omacom/omarchy-mac-installer/blob/c07e54c2134a8ec1415e7e2b917589ae1d3d7dc9/Sources/OmarchyAppleInstaller/SignedSupportCatalog.swift).
- [Artifact staging and resumable downloads](https://github.com/omacom/omarchy-mac-installer/blob/c07e54c2134a8ec1415e7e2b917589ae1d3d7dc9/Sources/OmarchyAppleInstaller/VerifiedArtifactStager.swift).
- [Review/confirmation binding](https://github.com/omacom/omarchy-mac-installer/blob/c07e54c2134a8ec1415e7e2b917589ae1d3d7dc9/Sources/OmarchyAppleInstaller/InstallerPlanReview.swift) and [execution admission](https://github.com/omacom/omarchy-mac-installer/blob/c07e54c2134a8ec1415e7e2b917589ae1d3d7dc9/Engine/overlay/src/omarchy_execution.py).
- [Authenticated helper](https://github.com/omacom/omarchy-mac-installer/blob/c07e54c2134a8ec1415e7e2b917589ae1d3d7dc9/Sources/OmarchyAppleInstaller/ClosedEngineHelperServer.swift) and [pinned engine executor](https://github.com/omacom/omarchy-mac-installer/blob/c07e54c2134a8ec1415e7e2b917589ae1d3d7dc9/Sources/OmarchyAppleInstaller/PinnedAsahiEngineExecutor.swift).
- [Stage journal and replay rules](https://github.com/omacom/omarchy-mac-installer/blob/c07e54c2134a8ec1415e7e2b917589ae1d3d7dc9/Engine/overlay/src/omarchy_stage1.py) and [Recovery retry eligibility](https://github.com/omacom/omarchy-mac-installer/blob/c07e54c2134a8ec1415e7e2b917589ae1d3d7dc9/Sources/OmarchyAppleInstaller/RecoveryAuthorizationRetryCheckpoint.swift).
- [Native Recovery design and validation limits](https://github.com/omacom/omarchy-mac-installer/blob/c07e54c2134a8ec1415e7e2b917589ae1d3d7dc9/Recovery/README.md).
- [Upstream release pipeline and hardware qualification scope](https://github.com/omacom/omarchy-mac-installer/blob/c07e54c2134a8ec1415e7e2b917589ae1d3d7dc9/docs/mac-release-pipeline.md).
- [Source license](https://github.com/omacom/omarchy-mac-installer/blob/c07e54c2134a8ec1415e7e2b917589ae1d3d7dc9/LICENSE).
- [Sekka installer integrity incident](incidents/2026-10-24-installer-hash-mismatch-after-erase.md), [storage reliability](storage-reliability.md), [screenshot provenance](screenshots.md), and [ISO qualification procedure](../.agents/skills/sekka-iso-qualification/SKILL.md).

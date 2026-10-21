---
name: sekka-iso-qualification
description: Qualify a Sekka ISO on a local workstation through real VM installation, disk-only boots, Aizome greeter and desktop, Tatami, and installed-system updates; diagnose failures and gate workstation publication on exact-image evidence.
---

# Sekka ISO qualification

A build is a candidate, not an installable release. Test the exact bytes users
will download. Keep CI building and populating the cache, without publishing
ISOs. Publication runs on the user's workstation after this procedure.

Read `AGENTS.md` and `DEVELOPERS.md` at the candidate revision. Historical reports,
component VMs, fixture installs, and another ISO's success cannot certify this
image. Keep the full scope below even when a session spans several agent turns.

## Coordinator and evidence

Create a durable run directory outside tracked source. Record ISO SHA-256 and
size, Sekka commit, lock SHA-256, locked Aizome/Sidestep commits, tool versions,
workstation, VM command, firmware, virtual hardware, and start time. Use unique
VM/socket/disk names. Only disposable disk files may be mutated; never pass host
block devices, personal folders, or credentials into a guest.

Maintain a ledger with one row per check: ID, candidate hash, VM, precondition,
input/action, expected result, observed result, evidence paths, status, and
issue/PR when failed. Status is `not-run`, `running`, `pass`, `fail`, `blocked`,
or `not-applicable` with a concrete reason. Blank, assumed, and blocked checks
are not passes. Screenshots must be inspected, and logs must support the claim.
A UI action requires mouse/keyboard evidence, not merely a successful command.

For broad runs, delegate independent firmware/install cases and desktop
inventories to agents, each with its own VM and ledger fragment. Give each agent
the candidate identity, assigned IDs, relevant reference subtree, and shared
constraints; do not load every leaf into every agent. One coordinator owns the
publication decision and reconciles all IDs and failures. Never allow two agents
to drive the same VM or mutate the same disk. An independent agent audits the
final evidence against this complete checklist before publication.

## Required tree: all branches must be accounted for

Read each leaf when reaching that phase; the coordinator tracks the whole tree.

| Gate | Read | Required result |
| --- | --- | --- |
| Q0 provenance and capability | [Prepare](references/prepare.md) | Immutable candidate, sufficient resources, complete feature inventory |
| Q1 live media | [Boot](references/install/boot.md) | Actual ISO boots from USB and optical media; all advertised boot entries |
| Q2 installation | [Install](references/install/install.md) | GUI and storage matrix; correct generated system; no host disks |
| Q3 installed boot and greeter | [Login](references/install/login.md) | Media absent; normal boot; Aizome greeter; authentication and session rotation |
| Q4 Aizome | [Aizome](references/desktop/aizome.md) | Compositor, shell, every installed app and settings page exercised |
| Q5 Tatami | [Tatami](references/desktop/tatami.md) | Menus, shortcuts, panels, configuration, capture and lifecycle exercised |
| Q6 applications and updates | [System](references/system.md) | Selected apps, Yukimi, update, rollback, persistence, recovery |
| Q7 fix and repeat | [Failures](references/failures.md) | PRs for confirmed defects; new ISO and full rerun after fixes |
| Q8 public distribution | [Release](references/release/publish.md) | Workstation-only upload, retained evidence, healthy download page and bucket |

There is no blanket "desktop works" checkbox. Expand Q4/Q5 using the actual
menus, settings pages, shortcuts and entrypoints at the pinned revision. Every
advertised feature needs success, cancellation and relevant error/recovery
paths. Record unsupported physical hardware separately: simulate what QEMU can,
and never call a missing radio, GPU, lid or camera a successful hardware test.
An advertised installer/boot/desktop function blocked by the VM needs a suitable
VM/device or physical check before certification, or an explicit public scope
limitation reviewed with the user. Do not silently shrink the release scope.

## Input and observation

Use QEMU QMP input and screenshots, or the available computer-use surface for
the VM. Read [VM controls](references/vm-controls.md) before driving it. Inspect
the current screenshot before choosing coordinates. Use guest-agent/SSH commands
for diagnostics and assertions, not as a substitute for the normal installer,
login, menus, applications or settings flow. Never enable autologin, replace the
greeter, or modify the guest's packages to make a qualification case pass.

A patched guest is a diagnostic experiment. A changed ISO starts a new run with
all required gates reset. Preserve failed images' hashes, logs and PR links.

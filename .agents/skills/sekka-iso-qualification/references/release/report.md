# Qualification report v1

The publication gate reads `QUALIFICATION_DIRECTORY/report.json`. The validator
in `scripts/publish-nightly.py` is authoritative. Keep the full working ledger
while testing; export the final report only after every required check passes.
The validator deliberately rejects `fail`, `blocked`, `not-run`, `running` and
`not-applicable` checks. It cannot determine whether a screenshot proves a
claim, whether an inventory omits a feature, or whether identities represent
independent reviewers. Those are mandatory coordinator and independent-auditor
responsibilities. This report is an auditable record, not a cryptographic
attestation or a substitute for the other reference branches.

## Identity and run

These are required top-level fields (the example is deliberately incomplete and
cannot pass validation):

```json
{
  "schema_version": 1,
  "candidate": {
    "iso_sha256": "64 lowercase hex characters",
    "iso_bytes": 123,
    "sekka_revision": "full Git commit",
    "flake_lock_sha256": "64 lowercase hex characters",
    "sidestep_revision": "full locked Git commit",
    "iso_store_path": "/nix/store/...-sekka-....iso"
  },
  "workstation": {
    "hostname": "the publishing workstation hostname",
    "kind": "workstation",
    "tools": "QEMU, firmware, Nix and control-tool versions; or evidence reference"
  },
  "coordinator": "coordinator name or agent task ID",
  "started_at": "2026-10-21T12:00:00Z",
  "completed_at": "2026-10-21T15:00:00Z",
  "limitations": [],
  "evidence": {},
  "checks": [],
  "inventories": {},
  "audit": {}
}
```

Run from the repository root. The tool requires a clean tracked checkout whose
HEAD is reachable from the fetched `origin/main`. Nix evaluates the ISO output
at that exact Git revision; the chosen ISO must resolve inside that output's
`iso/` directory. An ordinary `result/iso` symlink works. A copied arbitrary ISO
does not establish source provenance. Keep the tested Nix output alive with a
GC root; do not rebuild after testing. The source lock, Sidestep pin,
image size/hash and output path must all match `candidate` exactly.

The hostname must match the workstation performing publication. CI environment
markers are rejected even when set to `false`; remove them only on a real
workstation, never to bypass the CI restriction. This is an accidental-misuse
guard, not machine authentication. Credentials remain outside the report and
guest. `limitations` is an explicit list of public prose strings; it cannot
waive any required check. Apply the skill's hardware/scope review requirements.

## Evidence and checks

`evidence` maps relative file paths to lowercase SHA-256 values, for example
`{"uefi/greeter.png": "...", "uefi/input-and-journal.txt": "..."}`. Every
listed file must be nonempty, regular, inside the qualification directory, and
referenced by a check, inventory or audit. Paths contain only ASCII letters,
digits, `_`, `-`, `.`, and `/`; no absolute paths, traversal or symlinks.
`report.json` is not in this map because it cannot hash itself. Only the report
and listed files are uploaded. Inspect and redact personal data before hashing;
retain original private diagnostics separately if needed. Hash changes require
review again. The uploader snapshots and revalidates evidence before transfer.

Each check has this shape:

```json
{
  "id": "Q3.greeter",
  "gate": "Q3",
  "candidate_sha256": "same ISO SHA-256 as candidate",
  "vm": "unique VM ID; attach launch/firmware/disk provenance in evidence",
  "precondition": "installed disk, ISO and installation shares absent",
  "action": "actual input sequence, including wrong and correct password",
  "expected": "Aizome greeter rejects wrong password and starts selected session",
  "observed": "specific result and relevant process/unit/mount assertions",
  "status": "pass",
  "evidence": ["uefi/greeter.png", "uefi/input-and-journal.txt"]
}
```

Every check ID is unique and starts with its gate. Add as many individual checks
as the tree and discovered features require. These core IDs must also exist;
their evidence accounts for the complete corresponding reference branch, not
merely the short ID's name:

| Gate | Required IDs |
| --- | --- |
| Q0 | `Q0.provenance`, `Q0.capability` |
| Q1 | `Q1.uefi-usb`, `Q1.bios-usb`, `Q1.uefi-optical`, `Q1.boot-entries`, `Q1.live-functionality` |
| Q2 | `Q2.gui-uefi`, `Q2.gui-bios`, `Q2.default-install`, `Q2.storage-matrix` |
| Q3 | `Q3.disk-only`, `Q3.greeter`, `Q3.session-lifecycle` |
| Q4 | `Q4.inventory` plus all inventoried feature cases |
| Q5 | `Q5.inventory` plus all inventoried feature cases |
| Q6 | `Q6.applications`, `Q6.updates`, `Q6.rollback`, `Q6.persistence`, `Q6.recovery` |
| Q7 | `Q7.final-rerun` |

For example, Q1.boot-entries includes every advertised entry, including both
sufficient-memory and insufficient-memory copy-to-RAM behavior. Q2.storage-matrix
links individual cases, their restrictions, and actual resulting boot evidence.
Q7 records fixed defects/PRs and confirms a full reset/rerun on these final bytes.

## Dynamic desktop inventory

`inventories` has exactly `Q4` and `Q5`. Each contains `source_revision` (the
Sekka revision, which holds both Aizome's source and Tatami's), `evidence`
(source/menu inventory snapshots), and a nonempty `features` list. Inventory
all shell actions, applications, settings pages, menus, shortcuts and panels
described by their leaf references and the actual pinned source. A single summary feature
does not satisfy the skill even if its JSON shape is accepted.

```json
{
  "id": "Q4.app.sumi.save",
  "name": "Sumi Save As",
  "source_refs": ["desktop/aizome/docs/apps.md", "actual/pinned/source.rs:123"],
  "cases": {
    "success": "Q4.app.sumi.save.success",
    "cancel": "Q4.app.sumi.save.cancel",
    "recovery": "Q4.app.sumi.save.recovery"
  }
}
```

Each case names its own passing check at `FEATURE_ID.CASE`; it cannot reuse the
inventory summary or another feature's check. Use additional checks for distinct
routes, invalid inputs and persistence. `success` is always required. Only when
the operation truly has no cancellation or relevant recovery path may that case
instead be `{"not_applicable": "concrete reason tied to the actual feature"}`.
An unavailable adapter, untested behavior, or known defect is not such a reason.
The independent auditor reviews each exception.

## Independent final audit

After reproducing representative critical paths and reviewing every feature,
the separate reviewer writes an audit evidence file. Include its hash in the
evidence map, then calculate the digest:

```sh
nix run .#publish-nightly -- audit-digest QUALIFICATION_DIRECTORY
```

This SHA-256 is over UTF-8 JSON of the entire report except `audit`, with keys
sorted, compact separators, and Unicode preserved (`ensure_ascii=False`). Put
it in `audit.reviewed_report_sha256`. Do not put the digest inside the audit
evidence file itself, which would create a circular hash dependency.

`audit` requires `reviewer` (different from coordinator), `status: "pass"`,
`unresolved_findings: []`, `completed_at` (UTC Z, after the run completes),
`reviewed_report_sha256`, `reviewed_checks` (every check ID exactly accounted for),
`reproduced_checks`, and nonempty `evidence` references. Reproduction must include
at least `Q1.uefi-usb`, `Q2.gui-uefi`, `Q3.greeter`, `Q4.inventory`,
`Q5.inventory`, and `Q6.updates`; for inventory gates record which representative
actions the auditor personally repeated. Review still covers every other ID.

Any change outside `audit`, including an evidence hash, invalidates the audit
digest and needs renewed review. No publication occurs during `audit-digest` or
`validate`. Use validation to catch export errors before accessing credentials:

```sh
nix run .#publish-nightly -- validate ISO_DIRECTORY QUALIFICATION_DIRECTORY
```

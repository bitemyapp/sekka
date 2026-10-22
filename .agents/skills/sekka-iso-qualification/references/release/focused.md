# Focused release report

Use this narrower scope only when the user explicitly requests and approves it.
The exhaustive Q0–Q8 workflow and schema 1 remain the default. A focused report
must never claim exhaustive application or physical hardware qualification.

Schema 2 uses `profile: "focused-release-v1"` with the same exact candidate,
workstation, timestamps, evidence hashes and passing-check structure as schema 1.
The required IDs are `FOCUSED_IDS` in `scripts/publish-nightly.py`: actual UEFI
and BIOS media boot and GUI installation, an unchanged default installation,
media-free installed boots, Aizome greeter authentication, session lifecycle,
core Aizome and Tatami interaction, selected applications, networking and
persistence, plus provenance, capacity and the final exact-image rerun.

Both installations require real GUI input. Core desktop checks must include
launching apps, typing, file save/reopen, window controls and normal logout.
Greeter evidence includes rejected wrong password and successful normal login.
Disk-only evidence includes the launch arguments, mounts and a cold boot with
no ISO or installation share. Normal shutdown must exit QEMU cleanly.

`scope` contains a nonempty `authorization` description, `authorization_evidence`
referencing a hashed record of the user's approval, and nonempty `tested` and
`excluded` lists. `limitations` explicitly discloses untested scope and known
non-blocking issues. `inventories` is empty; it must not imply exhaustive coverage.
Failures affecting installation, boot, login or basic desktop use still block
publication. Other issues may be deferred only within the approved scope.

For the approved single-agent workflow, `audit.mode` is `coordinator-review`
and `audit.reviewer` equals `coordinator`. This openly records self-review.
The review accounts for every check and binds the final report digest; its
`reproduced_checks` lists the required checks actually performed by that reviewer.
It does not claim an independent repetition. Schema 1 still requires its separate
independent auditor. All source, hash, clean-checkout, immutable-upload,
conditional-index and workstation-only restrictions apply unchanged.

Publication records the profile and limitations in the public manifest/index,
and uploads the complete referenced report and evidence. Download the public
ISO and verify its checksum before declaring the download available and tested.

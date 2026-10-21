# Q6 — installed applications, updates, persistence and recovery

- Verify selected applications and desktop entries are present and unselected
  applications absent according to the installer's catalog contract. Run the
  application fixture's runtime checks, then open representative real apps
  from every category through the UI. Every first-party app must receive its
  functional inventory from Q4/Q5, not only a launch check.
- Browser: DNS/TLS, basic browsing, audio/video, file download, default URL
  handler and portal file/share selection. Use no personal login. Open a
  local document/image/media file and verify intended default handlers.
- Yukimi through the actual installed frontend: list/search/details, install
  and remove a small free app for each offered scope, dependency/progress
  reporting, cancel authentication, incorrect password, successful change,
  offline error and retry. Confirm installed files and persistent configuration.
- Inspect installed flake/lock and upstream tracking. Update using the normal
  UI and CLI paths offered by that revision; record before/after revisions,
  download/build log, generation and cache sources. Test a no-change update,
  failed network update and successful retry. Do not change production branches
  to create a test; use controlled disposable inputs where needed and label it.
- A default supported update should consume the documented caches; investigate
  unexpected large local builds. Unfree repackaging/driver exceptions must
  match `DEVELOPERS.md`, not an invented blanket allowance.
- Reboot into the new generation, authenticate and exercise Aizome and Tatami.
  Select the preceding generation in the boot menu and prove rollback boots,
  with no personal data loss. Return to the intended generation.
- Verify account, locale, timezone, keyring, network choices, desktop settings,
  wallpapers, application data and files persist across cold boots. Validate
  filesystem health, swap/encryption and service state again.
- Check installer logs, boot journal, failed system/user units and coredumps.
  Test bounded guest disk/memory pressure and recovery where it affects the
  installation experience. Never fill the host filesystem.
- Cover Kakehashi's downloaded-manifest/checksum behavior and USB media path in
  a disposable virtual target when relevant to public downloads. Never write
  a workstation's real disk as a testing shortcut.

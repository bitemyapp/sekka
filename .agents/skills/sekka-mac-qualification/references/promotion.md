# M9: promotion

Promotion uses the exact qualified bytes; nothing is rebuilt.

1. M9-01: put every artifact the ledger's candidate records (under the
   names it records), the signed catalog and the ledger in one directory,
   and run `installer/genkan/macos/packaging/publish-dry-run.py DIR --tool
   TOOL --published CURRENT` with the candidate revision's `sekka-mac-tool`.
   It refuses a signed catalog that `sekka-mac-tool verify` refuses as it
   stands (those exact bytes become the channel's file), a ledger that
   `ledger check` refuses, a ledger whose candidate recorded other bytes or
   another source revision or lock, changed bytes, mutable paths and a
   catalog not newer than the published one.
2. M9-02: an independent agent audits the evidence against this skill's
   whole tree.
3. M9-03: `packaging/audit-macho.py --verify --team TEAM --archive-team
   engine.tar.gz=ENGINE_TEAM` over the app and recovery.zip (build-app.sh
   --release runs it into `macho-audit.json`) shows every Mach-O signed by
   Sekka's team, except Asahi's Python runtime inside the engine archive,
   signed by the team `engine/source-lock.json` pins as `runtime_team`, and
   the one file it lists as `runtime_adhoc` (libffi, signed ad hoc), by its
   digest; `spctl --assess` accepts the notarized app. Read the notary log
   for any file inside `engine.tar.gz`: whether notarization accepts libffi
   there hasn't been seen yet, and if the log names it, the runtime is
   re-signed at pack time and the engine gets a new digest.
4. M9-04: the dry run's upload list is reviewed.
5. M9-05: publication itself is a separate, explicitly authorized request
   from the user. Afterwards, download the public bytes independently and
   compare them with the candidate.

CI never holds publication credentials for Mac artifacts, and nothing
publishes automatically.

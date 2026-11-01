# Working on Sekka

Read DEVELOPERS.md for the repository map and build commands. Develop on a topic branch from main and squash-merge reviewed work. Keep the release workflow inhibited unless the user explicitly asks to enable publication.

For ISO qualification, installation regressions, or nightly publication, follow [.agents/skills/sekka-iso-qualification/SKILL.md](.agents/skills/sekka-iso-qualification/SKILL.md). Its coordinator checklist is mandatory; load the relevant reference branches as each phase starts. Component and instrumented fixture tests do not certify the actual ISO.

Use local first-party source through the root flake. Preserve separate Cargo workspaces, narrow package source filters, existing Nix option compatibility, and GUI/helper privilege boundaries. Do not run disk mutation or installer fixtures against host devices. Use disposable test VMs.

Do not commit bulk data: benchmark captures, raw traces and logs, generated corpora, large test datasets, recordings, or build artifacts. Keep these outside the Git repository and track compact reports plus an immutable retrieval location, byte size and SHA-256 checksum; provide a verified fetch or restoration command when needed. Keep committed regression fixtures small and targeted. Use hash-pinned fetched inputs for large runtime assets. Do not use Git archive exclusions such as `export-ignore` to hide bulk files: Git and GitHub flake sources must have matching NAR hashes.

User-facing README prose should explain the experience and current limits; implementation and release details belong in DEVELOPERS.md. Keep screenshots at their original resolution with honest provenance. Run checks appropriate to changed components and distinguish component checks, image builds, VM installation, and physical hardware validation.

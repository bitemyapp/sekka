# Working on Sekka

Read DEVELOPERS.md for the repository map and build commands. Develop on a topic branch from main and squash-merge reviewed work. Keep the release workflow inhibited unless the user explicitly asks to enable publication.

For ISO qualification, installation regressions, or nightly publication, follow [.agents/skills/sekka-iso-qualification/SKILL.md](.agents/skills/sekka-iso-qualification/SKILL.md). Its coordinator checklist is mandatory; load the relevant reference branches as each phase starts. Component and instrumented fixture tests do not certify the actual ISO.

Some defaults are trade-offs the owner chose on purpose, and read as defects in a review. They are marked "Decided on purpose" where they are set and listed under that heading in the component's notes (Chanoma's: [desktop/aizome/docs/chanoma.md](desktop/aizome/docs/chanoma.md#decided-on-purpose)). Do not reverse one as a fix. Raise it with the user instead.

Use local first-party source through the root flake. Preserve separate Cargo workspaces, narrow package source filters, existing Nix option compatibility, and GUI/helper privilege boundaries. Do not run disk mutation or installer fixtures against host devices. Use disposable test VMs.

User-facing README prose should explain the experience and current limits; implementation and release details belong in DEVELOPERS.md. Keep screenshots at their original resolution with honest provenance. Run checks appropriate to changed components and distinguish component checks, image builds, VM installation, and physical hardware validation.

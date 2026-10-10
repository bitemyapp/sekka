# Working on Sekka

Read DEVELOPERS.md for the repository map and build commands. Develop on a topic branch from main and squash-merge reviewed work. Keep the release workflow inhibited unless the user explicitly asks to enable publication.

Use local first-party source through the root flake. Preserve separate Cargo workspaces, narrow package source filters, existing Nix option compatibility, and GUI/helper privilege boundaries. Do not run disk mutation or installer fixtures against host devices. Use disposable test VMs.

User-facing README prose should explain the experience and current limits; implementation and release details belong in DEVELOPERS.md. Keep screenshots at their original resolution with honest provenance. Run checks appropriate to changed components and distinguish component checks, image builds, VM installation, and physical hardware validation.

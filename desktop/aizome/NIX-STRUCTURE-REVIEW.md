# Nix structure review: Sekka and Aizome

Draft for review, 2026-10-24. Not checked in. Nothing in either repository was
changed to write it.

Reviewed: `~/work/sekka` at `99bda5d` (checkout on `feat/tatami-sound`) and
`~/work/aizome` at `bd73b8b` (`fix/polkit-recovery`). Excludes
`.claude/worktrees`, `result`, and Rust code except where it reads or writes
Nix.

## Summary

Some of the critique is right and some of it isn't.

**Right:**

- Sekka's Nix is hard to navigate. `flake.nix` is 626 lines, and about 350 of
  them are inline checks.
- First-party components are disguised as flake inputs.
- One concept has three names (`calamares`, `respin`, Genkan).
- Modules get their arguments through `specialArgs` threaded in from seven
  places.
- The installed system's modules live under the installer's Rust crate
  (`installer/genkan/rust/system/`).
- Facts about desktops are spread across four places, one of them Genkan's
  Rust.

None of that is a bug. It's accretion from importing several repositories into
one, and someone new to the code can't tell what is a feature, what is wiring
and what is history.

**Not right, or only partly:**

- **"Doesn't fix upstream."** Sekka and Aizome carry seven package fixes. Most
  have a regression check with a negative control and a written removal
  condition, which is more rigorous than most configurations. What's missing is
  a single place that lists them, a consistent mechanism for applying them, and
  a record of what has gone upstream.
- **"Interactive editing will end in disaster."** The mechanism is more careful
  than it looks:
  - lossless `rnix` edits
  - an allowlist of settings
  - a backup taken before every change, restored on failure
  - a build before every switch
  - a copy of the configuration kept for every generation
  - lock-file diffs checked against the requested update

  The real weaknesses are structural:
  - Tools edit a file a person wrote.
  - The allowlist is stringly typed and repeats the assertions.
  - The editing crates exist in two copies that are drifting apart.

The recommendations, most valuable first:

1. **Restructure Sekka's flake with flake-parts and an auto-imported `parts/`
   tree.** One file per feature module, system, check and app. Each exported
   module carries a `key`, so groups can overlap safely. This should be a pure
   move; see [Order of work](#order-of-work) for how to show that.
2. **Add a desktop registry** (`desktops.json` beside `applications.json`). The
   NixOS module, Yukimi's choices, Genkan and the reference systems all read
   it.
3. **Use one option namespace, `sekka.*`**, keeping `calamares.*` and
   `respin.*` working as aliases.
4. **Replace `specialArgs` and `_module.args` with modules that close over
   Sekka's inputs**, and with typed options. The installed `flake.nix` then
   shrinks to `modules = [ sekka.nixosModules.default ./configuration.nix ]`.
5. **Give Genkan and Yukimi a settings file only they write** (`sekka.json`,
   applied at priority 900). They stop editing `configuration.nix`, which a
   person's own settings still override. Yukimi can then show "set in your
   configuration" instead of fighting it.
6. **Keep a ledger of upstream workarounds** that checks can read, with a rule
   for how patches are applied. Send the module bugs upstream. Replace an
   upstream module wholesale only where Sekka already overrides it three or
   more times.
7. **Aizome:**
   - Split the 493-line module.
   - Separate session settings from system policy (polkit grants, the time
     zone takeover, wake alarms).
   - Build the default package with the consumer's nixpkgs.
   - Add a test helper; 18 of the 23 VM tests repeat the same block of about
    25 lines.

## The critiques, one at a time

### "Doesn't fix the issues made by upstream; replace upstream modules"

**Package fixes:**

| Fix | Where | Mechanism | Check |
| --- | --- | --- | --- |
| Aquamarine null-connector teardown (backport) | `sekka/nix/overlays/aquamarine.nix` | overlay, from a module | `checks.aquamarine`, with negative control |
| Hyprland screencopy sessions (proposed upstream) | `sekka/nix/overlays/hyprland.nix` | overlay, from a module | `checks.screenshare-sessions`, with negative control |
| wpa_supplicant scan handler | `sekka/installer/genkan/rust/system/wpa-supplicant/` | overlay, from a module | `checks.wpa-supplicant-scan` |
| Elephant search (Sekka only) | `sekka/desktop/tatami/elephant/` | own package | `checks.launcher-search`, with negative control |
| ChatGPT executor copy | `sekka/nix/chatgpt/` | own package | `checks.chatgpt-plugins` |
| xdg-desktop-portal-wlr captures | `aizome/nix/module.nix:53` | `let`-bound override | VM tests, indirectly |
| wf-recorder memfd buffers | `aizome/nix/module.nix:107` | `let`-bound `substituteInPlace` | none |

**Workarounds for upstream module behaviour:** about 20 places, including:

- `boot.zswap` still sets `zswap.zpool`, which Linux 6.18 removed
  (`tuning.nix`).
- The rootless Docker user unit excludes only root (`applications.nix`).
- Plasma Login Manager starts a bare X server, and its `XDG_DATA_DIRS` is
  forced (`desktops.nix`).
- ananicy's cgroup realtime workaround is forced off (`tuning.nix`).
- The autostart entries for polkit-gnome and IBus are overridden.
- GDM's hidden sessions are worked around.
- nm-applet gets a drop-in for Aizome sessions.
- xdg-desktop-portal-wlr's `ExecStart` is replaced so it can find its
  configuration.
- Aizome takes over the time zone through activation scripts.

**Verdict:** wrong about the package fixes, fair about the module workarounds.

The workarounds are well commented, but each one lives at its point of use.
Nothing records which ones are upstream bugs, which have been reported and when
each can go. Wholesale replacement is a reasonable tool for a personal
configuration. A distribution that tracks nixpkgs pays for every vendored module
on each nixpkgs update. Use it only where the overrides already outweigh the
module (see [Upstream fixes](#upstream-fixes)).

### "Looks vibed, not easy to reason about, no tree-like module structure"

**Verdict:** fair for Sekka, mostly unfair for Aizome's flake. Specifically:

- **`flake.nix` does everything.**
  - Outputs and roughly 350 lines of checks share one `let`. The checks are
    `assert` chains: `live-profiles` at `flake.nix:263`, `nvidia-on-media` at
    `:343`, `tatami-session-entry` at `:403`.
  - `x86_64-linux` is hard-coded 10 times across `flake.nix`, `nix/`,
    `modules/`, `installer/` and `tests/`.
- **Pseudo-inputs.**
  - `nix/components.nix` builds `tatami`, `yukimi`, `kakehashi`, `calamares`
    and `catalog` as attribute sets shaped like flakes, and `flake.nix:50`
    merges them into `inputs`.
  - So `inputs.calamares.nixosModules.desktops` reads like an external flake but
    is `installer/genkan/rust/system`. DEVELOPERS.md has to explain that
    `calamares` is "not another source repository".
- **One concept, three names:**
  - the `calamares.*` options
  - the `respin.*` options in `modules/installer.nix`
  - `respin-tools`
  - Genkan, the installer's actual name
- **Hidden arguments.**
  - `applicationPkgs`, `aiPackages` and `ompPackage` arrive through
    `specialArgs`, and `sekkaPackages` through `_module.args`.
  - They are set in seven places: `templates/flake.nix.in`,
    `nix/reference-systems.nix`, `nix/applications.nix`, `nix/components.nix`,
    `tests/desktop-matrix.nix`, `nix/fish-vm-test.nix` and
    `nix/yukimi-discover.nix`.
  - Package resolution is written twice: `packagesFor` in
    `installer/genkan/rust/src/applications.nix` and `packageList` in
    `nix/applications.nix`.
- **The path says the wrong thing.** The installed system's feature modules
  (tuning, nvidia, hyprland, binary cache, Wi-Fi, store space, USB wake) live
  under `installer/genkan/rust/system/`, which reads as "the installer's Rust
  crate".
- **Desktop facts are scattered:**
  - `desktops.nix`: `sessions`, `renamed`, `x11`, and the `titles` and
    `offered` lists inside Yukimi's choices
  - Genkan's `desktop.rs`: ids, titles and descriptions
  - `respin.referenceDesktops`
  - the test matrices
- **Dead compatibility shims within one repository.** For example,
  `applications.nix:112` checks `options ? programs.yukimi.catalogs`, but
  Yukimi's module is now in the same checkout.

**Aizome:** the flake is 141 lines and readable. The problems are the
single-file module and the duplicated tests (see [Aizome](#aizome)).

### "flake-parts would be perfect for interactive config updating"

**Verdict:** the structure is worth adopting for the repository. The mechanism
(the GUI choosing which modules are imported) is wrong for a distribution:

- `imports` can't depend on `config`, so a GUI-chosen import list has to be
  spliced into a file. That is the editing we want less of.
- An option that isn't imported doesn't exist. Yukimi couldn't read its type,
  its default or whether it is set.
- NixOS's own convention is to import everything and turn features on with
  options. That keeps every option documented and lets assertions span
  features.

So use flake-parts groups to organize the repository, and keep enable options as
the interface people and Yukimi use. More in [What not to
adopt](#what-not-to-adopt).

### "Entities" for external data

**Verdict:** the best idea in the critique, and Sekka already has one.
`applications.json` is read by Genkan's Rust, by the NixOS module, by Yukimi's
Discover and by the cache builder. Desktops should be next, then binary caches,
the workaround ledger and wallpapers. See [Entities](#entities).

### "Interactive nix config editing will end in disaster"

**What exists**, in `yukimi-helper/src/main.rs` and `yukimi-config`:

- **Edits:**
  - One setting is changed by replacing one expression's text.
  - The result must parse again or the change is refused.
  - Of the system's own settings, only those declared in
    `programs.yukimi.choices` and in catalogs can be changed, and only to the
    values listed.
  - Packages go into a file of their own, `yukimi.nix`.
  - Some edits do reach files people wrote: removing a package from
    `environment.systemPackages` or a user's `packages` (`--unlist-*`), and
    repointing a flake input (`--follow`).
- **Applying:**
  - Every file touched is backed up first.
  - The new system is built before switching.
  - Anything that fails before the switch restores the backups.
  - The configuration directory is copied for each generation, so a rollback
    brings back the choices that made it.
  - Proposed lock files are checked to change only the requested inputs.
- **Safety:** changes that could tear down the running session are staged for
  the next boot instead of switched to. A pre-switch check enforces this for
  other rebuild front ends too.

**What's actually risky:**

1. **Tools edit a file a person wrote.** The `calamares.*` lines in
   `configuration.nix` are edited in place. A person who restructures that file
   (`let`, `mkIf`, moving lines into an import) gets "left alone" errors, or
   worse, Yukimi edits one definition while another wins.
2. **The interface is strings.** `setting = "calamares.loginScreen"` and
   `needs = "calamares.desktops=aizome"` form a small language with no type
   checking. `needs` repeats an assertion (`desktops.nix:284` and `:314`).
3. **Two copies of the editing code.**
   - `aizome/apps/yukimi/crates/` is a copy of `sekka/apps/yukimi/crates/`
     that has already drifted: `lock.rs`, `nix.rs` and `updates.rs` differ.
   - Aizome's Yukimi still drives Sekka's privileged helper, so the UI and the
     helper can disagree about what a configuration means.

See [Interactive configuration](#interactive-configuration) for the proposal.

### "Doesn't use proper interfaces"

**Verdict:** partly fair. The concrete leaks:

- **`specialArgs` as a public interface.** The installed `flake.nix` that Genkan
  writes includes wiring (`import applications { … }`, `aiPackages`,
  `ompPackage`) that every installed system must reproduce exactly.
- **A desktop module that sets system policy by default.** Aizome's README
  offers the module to any NixOS system, but `programs.aizome.enable`:
  - Overrides the person's `time.timeZone` at priority 90, which beats the
    normal 100. A `time.timeZone = "Europe/Paris"` is silently discarded
    (`module.nix:248`).
  - Lets wheel users change the time, the host name, the locale and the network
    without a password (`module.nix:400`).
  - Grants `CAP_WAKE_ALARM` to every user manager (`module.nix:317`).

  Each is right for Sekka. None should be the default for someone who only
  wants the session.
- **Overlays from modules.** Any consumer of `nixosModules.tatami` or
  `nixosModules.desktops` gets Sekka's Hyprland, Aquamarine and wpa_supplicant
  overlays applied to its whole package set.

## Proposed layout for Sekka

### flake-parts with an auto-imported tree

Every file under `parts/` is a flake-parts module, imported by
[import-tree](https://github.com/vic/import-tree). Adding a feature means adding
a file, and `flake.nix` stays at about 40 lines. Component directories
(`desktop/tatami`, `apps/yukimi`, `installer/genkan`) keep their `module.nix` and
`package.nix` next to their source. `parts/` registers them.

```
flake.nix                    inputs, nixConfig, mkFlake
desktops.json                the desktop registry (beside applications.json)
parts/
  flake/        systems.nix  formatter.nix  devshells.nix
  entities/     desktops.nix  caches.nix  workarounds.nix  wallpapers.nix
  packages/     tatami.nix  yukimi.nix  kakehashi.nix  genkan.nix  hyprland.nix  tools.nix
  nixos/
    base.nix          binary cache, store space          -> nixosModules.base
    tuning.nix                                           -> nixosModules.tuning
    nvidia.nix                                           -> nixosModules.nvidia
    wifi.nix          the wpa_supplicant fix             -> nixosModules.wifi
    usb-wake.nix                                         -> nixosModules.usb-wake
    desktops.nix      sekka.desktops.*, login screen     -> nixosModules.desktops
    applications.nix                                     -> nixosModules.applications
    settings.nix      the managed sekka.json             -> nixosModules.settings
    components.nix    tatami, yukimi, aizome             -> nixosModules.{tatami,yukimi,aizome}
    groups.nix        nixosModules.default = the groups above
  systems/
    installer.nix     nixosConfigurations.installer, packages.iso
    reference.nix     nixosConfigurations.reference-<desktop>, packages.installed-systems
  checks/
    live-profiles.nix  nvidia-on-media.nix  binary-cache.nix  tatami-session-entry.nix
    vm/               one file per VM test
  apps/         update.nix  push-cache.nix  publish-nightly.nix
modules/
  desktops/     default.nix  aizome.nix  plasma.nix  gnome.nix  xfce.nix  hyprland.nix  legacy.nix
  ...           the NixOS module bodies moved out of installer/genkan/rust/system/
```

`flake.nix`:

```nix
{
  description = "Sekka: NixOS with Genkan, Tatami and Yukimi";
  inputs = {
    nixpkgs.url = "https://flakehub.com/f/NixOS/nixpkgs/0.1";
    flake-parts.url = "github:hercules-ci/flake-parts";
    flake-parts.inputs.nixpkgs-lib.follows = "nixpkgs";
    import-tree.url = "github:vic/import-tree";
    # determinate, fh, applications, ai-apps, omp, aizome, superluminal,
    # tracy: unchanged.
  };
  # Stays literal here: Nix reads nixConfig without evaluating anything.
  # checks.binary-cache keeps it equal to the caches entity.
  nixConfig = { /* unchanged */ };
  outputs = inputs: inputs.flake-parts.lib.mkFlake { inherit inputs; } (inputs.import-tree ./parts);
}
```

A feature file:

```nix
# parts/nixos/tuning.nix
{
  flake.nixosModules.tuning = {
    # Imported once however many groups include it.
    key = "sekka#tuning";
    imports = [ ../../modules/tuning.nix ];
  };
}
```

The groups, in one place:

```nix
# parts/nixos/groups.nix
{ self, ... }:
{
  # What an installed Sekka imports. Everything is imported; features turn
  # on through options (sekka.tuning.enable, sekka.desktops.selected, ...).
  flake.nixosModules.default.imports = [
    self.nixosModules.base
    self.nixosModules.settings
    self.nixosModules.desktops
    self.nixosModules.applications
    self.nixosModules.tuning
    self.nixosModules.nvidia
    self.nixosModules.wifi
    self.nixosModules.usb-wake
    self.nixosModules.yukimi
    self.nixosModules.tatami
    self.nixosModules.aizome
  ];
}
```

**Module keys.** Give every exported module a `key`. NixOS imports a given key
only once. Without keys, a module reached through two groups is imported twice,
and its options are declared twice. `nix/components.nix:59` already does this
for `applications` for that reason, and `modules/hyprland-fixes.nix` relies on
path identity for the same effect.

**Systems are real `nixosConfigurations`.** The reference systems become
`nixosConfigurations.reference-plasma` and so on, instead of values built inside
a function. Then `nix repl`, `nixos-option` and `nix build
.#nixosConfigurations.reference-plasma.config.system.build.toplevel` work on
them like on any system.

**`perSystem` removes the hard-coded `x86_64-linux`.** Sekka still builds for
one system, but the system is named once, in `parts/flake/systems.nix`.

**Cost:** two more inputs (flake-parts and import-tree). Because
`nix/installed-lock.nix` re-roots Sekka's whole lock, both also appear in every
installed system's `flake.lock`. That's harmless, but people will see it.

### One namespace: `sekka.*`

| Now | Proposed |
| --- | --- |
| `calamares.desktops` | `sekka.desktops.selected` |
| `calamares.defaultDesktop` | `sekka.desktops.default` |
| `calamares.loginScreen` | `sekka.loginScreen` |
| `calamares.applications` | `sekka.applications` |
| `calamares.installUser` | `sekka.primaryUser` |
| `calamares.tuning.enable`, `calamares.zswap.enable` | `sekka.tuning.enable`, `sekka.zswap.enable` |
| `calamares.nvidia.*`, `calamares.hyprland.enable` | `sekka.nvidia.*`, `sekka.hyprland.enable` |
| `calamares.binaryCache.*` | `sekka.binaryCache.*` |
| `respin.referenceDesktops`, `respin.cacheApplications` | `sekka.media.*` |

Keep the old names working forever. Installed `configuration.nix` files contain
them.

1. Start with `lib.mkAliasOptionModule`, which doesn't warn.
2. Ship Yukimi's migration, which moves the settings into `sekka.json` (see
   below).
3. Then switch to `lib.mkRenamedOptionModule` so that hand-written uses get a
   warning.

`desktops.nix` already uses renamed options for `calamares.omarchy` and
`calamares.tatami`. The names in Genkan's generator, Yukimi's choices and
`sekka.rs`'s migration change together.

Rename `installer/genkan/rust/system/` to `modules/` at the same time, so the
path says what the files are.

### Modules that close over Sekka's inputs

Today the installed flake has to supply `applicationPkgs`, `aiPackages` and
`ompPackage`, and three other places repeat that wiring. Instead, the module
defines them as typed options whose defaults come from Sekka's own inputs:

```nix
# parts/nixos/applications.nix
{ inputs, self, ... }:
{
  flake.nixosModules.applications = { lib, pkgs, ... }:
    let system = pkgs.stdenv.hostPlatform.system; in
    {
      key = "sekka#applications";
      imports = [ ../../modules/applications.nix ];
      # Overridable from configuration.nix, or by pointing the installed
      # flake's `applications` input elsewhere, as today.
      sekka.packageSets = {
        applications = lib.mkDefault (import inputs.applications {
          inherit system;
          config.allowUnfree = true;
        });
        ai = lib.mkDefault inputs.ai-apps.packages.${system};
        omp = lib.mkDefault inputs.omp.packages.${system}.omp;
        sekka = lib.mkDefault self.packages.${system};
      };
    };
}
```

The installed template becomes:

```nix
{
  inputs.sekka.url = "github:bitemyapp/sekka/nightly";
  inputs.nixpkgs.follows = "sekka/nixpkgs";
  # The other follows lines are unchanged: they still let a person repoint an input.
  outputs = { nixpkgs, sekka, ... }: {
    nixosConfigurations.@HOSTNAME@ = nixpkgs.lib.nixosSystem {
      system = "x86_64-linux";
      modules = [
        sekka.nixosModules.default   # also brings determinate's module and fh
        (sekka.lib.settings ./sekka.json)
        ./configuration.nix
      ];
    };
  };
}
```

**Existing installed flakes keep working without changes.** The `specialArgs`
they still pass name the same inputs (they follow `sekka/*`), so they're
redundant, not wrong. Yukimi can drop them later with the `rnix` migration it
already has in `sekka.rs`.

**`desktop-matrix.nix`, `application-matrix.nix` and `reference-systems.nix`**
stop rebuilding `specialArgs` and import `self.nixosModules.default`. The
duplicate `packageList` and `packagesFor` become one function in the module,
which `nix/applications.nix` reads back through `config`.

### Move the checks out of `flake.nix`

The `live-profiles`, `nvidia-on-media`, `binary-cache`, `tatami-session-entry`
and `kernel-settings` checks go to `parts/checks/*.nix`, one file each.

Some of the `live-profiles` asserts are invariants of the installer module
itself rather than tests: the Wi-Fi backend, the supported filesystems, Plymouth
off. Those belong in that module's `assertions`, so they fail at evaluation for
anyone, not only under `nix flake check`.

## Entities

### Desktops

There is already a precedent: `applications.json` is the single source for
Rust and Nix. `desktops.json` would sit beside it, with one entry per desktop:

```json
{
  "aizome":   { "title": "Aizome", "session": "aizome", "offered": true, "prebuilt": false,
                "wallpaper": "da-nang-at-night.jpg" },
  "tatami":   { "title": "Tatami", "formerNames": ["omarchy"], "offered": true, "prebuilt": true },
  "plasma":   { "title": "KDE Plasma", "blurb": "Full-featured and familiar, with deep customization.",
                "offered": true, "prebuilt": true, "wallpaper": "ocean-wave.jpg" },
  "gnome":    { "title": "GNOME", "offered": true, "prebuilt": true, "wallpaper": "foggy-forest.jpg" },
  "hyprland": { "title": "Hyprland", "session": "hyprland-uwsm", "offered": true, "prebuilt": true },
  "xfce":     { "title": "Xfce", "x11": true, "offered": true, "prebuilt": true },
  "cinnamon": { "title": "Cinnamon", "x11": true, "offered": false },
  "lxqt":     { "title": "LXQt", "x11": true, "offered": false }
}
```

The values above are illustrative. Fill them in from `desktops.nix`,
`desktop.rs` and `respin.referenceDesktops`.

Who reads it:

- **`modules/desktops`:**
  - `sessions`, `renamed` and `x11` are derived from it.
  - The `enum` type of `sekka.desktops.selected` is built from its keys.
  - Yukimi's `choices` (titles, offered values) are generated from it instead
    of written out by hand.
- **Genkan:** reads it with serde, the way it reads `applications.json`.
  `desktop.rs` keeps the behaviour and loses the tables.
- **The reference systems and test matrices:** `prebuilt` replaces the
  `respin.referenceDesktops` default list.
- **A flake-parts option** (`parts/entities/desktops.nix`) declares the schema
  and sets its value from the JSON. A misspelled field then fails
  `nix flake check` instead of being ignored.

### Other entities

These are Nix-only, so each is a flake-parts option:

- **`caches`:** each substituter's URL, key and priority. Read by
  `binary-cache.nix`, `push-cache`, the site, and `checks.binary-cache`, which
  compares the entity with the literal `nixConfig`. That duplication is forced
  by Nix, so the check stays.
- **`workarounds`:** the ledger in [Upstream fixes](#upstream-fixes).
- **`wallpapers`:** file names that `desktops.nix` and Aizome now refer to as
  strings (`"da-nang-at-night.jpg"`). Typed references would fail at evaluation
  when a file is renamed.

## Upstream fixes

### A ledger checks can read

```nix
# parts/entities/workarounds.nix (excerpt)
{
  sekka.workarounds = {
    aquamarine-null-connector = {
      kind = "package";             # package | module
      status = "backport";          # backport | proposed | reported | local
      upstream = "https://github.com/hyprwm/aquamarine/commit/fc77bd58a2c9ef6d0760a7c9c3b7b6670f956f28";
      package = "aquamarine";
      fixedAfter = "0.15.1";        # the patch must go once the package is newer
      check = "aquamarine";
    };
    zswap-zpool = {
      kind = "module";
      status = "local";             # not yet reported
      module = "boot.zswap";
      summary = "Sets zswap.zpool, which Linux 6.18 removed";
      check = "tuning";
    };
  };
}
```

A `checks.workarounds` derivation would then:

- **Check every `check` name exists**, so no workaround is untested.
- **Fail once a patched package passes `fixedAfter`.** DEVELOPERS.md currently
  asks people to remember that review after each input update.
- **Write the table** that DEVELOPERS.md now keeps by hand.

### One rule for applying package fixes

1. **Use the module's `package` option where there is one.** For example,
   `programs.hyprland.package = pkgs.hyprland.override { aquamarine =
   patchedAquamarine; }`. This is what Aizome already does for
   xdg-desktop-portal-wlr.
2. **Override only the package that needs it.** Use `.override` or
   `.overrideAttrs` on that package, not a system-wide overlay.
3. **Use `nixpkgs.overlays` from a module only when nothing else works.** That
   means the fixed package must replace the original everywhere, and the module
   that uses it has no `package` option (wpa_supplicant under NetworkManager).
   Record the missing option as an upstream gap in the ledger.

Overlays from an imported module change the package set of the whole system,
including every reverse dependency, for anyone who imports the module.

### Send the module bugs upstream

The code comments already describe these as nixpkgs behaviour. I haven't checked
whether any of them has been reported.

- `boot.zswap` sets `zswap.zpool`, which Linux 6.18 removed (`tuning.nix`).
- The rootless Docker user unit is conditioned only on not being root, so system
  users' managers start it at boot (`applications.nix:233`).
- Plasma Login Manager starts X11 sessions with the bare X server, without
  NixOS's arguments (`desktops.nix`, `plasmaLoginX`).
- The ananicy module forces `cgroup_realtime_workaround`, which moves
  compositors out of their units (`tuning.nix:122`).
- Xfce's polkit-gnome autostart entry has no `OnlyShowIn` (an upstream package
  issue).
- xdg-desktop-portal-wlr doesn't read `XDG_CONFIG_DIRS` (upstream project;
  `aizome/nix/module.nix`).
- wf-recorder puts its shared-memory buffers in `/tmp` (upstream project).

### When to replace an upstream module

RGBCube replaces upstream modules. For Sekka, do it only where both are true:

- There are already three or more overrides against the module, or overrides
  that force its internals.
- The module is small enough to own.

**The one candidate today:** Plasma Login Manager. It has the X server wrapper,
forced `XDG_DATA_DIRS` on both the system and user units, and the KWin cursor
drop-in.

**How to do it:**

- `disabledModules` the upstream file.
- Copy it to `modules/upstream/`, with a header naming the nixpkgs revision it
  came from.
- Add a check that fails when the upstream file changes, so each nixpkgs update
  forces a review.

Everything else stays as a commented override plus a ledger entry.

## Interactive configuration

### A settings file only tools write

Genkan and Yukimi write `/etc/nixos/sekka.json` and nothing else. The person's
`configuration.nix` is never edited after installation, except by a one-time
migration. Sekka applies the JSON at priority 900. That is above module defaults
(1000) and below anything a person writes (100):

```nix
# Sekka's flake: lib.settings
file:
{ lib, ... }:
{
  _file = file;
  # Lists are leaves for mapAttrsRecursive, so each setting is one definition.
  config = lib.mapAttrsRecursive (_: lib.mkOverride 900) (lib.importJSON file);
}
```

```json
{ "sekka": { "desktops": { "selected": ["aizome", "plasma"], "default": "aizome" },
             "loginScreen": "auto",
             "applications": ["firefox", "fish"] } }
```

This works because of how priorities filter definitions. I checked it with
`lib.evalModules`: a `listOf str` option with a priority-900 definition
`["managed"]` and a normal one `["person"]` evaluates to `["person"]`, with
`highestPrio = 100`. So the person's value replaces the managed one instead of
being concatenated with it.

What changes:

- **Yukimi writes JSON with serde.** There's no `rnix` surgery for routine
  changes. The option types validate the values on the build Yukimi already runs
  before switching. Packages (`yukimi.nix`) can move into the same file.
- **The careful machinery is kept for what needs it.** Removing a package a
  person listed (`--unlist-*`) and repointing an input (`--follow`) still edit
  files people wrote. Those stay lossless `rnix` edits with backups. They become
  the exception instead of the normal path.
- **Yukimi can tell when it isn't in charge.**
  - `/etc/yukimi/system.json` gains each choice's
    `options.<path>.highestPrio`.
  - 900 means `sekka.json` decides. 1500 means the default.
  - Anything else means the person's configuration decides, and Yukimi shows
    "Set in your configuration" instead of an editable control.
- **Genkan writes two files.** `configuration.nix` holds what only the installer
  decides (boot, filesystems, user, host name). `sekka.json` holds what Yukimi
  may change later.
- **Migration is one `rnix` edit per system.** It moves the
  `calamares.*` lines into `sekka.json` and adds the `lib.settings` line to
  `flake.nix`, using the backup and restore machinery that exists.

### Choices derived from declarations

The registry generates `programs.yukimi.choices`: allowed values from the
`enum`, titles and descriptions from `desktops.json`. Constraints are declared
once and produce both the assertion and the UI hint. For example, a login-screen
entry `aizome = { needsDesktop = "aizome"; }` would produce both the assertion
at `desktops.nix:314` and the `needs` string at `:284`.

### One copy of the editing code

Make Aizome depend on Sekka's `yukimi-config`, `yukimi-store` and
`yukimi-system` crates instead of copying them.

A flake input creates a cycle, because Sekka already depends on Aizome. Two
options avoid it:

- **A Cargo git dependency pinned in `Cargo.lock`.** Nix builds it through
  `importCargoLock`'s `outputHashes`.
- **A small repository for the crates.** Both projects then depend on it.

Either way, the helper and every UI use the same code to read a configuration.

### Test the choice space

`tests/desktop-matrix.nix` already evaluates configurations that the Rust backend
emits. Generate its cases from the registry: each offered desktop alone, every
pair, and each login screen with a desktop it needs. The matrix then covers what
Yukimi can actually write, not a hand-picked list.

## Aizome

1. **Split `nix/module.nix`** by concern:

   | File | Contents |
   | --- | --- |
   | `session.nix` | entry, environment, slices |
   | `portals.nix` | wlr wrapper, gtk portal, share picker |
   | `services.nix` | NetworkManager, BlueZ, UPower, UDisks, geoclue and the others |
   | `apps.nix` | Tokei, Koyomi, keep-awake units, mimeapps |
   | `theme.nix` | dconf profile, fonts, foot |
   | `greeter.nix` | as now |
   | `policy.nix` | below |

2. **Make system policy opt-in.**
   - `programs.aizome.enable` gives the session and the services it needs.
   - A new `programs.aizome.systemSettings.enable` (default `false`) covers:
     - the wheel polkit grants
     - `mutableTimeZone` (the priority-90 `time.timeZone` takeover and the
       activation scripts)
     - `CAP_WAKE_ALARM` for user managers
   - Sekka's desktops module turns it on.
   - When `mutableTimeZone` discards a `time.timeZone` the person set, emit a
     `warnings` entry. `options.time.timeZone.definitionsWithLocations` shows
     whose definition it was.

3. **Build the default package with the consumer's nixpkgs.**
   - `package` defaults to `self.packages.${system}.aizome`, which is built with
     Aizome's pinned nixpkgs. Sekka sets `follows`, so it isn't affected.
   - Any other consumer gets a second nixpkgs and a Mesa, libgbm and libdrm that
     differ from its `/run/opengl-driver`.
   - Default to `pkgs.callPackage ./package.nix { inherit src sidestep; … }`
     inside the module, closing over `self` for `src`.

4. **Add a test helper.** 18 of the 23 VM tests repeat the greetd `alice`
   auto-login and the virtio-gpu block.
   - Add `nix/tests/lib.nix` with `mkSessionTest { name; resolution ? …;
     extraConfig ? { }; script; }`.
   - Discover tests from `nix/tests/<name>/{default.nix,test.py}`, so `flake.nix`
     stops listing all 23.

5. **Move the patches to `nix/patches/`**, with entries in the same kind of
   ledger as Sekka's. xdg-desktop-portal-wlr and wf-recorder are both upstream
   bugs.

6. **flake-parts is optional for Aizome.** The flake is small, and the gain is
   consistency with Sekka. While there, consider limiting the VM checks to
   `x86_64-linux`. `forAll` now defines every VM check for `aarch64-linux` too.

## What not to adopt

- **Home Manager modules.** Aizome's apps own their per-user state: Settings
  writes `~/.config`, as do Fusen and the others. Home Manager links read-only
  store files into the same place, so the two would fight. Sekka is also
  multi-user, configured through `/etc/xdg` with per-desktop `XDG_CONFIG_DIRS`,
  which is the right layer for a distribution.
- **Letting the GUI choose the import list.** See above: imports can't depend on
  `config`, and options that aren't imported are invisible to Yukimi.
- **Pipe operators (`<|`).** RGBCube's snippet uses them. They are an
  experimental feature that every installed system would need turned on.
- **Replacing upstream modules by default.** Use the criteria above.

## Order of work

Each step is its own pull request, smallest risk first. Steps that only move
code should leave the derivations the flake evaluates to unchanged, as long as
the step doesn't move source files that end up in the store (`src` filesets,
`${./file}` references). Compare before and after:

```sh
nix eval --raw .#nixosConfigurations.installer.config.system.build.toplevel.drvPath
nix eval --raw .#packages.x86_64-linux.installed-systems.drvPath
nix eval --json .#checks.x86_64-linux --apply 'builtins.mapAttrs (_: c: c.drvPath)'
```

Identical output shows the refactor changed no derivation, which is the best
answer to "vibed". Steps that change behaviour say what they change, and their
diffs should be limited to that (for example `/etc/yukimi/system.json`).

**Sekka:**

1. Move the inline checks to `nix/checks/*.nix`. No new inputs.
   (Same derivations.)
2. Add flake-parts and import-tree, and port outputs file by file into
   `parts/`. Add `key` to every exported module. (Same derivations.)
3. Stop using pseudo-inputs internally. Keep the
   `nixosModules.{desktops,applications,tatami,yukimi}` outputs for old
   installed flakes. Move `installer/genkan/rust/system/` to `modules/`.
   (Same derivations, apart from the moved sources.)
4. Add `desktops.json` and generate Yukimi's choices and Genkan's tables from
   it. Changes: `/etc/yukimi/system.json` only.
5. Add the `sekka.*` namespace with alias modules, and update Genkan's generator
   and Yukimi's choice names.
6. Turn `specialArgs` into options with input defaults, and shrink the
   template. Existing installed flakes keep working.
7. Add `sekka.json` and `lib.settings`, plus Yukimi's one-time migration and the
   `highestPrio` export.
8. Add the workaround ledger and `checks.workarounds`. Send the module bugs
   upstream.

**Aizome:**

1. Add the test helper and test discovery. (Same derivations.)
2. Split the module. (Same system derivations.)
3. Add `systemSettings` (default off) and the time zone warning. Sekka turns
   it on in the same release.
4. Build the default package with the consumer's nixpkgs.
5. Move the patches into the ledger.
6. Share the Yukimi crates with Sekka instead of copying them.

DEVELOPERS.md's repository map and Aizome's README change with steps 2 and 3
(Sekka) and 3 (Aizome).

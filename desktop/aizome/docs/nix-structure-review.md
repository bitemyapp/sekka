# Sekka and Aizome: Nix structure and interactive configuration review

Review draft — 24 October 2026. No implementation changes proposed here have been applied.

## Recommendation

Adopt clearer module boundaries and explicit composition, using flake-parts first in Sekka. Keep Aizome usable as an independent NixOS module. Give interactive configuration a documented, typed ownership boundary and strengthen its file transaction handling before expanding the settings it can edit.

The critique identifies a real maintainability problem, but overstates two things: these projects already fix upstream problems, and their configuration editor already has substantial safeguards. Replacing more upstream modules is not itself a quality measure. The useful standard is whether a change has a clear owner, a small public interface, predictable composition, and evidence that it works.

The highest-value changes are:

1. Separate desktop integration from distribution policy, especially in Aizome's main module and Sekka's desktop-selection module.
2. Separate flake output assembly, reusable NixOS modules, installed-system composition, and live-media composition.
3. Extend Yukimi's existing managed-file approach to supported system choices, with explicit host targeting, effective-value reporting, and recoverable transactions.
4. Make patches, package provenance, and compatibility behavior discoverable and testable.

Flake-parts helps with the second item. It does not, by itself, establish the other three.

## Scope and evidence

I read the local working trees, including existing uncommitted work:

| Project | Location | HEAD at review |
| --- | --- | --- |
| Aizome | `/home/callen/work/aizome` | `bd73b8bdab71640bd5365499ba0602b3dea8b529` |
| Sekka | `/home/callen/work/sekka` | `99bda5dfaf20d4508dc9d1f321e233ff571c6d54` |

Both trees were already dirty. Sekka's uncommitted work includes Nix and Tatami changes, so observations describe the working tree, not only its HEAD. Sekka's lock pins Aizome to `5e69759f32c09bc1402b993e12c9219873201c27`, which differs from the independent Aizome checkout reviewed here. Do not interpret findings in the newer checkout as proof about the pinned installation image.

This was source inspection, not a build, runtime qualification, or full security audit. Existing tests were inspected but not run. Illustrative configurations below are proposed interfaces, not tested patches. The original draft was written outside both repositories without changing configuration, lock files, or existing work. This document is now versioned for review; none of its implementation proposals have been applied. Source links point to the reviewed HEAD revisions; existing uncommitted differences described above are not represented by those permalinks.

## How the projects are assembled today

Sekka's main path is:

```text
flake.nix
  ├─ external inputs + nix/components.nix
  ├─ nixosModules.default
  │    └─ components.calamares.nixosModules.default
  │         ├─ desktops
  │         │    ├─ installer/genkan/rust/system/
  │         │    ├─ Tatami module
  │         │    ├─ Yukimi module
  │         │    └─ Aizome input's module
  │         └─ application catalog module
  ├─ installer NixOS configuration → ISO
  └─ reference configurations → prebuilt installed closures

generated installed flake
  ├─ selected inputs follow Sekka's locked inputs
  └─ Determinate + sekka.nixosModules.default + configuration.nix
```

This is already organized to a degree. `nix/components.nix` deliberately adapts formerly separate components to their earlier flake-shaped interfaces. `nix/reference-systems.nix` evaluates installed reference systems. Hardware, tuning, cache, and networking work already have separate files. The problem is that the adapter has become a central architectural interface, while much of the distribution's reusable policy still lives under the installer's Rust directory.

Aizome is smaller: its 141-line flake exports packages, two aliases for its NixOS module, checks, and a development shell. Its principal complexity is the 493-line `nix/module.nix`, with a separate 135-line greeter module. Sekka's root flake is 626 lines; its desktop-selection module is 533 lines and Tatami's module is 542 lines. These counts locate concentrations of responsibility; they are not quality scores.

There is also a meaningful difference between defaults: importing Aizome's default module declares an opt-in desktop, while Sekka's default group includes desktop policy whose default selection is Aizome. New names and documentation should make this distinction explicit.

## Assessing the critique

| Critique | Assessment from the source | Useful response |
| --- | --- | --- |
| The configurations do not fix upstream issues | Incorrect as a blanket statement. There are package patches, service adaptations, and multi-desktop fixes. | Make fixes easy to find, scope, test, and retire. |
| The structure is difficult to reason about | Substantially fair at integration boundaries. Several files combine unrelated responsibilities and historical component names. | Define layers, stable exports, and policy ownership. |
| Flake-parts would improve composition | Reasonable, particularly for Sekka's output assembly. Aizome's root flake is already modest. | Introduce it incrementally without changing behavior at the same time. |
| Adding one imported feature should make configuration extensible | Useful for developers. Publishing a module and activating it are different operations. | Define feature exports and deliberate profile imports; keep GUI edits constrained. |
| Home modules should pair with system modules | Potentially useful if Home Manager becomes a supported integration. Neither root currently integrates it. | Make pairing explicit and optional rather than introducing a new dependency for every installation. |
| Hosts and external entities should be modeled | Useful where there is shared data with several consumers. These are primarily product/distribution repositories, not a personal fleet configuration. | Begin with desktop, application, and build-target metadata; add fleet entities when needed. |
| Interactive editing will inevitably fail | Unsupported as a conclusion. There are real safeguards and identifiable remaining risks. | Extend the safeguards and define the supported editing contract. |

Personal claims about an implementer's judgment do not resolve any of these design questions. Interfaces and regression evidence can.

## Concrete findings

### 1. Aizome's enable switch carries substantial system policy

[`nix/module.nix`](https://github.com/bitemyapp/aizome/blob/bd73b8bdab71640bd5365499ba0602b3dea8b529/nix/module.nix#L135) combines the session, portal patches, services, appearance, authorization, mutable time zone, hostnamed behavior, and wake-alarm capability.

Some integration belongs with a desktop. Other choices deserve independent policy controls:

- `mutableTimeZone` defaults to true, and the module sets `time.timeZone` to null at override priority 90. This deliberately wins over an ordinary administrator assignment. The existing [timezone policy test](https://github.com/bitemyapp/aizome/blob/bd73b8bdab71640bd5365499ba0602b3dea8b529/nix/timezone-policy-test.nix#L1) confirms that behavior.
- Hostnamed is forced to use writable `/etc/machine-info`.
- A polkit rule permits local active wheel users to perform enumerated settings actions and NetworkManager actions without another password prompt.
- `alarmsWakeSystem` adds `CAP_WAKE_ALARM` to the system's `user@` service configuration, a broader scope than a single Tokei process.
- Printing, discovery, Bluetooth, geolocation, networking, audio, and other services receive defaults together.

These are explicit product choices, not evidence of accidental implementation. They nevertheless make “enable this desktop” a large policy decision for an external NixOS consumer.

**Proposal:** separate session requirements, optional service integration, appearance defaults, and administrative policy. Let Sekka select its intended experience through a named profile. Preserve today's default entry point through a compatibility composition during migration; do not silently change installed behavior while moving files.

### 2. Distribution policy is coupled to installer internals

[`nix/components.nix`](https://github.com/bitemyapp/sekka/blob/99bda5dfaf20d4508dc9d1f321e233ff571c6d54/nix/components.nix#L40) exposes system behavior through a `calamares` component. [`system/desktops.nix`](https://github.com/bitemyapp/sekka/blob/99bda5dfaf20d4508dc9d1f321e233ff571c6d54/installer/genkan/rust/system/desktops.nix#L1) handles selection, login-manager arbitration, session filtering, keyrings, appearance, compatibility workarounds, and Yukimi's choice descriptions.

The desktop module even obtains the shared wallpaper package through `config.programs.tatami.wallpapers` when Tatami is not selected. This works, but makes a distribution resource appear to belong to a sibling desktop.

**Proposal:** move reusable system policy toward `nix/nixos/`; leave installer-specific orchestration with Genkan. Give shared assets a package or distribution option of their own. Retain existing `calamares.*` options and exported names until an explicit compatibility migration is ready. A bulk namespace rename would add risk without solving the ownership problem.

### 3. Importing a feature can have effects even when it is disabled

[Tatami imports the Hyprland-fixes module outside its enable guard](https://github.com/bitemyapp/sekka/blob/99bda5dfaf20d4508dc9d1f321e233ff571c6d54/desktop/tatami/module.nix#L328); that module unconditionally installs overlays. Consequently, registering the Tatami module affects the evaluated package set even with Tatami disabled. Lazy evaluation means this does not imply that Hyprland gets built or installed.

**Proposal:** distinguish feature registration from policy activation. Either apply the overlay under a documented Hyprland-stack policy, or gate it on the relevant feature. Preserve required fixes for both vanilla Hyprland and Tatami. Add a check for the intended disabled behavior rather than assuming all imports are inert.

### 4. Package provenance and system assembly require too much tracing

Sekka has intentionally separate system and application Nixpkgs inputs. It also has first-party packages and externally packaged catalog applications. This is useful and should remain explicit. Today, provenance travels through `externalInputs // components`, several `specialArgs`, catalog resolution, and repeated NixOS constructors.

Aizome exposes an overridable package option but defaults to its flake's package. Tatami mostly calls package definitions with the consumer's `pkgs`. Those are different extension behaviors: an external consumer's overlay does not automatically rewrite Aizome's preselected flake package.

**Proposal:** document which package set owns each dependency. Use package options for the components consumers reasonably replace. Share a small installed-system constructor among reference systems and fixtures, with hardware and identity supplied separately. Keep the live ISO as a distinct composition. Avoid replacing the adapter with another opaque all-purpose `inputs` object.

### 5. Interactive changes have a foundation, but their file boundary is incomplete

The existing design deserves credit:

- [`yukimi-config/edit.rs`](https://github.com/bitemyapp/sekka/blob/99bda5dfaf20d4508dc9d1f321e233ff571c6d54/apps/yukimi/crates/yukimi-config/src/edit.rs#L1) uses an rnix syntax tree, preserves surrounding text, reparses changes, and rejects unsupported expression shapes in relevant editing operations.
- [`packages.rs`](https://github.com/bitemyapp/sekka/blob/99bda5dfaf20d4508dc9d1f321e233ff571c6d54/apps/yukimi/crates/yukimi-config/src/packages.rs#L1) generates a dedicated `yukimi.nix` for added packages and programs.
- [`programs.yukimi.choices`](https://github.com/bitemyapp/sekka/blob/99bda5dfaf20d4508dc9d1f321e233ff571c6d54/apps/yukimi/nix/module.nix#L40) describes permitted settings and values; the privileged helper validates submitted choices.
- The [helper worker](https://github.com/bitemyapp/sekka/blob/99bda5dfaf20d4508dc9d1f321e233ff571c6d54/apps/yukimi/yukimi-helper/src/main.rs#L1176) takes a kernel file lock across Yukimi operations. It builds before activation, records results, and has separate switch/boot handling. [System-owned operations](https://github.com/bitemyapp/sekka/blob/99bda5dfaf20d4508dc9d1f321e233ff571c6d54/apps/yukimi/yukimi-helper/src/transaction.rs#L1) survive losing the GUI client.
- Generation rollback [preserves files changed by hand since the recorded configuration](https://github.com/bitemyapp/sekka/blob/99bda5dfaf20d4508dc9d1f321e233ff571c6d54/apps/yukimi/yukimi-helper/src/main.rs#L340). Activation-sensitive changes have a pre-switch check and completion planning.

The remaining concerns are concrete:

| Boundary | Current observation | Improvement |
| --- | --- | --- |
| Configuration identity | [`Setup::detect_in` and `find_main`](https://github.com/bitemyapp/sekka/blob/99bda5dfaf20d4508dc9d1f321e233ff571c6d54/apps/yukimi/crates/yukimi-system/src/setup.rs#L180) prefer `configuration.nix`, then search for references to `hardware-configuration.nix`, with hostname-based preference. | Publish the exact flake output selector and managed path. A flake's configuration key need not equal its hostname. |
| Editing scope | Desktop and catalog changes still edit the main configuration file. | Move adopted GUI-owned choices to an explicit managed input. Keep unsupported handwritten expressions outside automatic ownership. |
| File installation | [`write_file`](https://github.com/bitemyapp/sekka/blob/99bda5dfaf20d4508dc9d1f321e233ff571c6d54/apps/yukimi/yukimi-helper/src/main.rs#L259) uses in-place writes; edits happen before evaluation/build. | Build from an isolated candidate, then use durable replacement and a recovery journal. Atomic result receipts already exist; apply comparable care to configuration files. |
| Failed-build recovery | [`Backup::restore`](https://github.com/bitemyapp/sekka/blob/99bda5dfaf20d4508dc9d1f321e233ff571c6d54/apps/yukimi/yukimi-helper/src/main.rs#L234) unconditionally rewrites saved bytes and ignores restoration errors. The Yukimi lock does not stop a text editor. | Detect intervening changes and report restoration failures. Avoid overwriting a manual edit made during a long build. |
| Git source membership | New managed files use `git add --intent-to-add` so Git-backed flakes can see them. | Include index effects in recovery, or create the managed input during installation. Do not solve source inclusion by staging unrelated user work. |
| Desired versus effective settings | The running system exports choices, while syntax inspection sees local assignments. | Show requested, evaluated, running, and pending values separately where they differ; detect overrides rather than declaring success from a write alone. |

These observations establish design gaps, not demonstrated corruption incidents. The current process lock and generation-rollback comparison should be retained, not reimplemented under the assumption that they are missing.

### 6. Two Yukimi clients share an informal compatibility obligation

Aizome's [native Yukimi calls Sekka's installed helper](https://github.com/bitemyapp/aizome/blob/bd73b8bdab71640bd5365499ba0602b3dea8b529/apps/yukimi/src/ops.rs#L319). It also carries copies of `yukimi-config`, `yukimi-system`, and `yukimi-store`; [its documentation explicitly describes copying them](https://github.com/bitemyapp/aizome/blob/bd73b8bdab71640bd5365499ba0602b3dea8b529/docs/apps.md#L216). The config and system trees already differ, including lock-tree and update-display code.

Differences are not inherently defects, but both clients need an explicit helper protocol contract: version, capabilities, operation/result schema, and behavior when a capability is unavailable. Prefer a shared pinned library source or a documented synchronization process with compatibility fixtures. Do not create a Sekka ↔ Aizome flake dependency cycle merely to share Rust code; a neutral source package or a versioned protocol is sufficient.

## Proposed module structure

Use ordinary NixOS modules for runtime configuration and flake-parts modules for output assembly. Keep small related behavior together. The target is a comprehensible dependency graph, not a requirement that every option live in its own file.

```text
sekka/
  flake.nix                      # inputs, mkFlake, supported systems
  nix/
    flake/
      packages.nix               # packages/apps/dev shells
      modules.nix                # public modules and compatibility exports
      systems.nix                # live and reference output assembly
      checks.nix                 # checks, grouped by responsibility
    nixos/
      default.nix                # compatibility composition
      profiles/desktop.nix       # Sekka defaults and feature selection
      desktops/                  # selection, login manager, coexistence
      policy/                    # authorization, mutable settings, tuning
      applications.nix           # catalog integration
      yukimi.nix                 # management schema and integration
      compatibility.nix          # legacy option aliases
    overlays/                    # package replacements
    patches/                     # patch files and maintenance records
    lib/mk-installed-system.nix  # shared, deliberately small constructor
    data/                        # shared catalog/desktop metadata as needed
    tests/                       # evaluation, integration and VM checks
  desktop/tatami/                # implementation, assets, component packaging
  apps/                         # separate application workspaces
  installer/genkan/              # installer implementation and fixtures
  templates/                    # installed configuration contract

aizome/
  flake.nix
  nix/
    flake/                       # packages, modules, checks, dev shell
    nixos/
      default.nix                # existing behavior composed explicitly
      session.nix                # desktop entry, units, PAM, environment
      portals.nix                # routing, picker and coexistence
      greeter.nix                 # optional login screen
      services.nix               # optional service integrations
      appearance.nix             # fonts, dconf and MIME defaults
      settings-policy.nix        # mutable time zone and authorization
    packages/                    # existing package definitions, relocated later
    patches/
    tests/
```

This is a destination, not a first-PR checklist. Extract output assembly while keeping existing paths first. Move implementation files only after import and compatibility tests exist.

### Public composition and flake-parts

Keep `nixosModules.default`, `nixosModules.aizome`, Sekka's existing named modules, and current package/check names working. Add focused exports only where consumers need them. Reusable modules should take narrow package options or lexical dependencies; consumers should not have to reproduce Sekka's entire `specialArgs` arrangement. Flake-parts documents both [option-based bridging and `importApply`](https://flake.parts/define-module-in-separate-file).

For example, Aizome's initial conversion could retain its current module factory unchanged:

```nix
# flake.nix — outputs excerpt; flake-parts is a new pinned input
outputs = inputs:
  inputs.flake-parts.lib.mkFlake { inherit inputs; } {
    systems = [ "x86_64-linux" "aarch64-linux" ];
    imports = [
      ./nix/flake/packages.nix
      ./nix/flake/modules.nix
      ./nix/flake/checks.nix
      ./nix/flake/devshell.nix
    ];
  };

# nix/flake/modules.nix — separate file, retaining today's module path
{ self, flake-parts-lib, ... }:
{
  flake.nixosModules.default =
    flake-parts-lib.importApply ../module.nix { inherit self; };
  flake.nixosModules.aizome = self.nixosModules.default;
}
```

This changes assembly, not the desktop's policy. Subsequent work can split the factory behind those exports. `perSystem` handles system-specific packages and checks; published NixOS modules still operate in their consumer's module evaluation. The [flake-parts argument documentation](https://flake.parts/module-arguments.html) explains that distinction and the tools for bridging scopes.

For RGBCube's feature-oriented style, a single flake module can contribute a package, a NixOS module, optional Home Manager configuration, and checks. Explicit groups then select features. That can work well for shared features such as shell tools. It should not produce deeply nested closures that hide which module-system `config` a value belongs to.

Start with explicit imports. If later adopting automatic discovery, restrict it to a dedicated directory containing only flake modules and make its inclusion rules deterministic. Never recursively auto-import the entire Nix tree: it contains derivations, fixtures, and NixOS modules with different interfaces. Adding a published feature should not install it everywhere.

The optional [flake-parts `modules` facility](https://flake.parts/options/flake-parts-modules.html) can distinguish module classes. Existing public `nixosModules.*` exports can remain aliases. Neither that facility nor matching names automatically installs a Home Manager module into a system user's configuration.

### Home Manager and host entities

Do not make Home Manager or nix-darwin part of the required migration. Sekka currently targets x86_64 Linux; Aizome advertises x86_64 and aarch64 Linux outputs. A Darwin host constructor would introduce a new support commitment unrelated to repairing these interfaces.

If Home Manager support is requested, export optional user modules for stable declarative preferences. Explicitly select users and import the paired modules through the Home Manager integration. Standalone Home Manager users cannot be assumed to have `osConfig`. Avoid simultaneously managing a mutable application's entire settings file with Home Manager unless ownership and precedence are defined.

The critique's `flake.machines`, `self.people`, and `lib.systems.darwinSystem` illustrate a custom configuration framework, not facilities automatically supplied by flake-parts. The relevant idea is typed shared metadata. Sekka already has a JSON application catalog; extend that approach to desktop IDs, session names, permitted login managers, and reference-build targets where it removes real duplication.

For a future deployment repository, typed `machines` and `people` submodules could describe host platform, profiles, users, public SSH keys, DNS records, and builder capabilities. Derive consumers from that inventory instead of maintaining several copies. Keep private keys and credentials out of evaluated/store-resident metadata. Do not add a fleet inventory to these product repositories just to resemble a personal configuration.

## A clearer interactive configuration contract

The best part of the suggested “add one imported module” approach is the stable insertion point. Extend Yukimi's existing insertion point without granting the GUI ownership of arbitrary Nix source.

For newly installed systems, Genkan could create:

```text
/etc/nixos/
  flake.nix
  flake.lock
  configuration.nix
  hardware-configuration.nix
  managed/
    yukimi.nix                   # stable adapter, explicitly imported
    choices.json                 # supported GUI-owned desired state
```

Illustrative desired state:

```json
{
  "schemaVersion": 1,
  "desktop": {
    "sessions": ["aizome", "plasma"],
    "default": "aizome",
    "loginScreen": "auto"
  },
  "applications": ["firefox"],
  "packages": ["ripgrep"]
}
```

The adapter maps validated fields onto existing NixOS options. A tightly generated Nix file is also viable; JSON is preferable here because the editable vocabulary is data, not arbitrary expressions. Validate schema versions, field names, types, catalog IDs, package attribute paths, and cross-field requirements in the helper and adapter. Do not dynamically turn arbitrary JSON keys into privileged NixOS options.

Export an explicit management manifest from the installed system: supported protocol/schema versions, configuration directory, exact `nixosConfigurations` selector, managed-file location, supported operations, and applicable policy. This extends today's `/etc/yukimi/system.json` contract. Separate metadata must identify running and prepared generations; the manifest of the running generation is not automatically the schema of a candidate update.

Use ordinary definitions for GUI-owned selections. An equal-priority conflict should be explained, and a stronger administrative override should be reported as such. `mkForce` should never be a generic “make Apply work” mechanism. Resetting a selection should remove its managed definition and reveal the inherited default. List merging needs explicit semantics: removing a package from Yukimi's list cannot promise to remove a package installed by another module. Nix's [module merge and override rules](https://nixos.org/manual/nixos/stable/#sec-option-definitions) remain authoritative.

Existing systems need an adoption step that recognizes supported literal assignments, transfers only those choices, and removes the corresponding old definitions without disturbing comments or unrelated code. Keeping both definitions can merge lists or cause conflicts. Complex or ambiguous configurations remain inspectable and receive a precise explanation of what cannot be edited automatically. Maintain the existing supported legacy editing path during migration; requiring flake-parts in every user's installed flake is unnecessary.

### Transaction sequence

Retain the system-owned worker, polkit boundary, and existing operation lock. Strengthen the operation around them:

1. Read the trusted management manifest and record hashes of relevant source files, lock file, and Git state. Validate the requested operation against supported capabilities.
2. Produce an isolated candidate source containing the proposed choices and dependency changes. Preserve relative imports and the declared source closure; handle Git membership and external paths explicitly.
3. Evaluate assertions and effective values. Reject unsupported or overridden requests with an explanation. Build the exact candidate system before changing the user's source files.
4. Show the configuration/package diff and whether the result can apply live or must wait for boot. Bind the plan to the candidate source identity.
5. Before installation, verify the original files have not changed. Install managed files using durable temporary-file replacement and a transaction journal; a collection of renames is not a single atomic multi-file transaction.
6. Activate exactly the built store path using the existing session-safety decision. Record prepared, activated, failed, and recovery-required states distinctly. Recover interrupted file transactions on the next operation without overwriting newer manual work.

A staging design must account for Git-sensitive `self.rev`/dirty metadata and relative path inputs. Do not assume that copying a repository to `/tmp` evaluates identically. Record the candidate identity and retain it for reproducing the built generation. Explicitly document how supported layouts work before promising arbitrary-flake editing.

Separate two kinds of settings throughout this design:

| Setting | Appropriate authority |
| --- | --- |
| Installed desktops, packages, service enablement, authorization policy | Declarative system configuration and rebuild |
| Volume, joining a network, user appearance | Existing runtime service or user preference store |
| Time zone and machine display name | Explicitly chosen declarative or runtime ownership |

Aizome already uses an effective time-zone policy signal. Generalize that idea: when declarative policy owns a setting, Settings should explain why it is managed. When runtime changes are permitted, the policy should say so. Do not move every everyday desktop action into a Nix rebuild.

## Handling upstream fixes deliberately

Keep the existing fixes. They directly contradict the claim that upstream problems are ignored:

| Example | Current implementation/evidence | Structural improvement |
| --- | --- | --- |
| Aquamarine teardown crash | [Overlay](https://github.com/bitemyapp/sekka/blob/99bda5dfaf20d4508dc9d1f321e233ff571c6d54/nix/overlays/aquamarine.nix#L1), [negative-control and linkage check](https://github.com/bitemyapp/sekka/blob/99bda5dfaf20d4508dc9d1f321e233ff571c6d54/nix/aquamarine-test.nix#L1) | Preserve this pattern: reproducible failure, scoped patch, consumer verification, removal condition. |
| Hyprland screencopy session leaks | [Overlay](https://github.com/bitemyapp/sekka/blob/99bda5dfaf20d4508dc9d1f321e233ff571c6d54/nix/overlays/hyprland.nix#L1), screenshare VM check | Keep both standalone and installed compositions on the tested package set. |
| Wi-Fi scan state | [wpa_supplicant overlay](https://github.com/bitemyapp/sekka/blob/99bda5dfaf20d4508dc9d1f321e233ff571c6d54/installer/genkan/rust/system/wpa-supplicant/default.nix#L1) shared by installed and live systems | Relocate from installer internals to a named distribution fix. |
| Portal capture handling and recording buffers | [Aizome overrides](https://github.com/bitemyapp/aizome/blob/bd73b8bdab71640bd5365499ba0602b3dea8b529/nix/module.nix#L53) embedded in the desktop module | Give patched packages named definitions and explicit consumption points. |
| Login and multi-desktop integration | [Sekka desktop module](https://github.com/bitemyapp/sekka/blob/99bda5dfaf20d4508dc9d1f321e233ff571c6d54/installer/genkan/rust/system/desktops.nix#L43) and [Aizome greeter](https://github.com/bitemyapp/aizome/blob/bd73b8bdab71640bd5365499ba0602b3dea8b529/nix/greeter.nix#L1) | Separate session integration, login policy, and narrowly scoped compatibility adaptations. |

For each fix, record the affected versions, upstream reference/status, local rationale, consumers, regression check, and removal condition. Some records already exist in comments and DEVELOPERS.md; consolidate discoverability rather than inventing another disconnected registry.

Choose the smallest appropriate extension point: a public option for policy, a package override for a package defect, a service adaptation for a service defect. Replace an upstream NixOS module when its interface genuinely cannot express the required correction. NixOS supports [`disabledModules`](https://nixos.org/manual/nixos/stable/#sec-replace-modules); a replacement needs an explicit compatibility and maintenance obligation. Broad replacement without that evidence increases the surface Sekka must maintain.

Audit each `mkForce` and unusual priority by asking what conflict it resolves and how a consumer opts out. Some are justified, including current session coexistence behavior. Renaming or extracting those definitions does not automatically make them optional or safer.

## Migration order and review gates

| Stage | Work | Gate before proceeding |
| --- | --- | --- |
| 1. Define contracts | Document current exports, option ownership, package provenance, host targeting, and compatibility promises. | A reviewer can trace an installed desktop choice through one documented composition path. |
| 2. Extract output assembly | Introduce flake-parts in Sekka; split packages/modules/systems/checks while retaining implementation paths. Aizome can follow for consistency. | Existing outputs remain available; existing dependency revisions remain pinned; deliberate new flake-parts dependencies are isolated. Compare representative evaluated behavior and package paths. |
| 3. Split runtime modules | Extract Aizome policy/services/portals and Sekka desktop/login/coexistence concerns; preserve default compositions. | Existing options and supported combinations retain their behavior. Assert disabled-module effects and consumer override behavior. |
| 4. Harden Yukimi transactions | Add explicit target identity, conflict detection, durable source installation, recovery tests, and protocol negotiation. | Failure, cancellation, concurrent-edit, and interrupted-transaction cases have verified outcomes. |
| 5. Adopt managed choices | Add the stable adapter to new installations, then a narrow migration for recognized existing configurations. | Generated configurations, old configurations, GUI clients, helper, and rollback agree on ownership and schema. |
| 6. Consolidate shared metadata | Share desktop/application resolution and installed-system construction where duplication remains. Add optional Home Manager exports only with a defined consumer. | ISO references, installed systems, catalog resolution, and cache closures agree. |

Stages 2–3 can proceed independently of transaction design, but expanding interactive editing should wait for stages 4–5.

Pay particular attention to [`nix/installed-lock.nix`](https://github.com/bitemyapp/sekka/blob/99bda5dfaf20d4508dc9d1f321e233ff571c6d54/nix/installed-lock.nix#L1). It synthesizes an installed lock from Sekka's root lock and currently derives followed inputs by excluding two names. Adding flake-parts must not accidentally redefine which inputs installed systems expose. Review that logic together with the installed template and Yukimi's migration input list; prefer an explicit installed-input contract. Preserve separate base/application pins and the tested-source update model.

Use the existing tests as the starting point, not a parallel test architecture:

- Evaluation checks: exported module aliases, legacy options, default profiles, disabled features, package selection, administrator overrides, and valid/invalid desktop combinations.
- Existing integration checks: `hyprland-packages`, `aquamarine`, `screenshare-sessions`, `wpa-supplicant-scan`, `update-completion`, Aizome `timezone-policy`, greeter, settings, and polkit-recovery checks where their boundaries change.
- Generated-system checks: installed-template and desktop/application matrices, reference closures, and synthesized lock behavior. Inventory their actual cases; a matrix's name does not prove it covers newly added Aizome combinations.
- New transaction checks: two Yukimi operations, external edits during a build, build failure, restoration failure, interruption between file replacements, Git index recovery, unsupported schema, mismatched hostname/output key, and old/new client-helper combinations.

For a structural-only change, compare representative evaluated values and derivation identities rather than only successful parsing. Source relocation can legitimately change derivation identities; explain each such difference. Run relevant VM checks when integration changes, and qualify a real installation image separately before claiming the refactor is ready for release. Preserve narrow source filters and separate Cargo workspaces.

The first implementation should therefore be a compatibility-preserving extraction of Sekka's flake assembly, accompanied by a written module/ownership contract. The next substantive improvement is separating desktop policy and establishing Yukimi's managed configuration boundary. Those changes directly address the critique without turning a distribution refactor into a replacement framework for every possible Nix configuration.

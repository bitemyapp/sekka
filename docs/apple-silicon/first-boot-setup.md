# First-boot setup

Status: phase 2 of the [Apple Silicon installer plan](../apple-silicon-installer-plan.md#2-replace-the-developer-account-with-first-boot-setup),
implemented on `feat/apple-silicon-installer` on 6 November 2026. It replaces
the Mac image's shared developer account (`sekka` / `sekka`) with setup on
Aizome's login screen. What was tested, and what wasn't, is at the end.

## What the owner sees

The Mac boots to Aizome's login screen. Nobody has an account, so instead
of the people it shows setup, in the login screen's look (the wallpaper
under the kachi scrim, Archivo on Night's colours, sunk fields with the
amber focus rail, the mark at the foot, a dot for each page):

1. **Welcome.** Sekka's lockup, "Set up your Mac", one line on what follows.
2. **Language and region.** Language, keyboard and time zone, prefilled from
   macOS where the installer handed its choices over. The keyboard comes
   first, and the login screen's compositor switches to it as it's chosen
   (with a field to try it), so the password is typed on the layout it will
   be typed on at every login. The time zone is found by typing a city or a
   country.
3. **Make your account.** Full name, user name (suggested from the first
   name), password twice, computer name (macOS's, simplified: "Ada's
   MacBook Pro" → `adas-macbook-pro`). Each field says its rule before it's
   broken; the helper judges every field as it's typed, with Genkan's rules
   and in Genkan's words, and Continue says what's wrong and where.
4. **Choose your desktop.** Aizome (the default) or Tatami, both installed.
5. **Ready to set up.** The choices, then **Set Up**.
6. **Setting up your Mac…** The helper's steps as they happen.

Then the ordinary login screen, the new person picked with their desktop,
saying "Your account is ready. Log in with your new password." Tab reaches
every control, Return goes on, Escape goes back, F3 opens the power menu
throughout. Setup needs no network. A failure says what happened and what
to do; once the account exists, the person may log in while setup is retried.

## How it's built

| Piece | Where | Privilege |
| --- | --- | --- |
| Setup pages and the client side of the protocol | `desktop/aizome/crates/aizome-greeter/src/setup.rs`, `setup_view.rs` | the `greeter` user, as ever |
| `sekka-setup-helper` | `installer/genkan/first-boot/` (own Cargo workspace, GPL-3.0-or-later) | root, socket-activated |
| NixOS module (`calamares.firstBootSetup`) | `installer/genkan/first-boot/module.nix`, imported by `installer/genkan/rust/system/default.nix` | — |
| Login screen options (`programs.aizome.greeter.setup`, `keyboardFile`) | `desktop/aizome/nix/greeter.nix` | — |

### The helper and its protocol

systemd listens on `/run/sekka-setup.sock` (`sekka-setup.socket`: root,
group `sekka-setup`, mode 0660; only `greeter` is in the group) and starts
`sekka-setup-helper serve` with it. The helper also asks the kernel who
connected (`SO_PEERCRED`) and answers only the `greeter` account, telling
anyone else so and closing. Each connection has a thread; completions take
turns on a lock in the state directory. It exits a minute after its last
connection closes.

Requests are one JSON object a line, at most 16 KiB, parsed strictly
(unknown operations and fields refused, only objects): `status`, `check`
(the draft, and the password on the account page), `complete` (the choices
and the password), `retry` (finish what was begun). Answers are
`{"status":…}`, `{"checked":…}`, a sequence of `{"progress":…}` and then
`{"done":…}` or `{"failed":…}`, and `{"error":…}` before the connection
closes. Nothing in a request is a path or a command: the helper's
configuration (`/etc/sekka-setup/helper.json`, read only through a
root-owned chain, as Genkan reads its settings) names every program and
place it uses.

### Genkan's rules, reused

The helper depends on Genkan's backend (`installer/genkan/rust`, crate
`calamares-nixos`, without its `network` and `zonemap` features) and the
Mac installer's contract crate by path, with Genkan's exact dependency
versions. It judges choices with `RawRequest::parse`, the function Genkan's
own helper parses an installation with: a check puts each field into an
otherwise valid request, so the verdict is exactly Genkan's, and
completion parses the whole request. That covers the user name, host name,
full name and password rules, `LOCALES`, `KEYBOARDS`, `TimeZone::parse` and
`DesktopSelection`. Nothing in Genkan's crate was changed or made public.
What's added is what only the running system knows (a name a system account
already has) and what setup suggests and offers (labels, tzdata's
`zone.tab` with countries, the installed desktops).

The greeter can't link Genkan's crate: its exact pins (`anyhow =1.0.100`
and others) conflict with Aizome's lock. So it asks the helper (`check`)
instead of restating the rules. The rule text under each field is
presentation; the verdict is Genkan's.

The password is hashed as Genkan hashes it (SHA-512 crypt, 100 000 rounds,
`install.rs`). It arrives only over the socket, is read a byte at a time
into a zeroizing buffer (no intermediate buffer keeps a copy), lives in
zeroizing owners, reaches `chpasswd` on its input (never a command line),
and is never written in the clear. The greeter zeroes its copies after
sending.

### The handoff

`sekka/install.json` on the ESP (`calamares.firstBootSetup.handoff`,
`/boot/sekka/install.json` by default), `sekka.mac.handoff/1`, is parsed
with the contract crate. Each hint is used only if setup offers it:
locale and keyboard must be Genkan's, the time zone must pass
`TimeZone::parse`, the full name Genkan's rule; others are ignored. The
partitions are believed only when the kernel was booted with the same ones
(`sekka.root=PARTUUID=…`, `sekka.esp=PARTUUID=…`); then setup.nix names
them by GPT partition UUID, as the handoff contract asks. Without a handoff,
setup starts from the image's own settings.

### What setup changes, and how a rebuild converges

Setup builds no generation; the image's stays the running system. It
applies the choices as the running system's state, where NixOS permits it,
and writes the same choices as configuration:

| Choice | At once | In `/etc/nixos/setup.nix` |
| --- | --- | --- |
| Account | `useradd` (staged uid, group `users`, the groups below, home 0700, `users.defaultUserShell`), `chpasswd --encrypted` | `users.users.NAME` (`isNormalUser`, `uid`, `description`, `extraGroups = [ "networkmanager" "wheel" ]`) and `calamares.installUser = NAME`; no password (`mutableUsers` keeps `/etc/shadow`'s) |
| Groups | `networkmanager`, `wheel`, and `render` on the M3 Pro's GPU | `render` comes from `calamares.appleSilicon.graphicsUsers`, whose default is `installUser` |
| Host name | `hostnamectl --static --transient hostname` (hostnamed is pointed at `/etc/hostname` while `networking.hostName` is empty) | `networking.hostName` |
| Time zone | `timedatectl set-timezone` | none: Aizome's mutable time zone keeps `/etc/localtime` across rebuilds; setup.nix says so in a comment |
| Language | `localectl set-locale LANG=…` (`i18n.imperativeLocale`), and `systemctl set-environment` so the person's user manager starts with it before the next boot (systemd reads locale.conf only as it starts) | `i18n.defaultLocale` |
| Keyboard | `/etc/aizome/keyboard.toml` (the login screen's, `keyboardFile`) and, if not the image's layout, `~/.config/aizome/settings.toml` `[input.keyboard]` | `services.xserver.xkb.layout` |
| Desktop | the login screen remembers it for the person | `calamares.defaultDesktop` |
| Partitions | — | `fileSystems."/"` and `"/boot"` by PARTUUID, from a believed handoff |

setup.nix also sets `calamares.firstBootSetup.applied = true`, which
removes the helper, its socket, the greeter's group membership and the
login screen's setup and keyboard-file options from the built system.

The `configuration` check (below) proves the account's groups at run time
equal the groups the built configuration gives it, so the first rebuild
changes none of them, and that a whole Sekka installation evaluates with
setup.nix.

### The transaction and its reconciliation

Completion only goes forward, from a recorded intent, and every step looks
at what is already done:

1. **Check.** Genkan's parse, the system's account names, a free uid (the
   lowest from 1000 that no account has and `/var/lib/nixos/uid-map`
   hasn't given out), the groups present.
2. **Stage** in `/var/lib/sekka-setup` (0700 root): `credential` (the hash,
   0600, created exclusively), then `staged.json` (the choices, uid and
   groups: the intent), then `setup.nix` as it will be written.
3. **Account.** Made if absent; given the staged hash only while its shadow
   entry is unset or locked; missing groups added; home made if missing.
4. **Settings.** Host name, time zone, locale, keyboards, each only if it
   differs.
5. **Configuration.** `/etc/nixos/setup.nix`, atomically (0644).
6. **Verify.** Account, uid, groups, a usable password, host name, time
   zone, locale, setup.nix's bytes; and, with `verifyConfiguration` and an
   `/etc/nixos/flake.nix`, `nix eval --offline` of
   `nixosConfigurations."HOST".config.system.build.toplevel.drvPath`.
7. **Finish.** The marker `complete` (schema, user, uid, desktop, time, the
   evaluated system; no secrets), then the credential and staged files are
   deleted.

Files are written to a new name, synced and renamed, and their directory
synced. Interrupted anywhere:

| Stopped after | Next start |
| --- | --- |
| the credential, before the intent | no intent: the stray credential is deleted and setup starts over |
| the intent, before the account | `status` says interrupted; the login screen finishes it (`retry`); new choices may still replace it |
| the account, before its password | the staged hash is set (the account is setup's: its name and uid are staged, and no one had an account when it was) |
| the password or later | each later step is redone only where it isn't done |
| the marker | complete; leftover staged files are deleted |

A retry never gives an account a password it already has (if the person
changed it, it stays) and never changes a uid; an account of that name with
another uid is someone else's, and setup stops. Setup is offered only if
the module enables it, there is no marker, and either a staged setup exists
or no account with a login shell has a uid from 1000 to 59999 (Nix's build
users, from 30001, have none; the login screen lists people the same way).
An installation set up any
other way, like the original developer image, is never put back into setup,
and the helper refuses to stage there.

## For the image (phase 1)

`/etc/nixos/configuration.nix` should:

```nix
{ lib, ... }:
{
  imports = lib.optional (builtins.pathExists ./setup.nix) ./setup.nix;
  calamares.firstBootSetup.enable = true;
  calamares.desktops = [ "aizome" "tatami" ];
  calamares.defaultDesktop = "aizome";
  users.mutableUsers = true;
  # …the platform, boot, file systems and applications, as today.
}
```

and must not set, at normal priority, anything setup.nix sets:
`networking.hostName`, `time.timeZone`, `i18n.defaultLocale`,
`services.xserver.xkb.layout`, `calamares.installUser`,
`calamares.defaultDesktop` (setup.nix's would conflict; the module gives
each a default), nor any account with a password, `initialPassword` or
autologin. `/etc/nixos/flake.nix` must name its configuration by the host
name setup chooses, as Yukimi and `nixos-rebuild` look it up, for example
`nixosConfigurations = { ${c.config.networking.hostName or "sekka"} = c; }`
with `c = nixpkgs.lib.nixosSystem {…}` (an empty host name before setup
needs a fallback name). Its inputs must be in the image's store for the
offline check; otherwise turn `verifyConfiguration` off and say so.

## Options

| Option | Default | |
| --- | --- | --- |
| `calamares.firstBootSetup.enable` | false | setup on the login screen |
| `.applied` | false | set by setup.nix: setup is over |
| `.package` | `./package.nix` | the helper |
| `.handoff` | `<ESP>/sekka/install.json` | the installer's hints |
| `.device` | "Mac" with `appleSilicon`, else "computer" | "Set up your Mac" |
| `.desktops`, `.defaultDesktop` | `calamares.desktops`, `calamares.defaultDesktop` | what's offered |
| `.declaredGroups` | `networkmanager`, `wheel` | what setup.nix lists, as Genkan's configuration does |
| `.groups` | those, and `render` on the M3 Pro's GPU | what the account gets at once |
| `.verifyConfiguration` | true | evaluate `/etc/nixos/flake.nix` before finishing |

Enabled, the module also sets: `networking.hostName`, `time.timeZone`,
`i18n.defaultLocale` and `services.xserver.xkb.layout` as defaults;
`i18n.imperativeLocale` and every offered locale (`offered.json`, which
the helper's tests prove are Genkan's); hostnamed's static host name file;
root's password locked; `calamares.installUser = "greeter"` as a default
until setup.nix names the person (modules that give the installer's
account something then name an account that exists). It asserts
`users.mutableUsers`, Aizome's login screen, and no autologin.

## Tests

```sh
cd installer/genkan/first-boot && cargo test --locked          # with TZDIR=/etc/zoneinfo
cargo test --locked --manifest-path desktop/aizome/Cargo.toml -p aizome-greeter -p aizome-face
nix build .#legacyPackages.aarch64-linux.first-boot.helper     # the helper, with its tests
nix build .#legacyPackages.aarch64-linux.first-boot.configuration
nix build .#legacyPackages.aarch64-linux.first-boot.vm-test -L
```

- **Helper** (29): the protocol's bounds and strictness; Genkan's verdicts
  field by field and the whole parse; handoff hints kept, dropped and
  partition binding; setup.nix (quoting, no secret, the golden example);
  the transaction stopped at each of its 14 durable points and finished by
  a retry, with the same account, uid, password, settings and files as an
  uninterrupted run; each failing operation retried; a retry keeping a
  changed password and refusing an account with another uid; an
  installation with people never set up and untouched; SO_PEERCRED
  refusal, over a socket pair and a real socket; connections side by side;
  files' modes under the service's 0077 umask;
  no credential or staged file left, no password or hash in the marker or
  setup.nix.
- **Greeter**: the protocol's lines; zone matching; account readiness;
  `tests/setup.rs` drives every page with the keyboard against the
  helper's answers, and pictures each in `target/greeter-shots/setup-*`
  by the night and the day wallpaper (the login screen is always Night).
- **configuration**: a Sekka installed system (Genkan's modules, Aizome and
  Tatami, the Apple Silicon platform on aarch64) evaluated before and after
  `test-data/setup.nix`, which is byte for byte what the helper writes
  (a helper test holds them equal); groups, host name, locales, the
  socket, the absence of any password, and both systems' derivations.
- **vm-test**: an image with no account, setup on, the handoff in place,
  typed at through QEMU's keyboard; see the file's header for each step.
  On the J514S itself, KVM's in-kernel interrupt controller stalls any
  aarch64 guest at boot (a minimal NixOS test too, with a watchdog lockup
  on more than one processor); `userspaceIrqchip = true` has QEMU emulate a
  GICv2 instead. To run a checkout's login screen without rebuilding all of
  Aizome, pass `greeterPackage` (Aizome's `greeter` package) and an
  existing Aizome build as `aizomePackage`:

  ```sh
  nix build --impure -L --expr 'let f = builtins.getFlake (toString ./.); a = f.lib.components.aizome; in
    import ./installer/genkan/first-boot/vm-test.nix {
      pkgs = f.inputs.nixpkgs.legacyPackages.aarch64-linux; aizome = a;
      helper = f.legacyPackages.aarch64-linux.first-boot.helper;
      aizomePackage = builtins.storePath "/nix/store/…-aizome-0.1.0";
      greeterPackage = a.packages.aarch64-linux.greeter; userspaceIrqchip = true; }'
  ```

### Results, 6 November 2026, on the J514S (aarch64)

| Check | Result |
| --- | --- |
| Helper `cargo test` and its Nix build (check phase) | 29 passed |
| `aizome-greeter` tests (unit, `screens`, `setup`, `budget`) and `aizome-face` | 26 + 5 + 5 + 1, and 4, passed |
| `configuration`, aarch64 (with the Apple Silicon platform) | passed; both systems evaluated to derivations |
| `configuration`, x86_64 | its assertions pass and it instantiates; not built (no x86_64 builder) |
| The existing `installed-j514s` with the module imported, disabled | evaluates |
| `vm-test`, aarch64, `userspaceIrqchip` | all ten subtests passed (screenshots in its output) |

## Limits and what isn't verified

- The VM test was run on the J514S (aarch64, `userspaceIrqchip`) with the
  greeter built from this tree (Aizome's `greeter` package) and the
  compositor, shell and lock screen of the Aizome build installed on that
  Mac (`aizomePackage`), not one built from this tree: a full Aizome build
  was not run. It has not been run on x86_64.
- The VM test uses a stand-in session named Tatami (sway): Tatami's real
  session builds eight first-party packages. The greeter's choice of it is
  what's tested.
- In that VM, keys QEMU sends 10 ms apart were sometimes lost; the test
  types slowly and presses a page's key again only if the page didn't move.
  Nothing suggests a person's keyboard loses keys.
- Tatami's own keyboard comes from the configuration
  (`/etc/xdg/tatami/hypr/hyprland.lua`), so until the first rebuild a
  Tatami session types on the image's layout. Aizome, the login screen and
  the lock screen use the chosen one at once.
- The console keymap follows at the first rebuild (`console.useXkbConfig`).
- In the VM, `/etc/nixos/flake.nix` is a stand-in without inputs; the real
  installed flake's offline evaluation depends on the image seeding its
  inputs (phase 1). The `configuration` check evaluates a real system with
  setup.nix outside a VM.
- No physical J514S run: the handoff partitions, the ESP mount and the
  real flake were not exercised on a Mac.

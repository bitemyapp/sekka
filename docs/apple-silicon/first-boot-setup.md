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
to do; once the account has its password, the person may log in while setup
is retried, and the retry keeps whatever they change meanwhile.

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
`calamares-nixos`, without its `network` and `zonemap` features), the
Mac installer's contract crate and Yukimi's `yukimi-config` (the edit
Yukimi makes to a configuration file) by path, with Genkan's exact
dependency versions. It judges choices with `RawRequest::parse`, the function Genkan's
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
into a zeroizing buffer, lives in zeroizing owners, reaches `chpasswd` on
its input (never a command line), and is never written in the clear. The
buffers it passes through that the helper doesn't own are zeroed too: the
helper's allocator (`src/memory.rs`) zeroes every block it frees and moves
a block that grows, so serde_json's scratch buffer, which a password with
`"` or `\` is unescaped into, leaves no copy in freed memory. The helper is
undumpable (`PR_SET_DUMPABLE`) and its unit sets `LimitCORE=0`, so no core
holds its memory. The greeter writes each request into a buffer measured
to its size, never grown, and zeroes it and its other copies after sending.
The staged hash (`credential`) is deleted as soon as the account has its
password.

### The handoff

`sekka/install.json` on the ESP (`calamares.firstBootSetup.handoff`,
`/boot/sekka/install.json` by default), `sekka.mac.handoff/1`, is parsed
with the contract crate. Each hint is used only if setup offers it:
locale and keyboard must be Genkan's, the time zone must pass
`TimeZone::parse`, the full name Genkan's rule; others are ignored. The
handoff's partitions aren't setup's to record: `sekka-install-identity`
writes them to `/etc/nixos/identity.nix` at the first start
(`calamares.appleSilicon.identity.{root,esp}`, [image.md](image.md)), and
that module derives both the mounts and the kernel's `sekka.root=` and
`sekka.esp=` from them. setup.nix says nothing about the disk, so the two
can't disagree, and a setup.nix copied to another installation mounts
nothing of the one it came from. Without a handoff, setup starts from the
image's own settings.

### What setup changes, and how a rebuild converges

Setup builds no generation; the image's stays the running system. It
applies the choices as the running system's state, where NixOS permits it,
and writes the same choices as configuration, all in `setup.nix` but the
default desktop:

| Choice | At once | In the configuration (`/etc/nixos/setup.nix` unless named) |
| --- | --- | --- |
| Account | `useradd` (staged uid, group `users`, the groups below, home 0700, `users.defaultUserShell`), `chpasswd --encrypted` | `users.users.NAME` (`isNormalUser`, `uid`, `description`, `extraGroups = [ "networkmanager" "wheel" ]`, and `shell` by the path it was made with) and `calamares.installUser = NAME`; no password (`mutableUsers` keeps `/etc/shadow`'s) |
| Groups | `networkmanager`, `wheel`, and `render` on the M3 Pro's GPU | `render` comes from `calamares.appleSilicon.graphicsUsers`, whose default is `installUser` |
| Host name | `hostnamectl --static --transient hostname` (hostnamed is pointed at `/etc/hostname` while `networking.hostName` is empty) | `networking.hostName`: the name the computer has when setup.nix is written, the chosen one unless the owner renamed it meanwhile |
| Time zone | `timedatectl set-timezone` | none: Aizome's mutable time zone keeps `/etc/localtime` across rebuilds; setup.nix says so in a comment |
| Language | `localectl set-locale LANG=…` (`i18n.imperativeLocale`), and `systemctl set-environment` so the person's user manager starts with it before the next boot (systemd reads locale.conf only as it starts) | `i18n.defaultLocale`, which only seeds `/etc/locale.conf` should it be missing: the language stays the system's own, which Settings changes and rebuilds keep; setup.nix says so |
| Keyboard | `/etc/aizome/keyboard.toml` (the login screen's, `keyboardFile`) and, if not the image's layout, `~/.config/aizome/settings.toml` `[input.keyboard]` | `services.xserver.xkb.layout`, and so the console's keymap (`console.useXkbConfig`; the module keeps `/etc/vconsole.conf` NixOS's under `imperativeLocale`) |
| Desktop | the login screen remembers it for the person | `calamares.defaultDesktop` in `configuration.nix`, where the image names it as a plain string and Yukimi's Default desktop choice changes it: setup makes Yukimi's own edit (`yukimi_config::edit::set_string`, which changes only that string, or adds it beside the other `calamares` settings where there is none). Named some other way (by hand, say `lib.mkDefault`), it is left alone, as Yukimi leaves it, and the login screen still starts the person's desktop. Not in setup.nix, where a second definition would conflict with configuration.nix's or override it |
| Partitions | — | nothing: identity.nix's (`calamares.appleSilicon.identity`) |

setup.nix also sets `calamares.firstBootSetup.applied = true`, which
removes the helper, its socket, the greeter's group membership, the login
screen's setup and keyboard-file options and the module's assertions from
the built system, and removes (tmpfiles `r`) whatever an unfinished setup
left in `/var/lib/sekka-setup`, its completion record aside.

The `configuration` check (below) proves the account's groups and shell at
run time equal those the built configuration gives it, so the first rebuild
changes none of them, and that a whole Sekka installation evaluates with
what setup writes.

### What the first rebuild with setup.nix builds

setup.nix is written to change as little of the image's system as its
choices need, so the first rebuild builds small files from what the image
holds. Evaluated on 7 November 2026 for the product image (identity.nix
alone against identity.nix with setup.nix, `nix-store -qR` of both systems'
derivations), a setup choosing English (US), a US keyboard and Tatami
changes only: the host name's files (`etc-hostname`, `hosts`,
`string-hosts`, both `avahi-daemon.conf`, `initrd-hostname`, so the
initrd), `users-groups.json`, `tmpfiles.d` and `nixos-tmpfiles.d`,
`aizome-greeter.toml` (its setup section gone), `etc-yukimi-system.json`
(the default desktop), the units for avahi, hostnamed and tmpfiles, and
`etc`, `system-units`, `activate`, `dry-activate`, `boot.json` and the
system. Another keyboard adds `vconsole.conf`, `xkb-console-keymap`,
`xkb-validated`, `etc-X11-xorg.conf.d-00-keyboard.conf`,
`aizome-greeter-compositor.toml`, `etc-xdg-aizome-environment`,
`tatami-hyprland.lua`, `etc-xdg-tatami-update.json` and the vconsole
reload unit; another language adds `locale.conf`. Nothing else:

- the account's shell is named by its path, not its package, so the
  system's package set (`system-path`, and `dbus-1`, polkit, man and the
  rest built from it) is the image's;
- every offered locale is in `i18n.supportedLocales` in one fixed order,
  so glibc's locale archive is the image's whichever language is chosen.

Beyond plain text files, those need `json2x` (the two greeter TOML files),
`ckbcomp` (the console keymap) and `xkbvalidate` with `xkeyboard-config`
(another keyboard). Seeding them, and the inputs of the initrd, `/etc` and
the system, for an offline rebuild is the image's (`rebuildInputs`).

### The transaction and its reconciliation

Completion only goes forward, from a recorded intent, and every part is
recorded as it's done:

1. **Check.** Genkan's parse, the system's account names, a free uid (the
   lowest from 1000 that no account has and `/var/lib/nixos/uid-map`
   hasn't given out), the groups present.
2. **Stage** in `/var/lib/sekka-setup` (0700 root): `credential` (the hash,
   0600, created exclusively), then `staged.json` (`sekka.setup.staged/2`:
   the choices, uid, shell and groups, and the host name, time zone,
   locale, login screen keyboard, setup.nix and configuration.nix's default
   desktop the system had then: the intent), then `setup.nix` as it will
   be written.
3. **Account.** Made if absent; missing groups added and home made if
   missing; only then, the account whole, given the staged hash while its
   shadow entry is unset or locked. The credential is deleted once the
   password is recorded.
4. **Settings.** Host name, time zone, locale, keyboards.
5. **Configuration.** The default desktop in `/etc/nixos/configuration.nix`
   (its mode kept), then `/etc/nixos/setup.nix` (0644), each atomically;
   setup.nix names the host name the computer has by then.
6. **Verify.** The account and its uid, every part recorded; and, with
   `verifyConfiguration` and an `/etc/nixos/flake.nix`, `nix eval
   --offline` of
   `nixosConfigurations."HOST".config.system.build.toplevel.drvPath`.
7. **Finish.** The marker `complete` (schema, user, uid, desktop, time, the
   evaluated system; no secrets), then the staged files are deleted.

The groups and home, the password, the host name, the time zone, the
locale, the keyboards, the default desktop and setup.nix are each a part: done, checked (the
value read back), synced (`sync`, since systemd's services don't sync what
they write), and recorded in `staged.json`. A retry never does a recorded
part again. A part not yet done changes its setting only from the value
`staged.json` says it had: one that differs from both that and the choice
was changed since, by the person, who may log in once the account has its
password (a failure offers Log In then), and it stays. So a retry, which
the login screen sends by itself each time it starts while setup is
unfinished, never undoes what the owner changed after logging in: a time
zone, a language, a host name, a group they left, a locked password, a
default desktop chosen in Yukimi or an edited setup.nix. A host name they
gave the computer before setup.nix was written, whether before or after
the host name's part, is the one setup.nix names and the check evaluates,
so the configuration and the running system agree, and `nixos-rebuild`,
which looks the system up by the running name, finds it; a name the
configuration can't hold (not one DNS label, such as `babbage.local`)
gives way to the chosen one, which the first update sets.

Files are written to a new name, synced and renamed, and their directory
synced. Interrupted anywhere:

| Stopped after | Next start |
| --- | --- |
| the credential, before the intent | no intent: the stray credential is deleted and setup starts over |
| the intent, before the account | `status` says interrupted; the login screen finishes it (`retry`); new choices may still replace it |
| the account, before its password | the groups and home are completed and the staged hash is set (the account is setup's: its name and uid are staged, and no one had an account when it was); the failure doesn't offer Log In |
| a part's change, before its record | the part finds it done, and records it |
| a part's record | the part is left alone |
| the password's record | the credential is deleted |
| the marker | complete: the login screen asks no more, and the helper deletes the leftover staged files as it next starts |

A retry never gives an account a password it already has (if the person
changed it, it stays) and never changes a uid; an account of that name with
another uid is someone else's, and setup stops. Should the account be gone
after its password was recorded, setup doesn't make it again. Setup is offered only if
the module enables it, there is no marker, and either a staged setup exists
or no account with a login shell has a uid from 1000 to 59999 (Nix's build
users, from 30001, have none; the login screen lists people the same way).
An installation set up any
other way, like the original developer image, is never put back into setup,
and the helper refuses to stage there.

The first helper's `staged.json` (`sekka.setup.staged/1`, on a test
installation of an earlier image whose setup was interrupted and which was
then updated) recorded no shell, earlier settings or parts, and named the
partitions. This helper finishes it as that helper would have: the
partitions are ignored (identity.nix names them), the shell is the
configuration's, which that helper made the account with, and every part
not yet the choice is done from the settings as they are. A kind it
doesn't know is reported, never guessed at, and the helper's tidying as
it starts leaves it and its credential in place.

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
`services.xserver.xkb.layout`, `calamares.installUser` (setup.nix's would
conflict; the module gives each a default), nor any account with a
password, `initialPassword` or autologin. `calamares.defaultDesktop` is
the other way round: configuration.nix names it as a plain string
(`calamares.defaultDesktop = "aizome";`, or in a `calamares = { … }`
set), or not at all, and setup and Yukimi both change that string in
place; a `lib.mkDefault` there is a value neither can change.
`/etc/nixos/flake.nix` must name its configuration by the host
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
the helper's tests prove are Genkan's), with `i18n.supportedLocales` (a
default) listing them in one order whatever the default locale, so glibc's
locale archive doesn't change with the language chosen;
`/etc/vconsole.conf` as NixOS's console module writes it, which it leaves
out under `imperativeLocale`, so the console keymap stays the
configuration's (the keyboard isn't imperative; the language is);
hostnamed's static host name file; root's password locked;
`calamares.installUser = "greeter"` as a default until setup.nix names the
person (modules that give the installer's account something then name an
account that exists). Until setup is applied it asserts
`users.mutableUsers`, Aizome's login screen, and no autologin; once it is,
those are the owner's (another login screen from Yukimi, autologin,
declared users with a password; NixOS's own check still refuses a system
nobody can log in to). The helper's unit sets `LimitCORE=0`.

## Tests

```sh
cd installer/genkan/first-boot && cargo test --locked          # with TZDIR=/etc/zoneinfo
cargo test --locked --manifest-path desktop/aizome/Cargo.toml -p aizome-greeter -p aizome-face
nix build .#legacyPackages.aarch64-linux.first-boot.helper     # the helper, with its tests
nix build .#legacyPackages.aarch64-linux.first-boot.configuration
nix build .#legacyPackages.aarch64-linux.first-boot.vm-test -L
```

- **Helper** (33): the protocol's bounds and strictness; Genkan's verdicts
  field by field and the whole parse; handoff hints kept and dropped;
  setup.nix (quoting, no secret, nothing about the disk or the desktop,
  the golden example); the first helper's staged setup finished, an
  unknown kind refused and kept; the default desktop changed in
  configuration.nix by
  Yukimi's edit (the golden example, only that string changed, a later
  edit by Yukimi, one added where none is named, one named by hand left
  alone, no configuration.nix); the transaction stopped at each of its 22 durable points (each
  part's change and its record among them) and finished as the login
  screen would (a retry, or the helper tidying as it starts once the marker
  is written), with the same account, uid, password, settings and files as
  an uninterrupted run, each setting changed once, and the credential gone
  from the password's record on; each failing operation retried, Log In
  offered only once the password is set; a retry keeping a changed or
  locked password and refusing an account with another uid; a retry after
  the person logged in keeping their host name, time zone (even one put
  back as it was), language, groups, default desktop and setup.nix; a part
  not yet done never replacing a setting they changed meanwhile, nor their
  other edits to configuration.nix; setup.nix naming, and the check
  evaluating, the computer as they renamed it before setup.nix was
  written, or the chosen name for one the configuration can't hold; an interrupted status
  saying whether its account can log in; an installation with people never
  set up and untouched; SO_PEERCRED refusal, over a socket pair and a real
  socket; connections side by side; files' modes under the service's 0077
  umask; the allocator leaving no freed or outgrown block with anything in
  it; no credential or staged file left, no password or hash in the marker
  or setup.nix.
- **Greeter**: the protocol's lines (a request in a buffer of its own
  size); zone matching; account readiness; `tests/setup.rs` drives every
  page with the keyboard against the helper's answers, a made account
  without its password offering only Try Again, and pictures each in
  `target/greeter-shots/setup-*` by the night and the day wallpaper (the
  login screen is always Night).
- **configuration**: a Sekka installed system (Genkan's modules, Aizome and
  Tatami, the Apple Silicon platform on aarch64) evaluated before and after
  setup: `test-data/configuration.nix` (the image's default desktop, as it
  names it), then `configuration.after.nix` with `setup.nix`, each byte
  for byte what the helper writes (helper tests hold them equal); groups,
  the shell the helper uses, host name, locales, the default desktop
  chosen and defined by configuration.nix alone, the disk left alone, the system's
  packages (`system-path`) unchanged, the same glibc locales for every
  offered language, the console keymap following the keyboard, the socket
  and the module's assertions gone once applied (with another login
  screen, autologin and immutable users), the state directory's cleanup,
  the absence of any password, and both systems' derivations.
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

### Results, 7 November 2026, on the J514S (aarch64)

After the review fixes (setup.nix without the disk, per-part records and
the retry that keeps the person's changes, Log In only with a password,
the credential deleted early and leftovers tidied, the zeroing allocator,
the fixed locale order, the console keymap, assertions only while waiting):

| Check | Result |
| --- | --- |
| Helper `cargo test --locked` and its Nix build (check phase) | 33 passed |
| `aizome-greeter` tests (unit, `screens`, `setup`, `budget`) and `aizome-face` | 26 + 5 + 6 + 1, and 4, passed |
| `configuration`, aarch64 (with the Apple Silicon platform) | passed; both systems evaluated to derivations |
| The product image's system with identity.nix, with and without setup.nix (English/US, German/German, German/US) | evaluated; what setup.nix changes is listed above |
| `vm-test` | not run again: the J514S had too little free disk for the VM; its stand-in flake's reading of setup.nix was evaluated alone |

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
- The console keymap follows at the first rebuild (`console.useXkbConfig`,
  with `/etc/vconsole.conf` kept NixOS's); until then a text console types
  on the image's layout. Setup doesn't set it at once.
- The language is the system's own after setup, as the time zone is:
  editing `i18n.defaultLocale` in setup.nix changes nothing a rebuild
  applies (setup.nix says so); Settings changes it.
- A retry keeps what the person changed by comparing each setting with
  what it was when setup was staged; a setting someone changed and then
  changed back to exactly that, before its part was done, is taken as
  unchanged and set.
- In the VM, `/etc/nixos/flake.nix` is a stand-in without inputs; the real
  installed flake's offline evaluation depends on the image seeding its
  inputs (phase 1). The `configuration` check evaluates a real system with
  setup.nix outside a VM.
- No physical J514S run: the handoff partitions, the ESP mount and the
  real flake were not exercised on a Mac.

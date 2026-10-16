# Yukimi 雪見

*Snow-viewing for NixOS.*

On Sekka, **Updates → Update Sekka** fetches the newest distro and builds it
for your next restart in one action, with administrator authentication.
The same operation is available as `sekka-update` (`--switch` activates now).
Older installer configurations migrate automatically; external package pins
keep their separate update controls below. For the bootstrap command, see
the [Sekka update instructions](../../README.md#updating-an-existing-installation).

A yukimi-shōji is a paper screen with a pane of glass set low in it, so you
can sit in a warm room and watch the snow fall in the garden. Yukimi is that
window onto a NixOS system. You can see what is installed and why, find what
is available, and change it without opening a text editor.

Nix can do far more than any GUI can show. Yukimi doesn't try to show all of
it. It covers the everyday 80%: installing and removing software, updating,
going back to an earlier version and freeing disk space. Along the way it
explains what Nix is doing, so the system makes sense to you instead of being
something you edit and hope.

## What it shows

- **Overview.** The system at a glance: its NixOS version and kernel, how
  many packages it has, how fresh its Nixpkgs is, how much room the store
  takes, and what changed most recently.
- **Installed.** Everything installed, grouped by why: added with Yukimi;
  listed in your configuration by hand (`environment.systemPackages`, and
  your own `users.users.<you>.packages`); chosen from a catalog your system
  brings, such as its installer's; installed just for you with `nix profile`;
  and everything else your desktops and settings bring. Anything in the
  first four groups can be removed from here, including entries you wrote
  yourself: Yukimi takes just that entry out of its list.
- **Discover.** A catalog of well-known applications to start from, and a
  search of every top-level package in Nixpkgs, ranked by name first, then
  by description. You can try a package without installing it, install it
  just for you (no password), or install it for everyone. Packages that are
  broken or unfree are labelled before you pick them. A search shows the
  catalog's applications by that name above the packages. Applications that
  need a NixOS program as well as their package, such as Steam, can only be
  installed for everyone: without the 32-bit graphics drivers its program
  switches on, Steam's package quits before its window opens, so its search
  result installs the catalog's Steam too.
- **Updates.** Where the system comes from: each input of its flake, or each
  of its channels, the exact revision it is at, how old that is, and a link
  to see it. *Check for updates* asks every source for its newest version
  without changing anything, then marks each as up to date, newer, or *would
  go back* (when what `flake.nix` asks for now points somewhere older than
  your lock). Yukimi updates only what moves forward, now or at the next
  restart. The page checks by itself when you open it and the last check is
  more than ten minutes old. Each source from GitHub, GitLab, SourceHut or
  Git shows the branch it follows; choose another and Yukimi changes the
  input's address in `flake.nix` and moves to that branch's newest commit.
  So working on a project your system is built from comes down to: push to
  the branch your system follows, open Updates, press Update.
- **History.** Every generation you can go back to. Open one to see what
  changed from the one before: upgrades, downgrades, additions and removals,
  with sizes. Going back to one also brings back the choices it was built
  from, so your next change starts from there.
- **Storage.** The Nix store split into what the running system needs, older
  generations, user profiles, projects and dev shells, and garbage. It lists
  the heaviest packages and every garbage-collector root, and can clean up
  with one button.

## Any NixOS system

Yukimi finds the configuration the way `nixos-rebuild` does:

- **A flake** when `/etc/nixos/flake.nix` exists, following it if it links
  elsewhere (to a repository in your home, say), and building its
  `nixosConfigurations` entry named after this computer (or its only one).
  A flake that `/etc/nixos` doesn't lead to can be named with
  `programs.yukimi.configuration`.
- **Channels** otherwise: `/etc/nixos/configuration.nix`, built with the
  Nixpkgs of root's channels, as NixOS's own installer sets a system up.

What Yukimi installs for everyone goes in a file of its own, `yukimi.nix`,
next to your main configuration file, which imports it once: packages by
their attribute names in Nixpkgs, and NixOS programs (Steam, say) switched on
with `programs.<name>.enable`. The main file is `configuration.nix`, or in a
flake without one, the file that imports `hardware-configuration.nix` (in a
directory named after this computer, when there are several). In a Git
repository, Yukimi tells Git about `yukimi.nix`, without which the flake
wouldn't see it; files it creates belong to whoever owns the directory.

Every change is built first and switched to only if the build succeeds, so a
failed build leaves the running system as it was, and the files Yukimi
touched are put back. The helper keeps a copy of the configuration behind
each generation in `/var/lib/yukimi/configurations`, which is how going back
restores your choices too. A file you have changed by hand since then is
left as it is.

## Updates, quickly

Checking asks GitHub for the commit each branch is at, FlakeHub for its
newest release and the NixOS channels for theirs: a second or two, and
nothing is downloaded. Sources hosted elsewhere are checked by locking them
anew with Nix.

Updating gets the update ready as you, before asking for a password: Nix
locks the chosen inputs anew into a lock file of Yukimi's, and every source
that changed is copied into the store. The helper checks that this lock file
changes only those inputs, each still coming from where it came from, puts
it in place, and builds. Root's Nix keeps its downloads apart from each
user's, so without this it would download every new source again (in a test,
six minutes for one copy of Nixpkgs; with it, none).

## A window that never waits

Nothing slow happens on the interface thread. Yukimi reads the system in two
parts away from it: the configuration, generations and sources first, in a
few milliseconds, then the store, which can take seconds and fills in when
it is read. Searching runs in a thread of its own and shows only the answer
to the newest query; what a generation changed is worked out when it is
opened, in the background; long lists are built a batch at a time between
frames. A page is built again only when something it shows has changed, and
only once it is the page in view. The package index is made only once
Discover is opened, at the lowest priority, and the index of the Nixpkgs
before serves meanwhile. The falling snow pauses when the window is in the
background.

To see for yourself, start Yukimi with `YUKIMI_STALLS=50`: whenever the
interface thread is busy for longer than that many milliseconds, it says so
on standard error, with what it was doing.

## How it works

Everything is Rust, including the parts that read Nix's own data:

| Crate | What it does |
| --- | --- |
| `yukimi-store` | Reads the Nix store database (`/nix/var/nix/db/db.sqlite`) read-only. Handles store paths and name/version splitting, follows Nix's own `compareVersions`, builds the reference graph (closures, sizes, *why is this here*) and finds GC roots the same way the garbage collector does. |
| `yukimi-config` | Edits Nix files without disturbing them, using a lossless [rnix](https://github.com/nix-community/rnix-parser) syntax tree: string lists, imports, entries of package lists, and `flake.lock`, including checking that a new lock file changes only what it should. |
| `yukimi-system` | Finds how the system is configured, and reads its generations, profiles, channels and catalogs. Asks sources for their newest versions, builds the package index, diffs closures, and turns Nix's `internal-json` log into progress. |
| `yukimi` | The GTK 4 and libadwaita app. |
| `yukimi-helper` | The only part that runs as root, started through `pkexec`. It changes the configuration, updates inputs or channels, builds, switches, rolls back and collects garbage, with arguments it checks itself, on the configuration it finds itself. If anything fails, or you press Stop, it puts the configuration back as it was. Once the new system starts switching it finishes, so the switch is never cut off halfway. |

## Installing

Yukimi lives in [Sekka](../../README.md). With a flake, import Sekka's Yukimi module:

```nix
{
  inputs.sekka.url = "github:bitemyapp/sekka/nightly";
  inputs.sekka.inputs.nixpkgs.follows = "nixpkgs";
  outputs = { nixpkgs, sekka, ... }: {
    nixosConfigurations.host = nixpkgs.lib.nixosSystem {
      modules = [
        sekka.nixosModules.yukimi
        { programs.yukimi.enable = true; }
      ];
    };
  };
}
```

Without a flake, import the module from a tarball in `configuration.nix`:

```nix
{
  imports = [
    "${builtins.fetchTarball "https://github.com/bitemyapp/sekka/archive/main.tar.gz"}/apps/yukimi/nix/nixos.nix"
  ];
  programs.yukimi.enable = true;
}
```

The module installs the app with its polkit policy, so the password prompt
says what Yukimi is asking for, and NixOS's setuid `pkexec`, which recent
releases leave out unless asked. NixOS restarts polkit whenever the installed
packages change, so each change asks for the password again. It also writes
`/etc/yukimi/system.json`: the Nixpkgs the system is built from, whether it
accepts unfree packages, where its configuration is, and its catalogs, which
Yukimi would otherwise have to evaluate the configuration to learn.

| Option | What it does |
| --- | --- |
| `programs.yukimi.enable` | Installs Yukimi. |
| `programs.yukimi.package` | The Yukimi package. |
| `programs.yukimi.configuration` | The directory the configuration is in, when `/etc/nixos` doesn't lead to it. |
| `programs.yukimi.catalogs` | Catalogs of applications Discover offers besides Yukimi's own. |

### Catalogs for distributions and installers

A distribution, or an installer that offered applications, can give Discover
its own catalog: a JSON list in the format of
[Yukimi's own](crates/yukimi-system/data/applications.json) (`id`, `name`,
`description`, `category`, `packages`, and optionally `program`, `needs`,
`unfree` and `requires`; `needs` says what an application's program sets up
that its package can't). With a `setting`, a list of application ids in the
main configuration file installs them, as the installer's own option does;
its entries take the place of Yukimi's for the same applications. An entry
that takes the place of one of Yukimi's with a `program` keeps that program,
so Discover installs its package only through the entry, never just for one
person.

```nix
programs.yukimi.catalogs = [
  {
    file = ./applications.json;
    setting = "calamares.applications";
    title = "Apps chosen when installing";
  }
];
```

## Building

```sh
nix build github:bitemyapp/sekka#yukimi
nix develop   # from the Sekka repository root
cargo test --locked --manifest-path apps/yukimi/Cargo.toml
```

## Licence

MIT or Apache-2.0, at your option.

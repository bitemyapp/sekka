# Why Sekka systems compiled Codex instead of downloading it

October 12–13, 2026. Fixed on `main` in `24073d5`, `7945f89` and `8f23ed0`; the
first nightly under the fixed model is `f343f83`.

## Summary

Codex, one of the catalog's AI applications, comes from
[numtide/llm-agents.nix](https://github.com/numtide/llm-agents.nix). numtide's
binary cache, `https://cache.numtide.com`, holds the Codex builds their CI
makes. The Codex an installed Sekka system needed was the same store path
numtide had cached (`8h77jlpn…-codex-0.162.1`, signed `niks3.numtide.com-1`).
Sekka systems compiled it from source anyway, for three reasons that stacked:

1. **Sekka systems didn't list numtide's cache.** Only the nightly builder did,
   so CI never saw the problem.
2. **Adding the cache didn't help the update that added it.** Nix takes its
   caches from the running system, not from the one being built.
3. **Updates took numtide's newest commit.** Nothing guaranteed that commit's
   Codex was cached yet.

On ganymede, an update that moved `ai-apps` started compiling Codex, the disk
filled, and Yukimi restored the configuration. Nothing in the update path
watched free space.

## How Codex reaches a system

The installed flake passes `ai-apps.packages.x86_64-linux` to the catalog as
`aiPackages`. `ai-apps` keeps its own Nixpkgs: it does not follow Sekka's, as
`flake.lock` shows. Codex's derivation therefore depends only on the
llm-agents.nix revision, so a system at a given revision asks for exactly the
store path numtide built and cached at it.

Nix downloads a path only from a substituter listed in its configuration, with
a signature from a key it trusts. Otherwise it builds the path, here a large
Rust build. (We didn't measure its size; it was enough, together with
everything else on that disk, to fill it.)

## Cause 1: the cache was configured on the builder only

Before `24073d5`, `installer/genkan/rust/system/binary-cache.nix` added only
`https://cache.sekka.computer`. cache.nixos.org doesn't build llm-agents.nix,
so on the live media and on installed systems, any Codex not already in the
local store was compiled.

numtide's cache had been added to wx-workstation's own `nix.conf` for the
nightly builder, so CI downloaded Codex and nothing failed there.
`checks.binary-cache` only confirmed that the flake, the live media and
installed systems named the same Sekka cache.

**Fix (`24073d5`).** `binary-cache.nix` adds `https://cache.numtide.com` with
numtide's published key (`calamares.binaryCache.numtide.enable`, on by
default). The flake's `nixConfig` advertises it too, and `checks.binary-cache`
requires both caches everywhere. numtide's `nix-cache-info` declares priority
30, which would make Nix ask numtide before cache.nixos.org (40) for every
path. `?priority=55` makes Nix ask it last instead.

## Cause 2: a new cache only serves the update after the one that adds it

`nix-daemon` reads its substituters from the running system's configuration.
Under Determinate that's `/etc/nix/nix.custom.conf`, which the NixOS module
generates. The update that adds numtide's cache builds the new system with the
old configuration. The new one takes effect only once the new system is
activated, when NixOS restarts `nix-daemon` (it is in the daemon's
`restartTriggers`). So the first update after `24073d5` would still compile
Codex, and the only workaround was two updates: Sekka alone first, then
everything else.

The flake's `nixConfig` doesn't close the gap. The daemon accepts additional
substituters only from trusted users, and on Sekka `trusted-users` is `root`
alone. For anyone else, Nix ignores the cache and prints `ignoring untrusted
substituter 'https://cache.numtide.com?priority=55', you are not a trusted
user`. We saw exactly that when running
`nix run --accept-flake-config github:bitemyapp/sekka/nightly#update`.
Yukimi's helper runs as root, but it passed no caches.

**Fix (`7945f89`).** Before building, the helper evaluates the new system's
`config.nix.settings` (substituters and trusted public keys, including the
`extra-` lists). It passes them to `nix build` as `--option
extra-substituters` and `--option extra-trusted-public-keys`; coming from
root, the daemon accepts them (`caches` in
`apps/yukimi/yukimi-helper/src/main.rs`). A cache an update adds now serves
that same update.

## Cause 3: updates took numtide's newest commit, untested

Yukimi's Update moved every input to its own newest revision, `ai-apps`
included. numtide's `main` moves many times a day through automatically merged
update pull requests. Whether Codex is cached by the time a user's update locks
a commit is up to numtide's CI. When we looked, Codex was cached at each of
their six newest commits, the newest only 15 minutes old. But we didn't confirm
that their merges wait for the cache push, so nothing guaranteed it. The same
applied to every other input: Nixpkgs, `applications`, oh-my-pi and
Determinate.

**Fix (`8f23ed0`).** Installed flakes follow Sekka's inputs
(`inputs.ai-apps.follows = "sekka/ai-apps"`, and likewise for the others), as
the template, the installed lock and both lock validators now require. Yukimi
converts existing flakes and leaves alone any input pointed at an address of
the user's own. Updating Sekka moves everything together to what Sekka's lock
pins, and that lock reaches users only through the `nightly` branch. The
nightly workflow moves `nightly` only after these steps pass:

1. updating the inputs, `nix flake check`, building the ISO;
2. `nix run .#push-cache`, which now publishes an installed system for every
   desktop plus one with every free catalog application;
3. `scripts/check_cache_coverage.py .#installed-systems`, which follows what a
   system with an empty store would fetch or build, and fails if anything would
   be compiled.

## Why the disk filled

On ganymede, the Nix store, Nix's build directories (`/nix/var/nix/builds`) and
`/tmp` share one 437 GB filesystem. At the time, Claude Code's temporary
folders also held 64 GB there. Nothing limited how much space a build could
take: the installed systems set no `min-free`, and the updater didn't check
free space.

**Fix (`8f23ed0`).** The helper won't start building with less than 10 GB free
on `/nix` or `/tmp`. It stops a build that leaves less than 4 GB, and puts the
configuration back. Installed systems also have Nix collect garbage below 5 GB
free, up to 20 GB (`installer/genkan/rust/system/store-space.nix`).

## Getting existing systems through

Systems already installed run the Yukimi from before any of these fixes, and
their next Update is the one that installs the new Yukimi. To keep that update
from compiling Codex:

- **Mirrored Codex into Sekka's cache.** We copied numtide's Codex
  (`8h77jlpn…`) into cache.sekka.computer, which those systems already trust,
  after checking numtide's signature and the archive's signed `NarHash`. We
  then signed it with `cache.sekka.computer-1`. Codex is Apache-2.0, within
  the cache's free-software rule.
- **Waited out Nix's memory of the miss.** Nix remembers a cache miss for
  `narinfo-cache-negative-ttl` (3600 s by default). ganymede's daemon had
  looked for Codex before it was mirrored, so updating had to wait until
  01:39, when that ran out.
- **Kept the mirror current.** A job re-copied numtide's newest Codex every 30
  minutes. It ended early, when the Claude session running it ended, but
  numtide's newest Codex at 13:05 on October 13 was still `8h77jlpn…`.
- **Published the old updater's Sekka packages.** We published the 10 Sekka
  packages the old updater's choice of Nixpkgs needs, for the other laptop.

ganymede then updated at 03:36 on October 13 without compiling anything; the
new generation is waiting for a restart.

## Keeping it fixed

- **Keep `ai-apps` on its own Nixpkgs.** If Sekka made it follow Sekka's
  Nixpkgs, every Codex would be a store path numtide never built.
- **Give every cache to the systems, not just the builder.** A new upstream
  source whose packages aren't on cache.nixos.org needs its cache in
  `binary-cache.nix`, or its packages published to Sekka's cache. Otherwise
  the coverage check fails the nightly.
- **Keep the coverage check's cache list in sync.**
  `scripts/check_cache_coverage.py` lists the caches by hand: cache.nixos.org,
  Sekka's, numtide's and `install.determinate.systems`, which Determinate's
  own `nix.conf` adds. If `binary-cache.nix` changes, change the script to
  match.
- **Know what still builds.** NVIDIA's driver is built for each new kernel,
  and unfree applications are repackaged from their vendors' downloads. A
  system with an input pointed at an address of its own may also compile
  Sekka's packages.

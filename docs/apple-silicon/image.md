# Sekka's Mac image

Developer notes for phase 1 of the [Apple Silicon installer plan](../apple-silicon-installer-plan.md):
the image Sekka's macOS installer will write to a 14-inch MacBook Pro with
an M3 Pro (J514S), the installed configuration it ships, the per-install
identity handoff, boot-chain updates, the release manifest, and the VM
harness that tests the packaged image. Sources are in `nix/apple-silicon/`.

## Product and developer images

Both images are built from one installed-system configuration:

| | Product image | Developer image |
| --- | --- | --- |
| Flake output | `packages.aarch64-linux.mac-release-j514s` | `packages.aarch64-linux.mac-installer-j514s` |
| Configuration | `product/configuration.nix` | the same file, plus `developer/developer.nix` |
| Installed by | Sekka's macOS installer (not yet built) | `install-sekka.sh`: the unmodified Asahi installer v0.9.2 |
| Accounts | none; root has no password (`!`) | `sekka`, password `sekka` |
| Root and ESP | the GPT partition UUIDs the installer hands off | label `sekka-root`, FAT volume ID `5ECC-A5E1` |
| Root label, root UUID, ESP serial | `sekka-system`, `5ecca000-…-000000000002`, a new one per installation | `sekka-root`, `5ecca000-…-000000000001`, `5ECC-A5E1` |
| Release set | payload, `installer_data.json`, `manifest.json`, `nixos/`, `checks/` | payload, `installer_data.json`, `install-sekka.sh` |

The product image has no usable password and no account: root's password
is locked (`!`), and `calamares.firstBootSetup.enable = true` (the block
marked `FIRST-BOOT SETUP` in `product/configuration.nix`) makes Aizome's
login screen ask for the owner's account and choices
([first-boot setup](first-boot-setup.md)). Setup writes
`/etc/nixos/setup.nix`, which the shipped configuration imports when it
exists, as it does `identity.nix`. The configuration therefore leaves
everything setup chooses to setup's module and `setup.nix`: the host name,
time zone, locale, keyboard layout and installing user are not set. The
default desktop is a plain `calamares.defaultDesktop = "aizome";`, which
Yukimi's Default desktop choice rewrites in place (`edit::set_string`
accepts only a string literal); setup is to change the same line rather
than set it in `setup.nix`, where a second definition of the enum would
conflict. The optional files are imported by `imports.nix`, so that
`configuration.nix`'s own `imports` is a plain list, which Yukimi appends
`./yukimi.nix` to when it installs for everyone. `mac-image-tests` runs
Yukimi's own `packages::import_into` and `edit::set_string` on the shipped
file (`yukimi-edits.rs`), with the former forms as the failing control. The
VM harness, which doesn't drive
setup's screens, logs in through systemd's debug shell, enabled only on the
VM's copy of the boot entries.

The developer image turns setup off (`mkForce false` in `developer.nix`)
and sets what setup would otherwise have chosen: host name `sekka`, UTC,
`en_US.UTF-8`, `us`, and the `sekka` account. Setup's module also refuses
to start on a system that already has an account, so no update puts it into
setup.

The flake names its system after the host name the configuration sets,
as Yukimi and `nixos-rebuild` look it up. Before setup that is empty, and
the system is named both `sekka` and `nixos`, which is what the running
system then reports (systemd's fallback), so `nixos-rebuild` without a name
finds it; the VM test's rollback failed with `sekka` alone:

```nix
nixosConfigurations =
  (c: if c.config.networking.hostName == ""
    then { sekka = c; nixos = c; }
    else { ${c.config.networking.hostName} = c; })
  (nixpkgs.lib.nixosSystem { … });
```

Other outputs:

| Output | What it is |
| --- | --- |
| `mac-release-j514s-system` | the product image's NixOS system (toplevel) |
| `mac-release-j514s-esp` | its ESP tree, as packaged |
| `mac-release-j514s-root` | its `root.img` |
| `mac-release-j514s-etc-nixos` | its `/etc/nixos` |
| `mac-release-j514s-image-check` | the pre-packaging check report |
| `mac-release-j514s-rebuild-inputs` | the first rebuilds' build inputs the root image keeps |
| `image-j514s-system`, `image-j514s-esp` | the developer image's system and ESP |
| `mac-image-tests` | script tests and the installed-flake check (below) |
| `checks.x86_64-linux.apple-installed-template` | the installed-flake check with the PC fixture |

## The shipped /etc/nixos

The image ships `flake.nix`, its exact `flake.lock`, `configuration.nix` and
`imports.nix` (and `developer.nix` in the developer image) in `/etc/nixos`.
The image's system is evaluated by calling that same `flake.nix`'s `outputs`
with the inputs its lock pins, so evaluating `/etc/nixos` with the shipped
lock gives the image's toplevel exactly, and a no-change rebuild is a no-op.

- **flake.nix** is Genkan's own template, `templates/flake.nix.in`, rendered
  by `installed-flake.nix` for `aarch64-linux` and the branch
  `fix/apple-silicon-aizome` (Macs follow the Apple integration branch until
  the platform is on main), with the header comment adjusted and the
  system named by its host name (above). Every substitution must match an
  exact number of times. The rendered file is tracked as
  `product/flake.nix` (and `developer/flake.nix`); evaluation fails if
  either differs from the rendering, or if `developer/configuration.nix`
  or `developer/imports.nix` differs from the product's.
- **configuration.nix** and **imports.nix**, which imports `identity.nix`,
  `setup.nix` and `developer.nix` when they exist.
- **flake.lock** comes from `nix/installed-lock.nix`, now parameterized by
  `ref`; its default (`nightly`) gives the PC lock byte for byte. A clean
  build pins Sekka as `github:bitemyapp/sekka` at the image's own commit
  and NAR hash; a dirty development build pins the exact store source
  (`type = "path"`). The root names only Sekka and the six inputs that
  follow it (`nixpkgs`, `determinate`, `fh`, `applications`, `ai-apps`,
  `omp`); implementation inputs (Superluminal, Tracy, Uta) stay beneath
  Sekka.
- **Offline evaluation.** The root image also holds every source that
  evaluating `/etc/nixos` reads (`seedInputs` in `image.nix`): Sekka's own
  source, the root Nixpkgs (also `applications`), Determinate (its Nixpkgs,
  Nix, Nix's Nixpkgs and bdwgc, and the aarch64-linux daemon) and fh (its
  Nixpkgs, crane and fenix). That set was found by evaluating the shipped
  flake in an empty chroot store with no network: everything else in the
  lock was never fetched. Nix finds a locked input in the store by its NAR
  hash, so the GitHub-locked Sekka resolves offline to the seeded source.
  No closure the system keeps holds most of them, so they have a
  garbage-collection root of their own
  (`/nix/var/nix/gcroots/sekka-offline-sources`): a collection (Yukimi's
  Storage page, or Nix freeing space for a build) keeps them. Once the lock
  has moved, both of the image's roots can be removed.

`mac-image-tests` runs the rendered template and generated lock through
Genkan's real `Template::parse` (the VM fixture's `validate-template`, as
`template-lock` does for PCs), checks that the parser's output for a host
name is byte for byte `installed-flake.nix`'s rendering for it (from which
the shipped flake differs only in naming the system by its configuration),
and checks that a lock with an extra root input is refused.
`checks.x86_64-linux.apple-installed-template` is the same check with the
PC fixture.

- **First rebuilds offline.** The first generation that names the
  installation's partitions (`identity.nix`) changes the initrd, `/etc`,
  the console keymap and the system itself, whose build inputs (the
  initrd's modules, units and splash, the keymap's tools, the generic
  builder) are not in a system's closure. The owner's choices in
  `setup.nix` change more: the account's shell joins the system's
  packages, and the host name, language, keyboard and default desktop reach
  `/etc` (the login screen's configuration, the keyboard, Tatami's
  settings), units and checks (the keyboard's validation). `image.nix`
  evaluates the system as setup leaves it for two owners (`afterSetup`:
  `de_DE`/`de` with Aizome, `ja_JP`/`jp` with Tatami), takes every package
  list, `/etc` file, unit, check and manual index whose derivation differs
  from the image's, and keeps those derivations' inputs (`setupInputs`):
  the tools that write `/etc`'s files (json2x, xsltproc), texinfo, the
  keyboard validator, and packages a package list names but its build
  doesn't keep. The root image holds all of these under a
  garbage-collection root (`/nix/var/nix/gcroots/sekka-offline-rebuild`).
- **One glibc-locales for every language.** glibc's locales are compiled
  from `i18n.supportedLocales` in its order, and NixOS puts the default
  locale third, so six of setup's eight languages gave a glibc-locales that
  no cache has and the image can't build. With first-boot setup,
  `image-system.nix` lists every offered language first, so every choice
  gives the image's own glibc-locales; a language added later is appended.

## Per-install identity

Every image is the same bytes, so the root filesystem's label and UUID are
the same on every Mac the product image is installed on. Only the GPT
partition UUIDs the installer creates identify an installation.

The product image shares no name with the developer image
(`identities.json`, checked by `image.nix` and `manifest.py`): a developer
install, like the original J514S installation, mounts `/` by the label
`sekka-root` and `/boot` by the FAT serial `5ECC-A5E1`, so a product image
with the same names, installed beside it, would make udev's
`/dev/disk/by-label` and `by-uuid` links name either installation's
partitions. Its root is `sekka-system` with another filesystem UUID, and its
`installer_data.json` gives the ESP no `volume_id`, so Asahi's
`partition_disk` lets `newfs_msdos` choose a serial for each installation.

The handoff (`installer/genkan/macos/crates/contract/src/handoff.rs`) is:

1. The installer partitions the disk, writes the ESP from the payload and
   `root.img`, then replaces `@SEKKA_ROOT_PARTUUID@` and
   `@SEKKA_ESP_PARTUUID@` in `loader/entries/nixos-generation-1.conf`
   (each appears exactly once, on the `options` line) with the new
   partitions' GPT UUIDs, lowercase. The entry then carries
   `sekka.root=PARTUUID=<uuid> sekka.esp=PARTUUID=<uuid>`.
2. It writes `sekka/install.json` on the ESP (`sekka.mac.handoff/1`,
   canonical JSON): installation UUID, release, source revision, the two
   partition UUIDs, a timestamp and setup hints.

Sekka's initrd is systemd-based (the scripted stage 1 is gone from this
Nixpkgs). With `calamares.appleSilicon.identity.handoff` (`identity.nix`):

- **udev** (`61-sekka-identity.rules`, initrd and booted system): for each
  partition, `identity/role.sh` compares udev's `ID_PART_ENTRY_UUID` with
  the command line and links a match as `/dev/disk/by-sekka/root` or
  `/dev/disk/by-sekka/esp`.
- **Mounts.** Generation 1 mounts `/dev/disk/by-sekka/root` (ext4,
  `autoResize`) in the initrd and `/dev/disk/by-sekka/esp` at `/boot`.
  Nothing is selected by label or filesystem UUID.
- **`sekka-identity.service`** (initrd, required by `sysroot.mount`,
  `identity/check.sh`) stops the boot with a message on the console and
  in the journal, instead of systemd's 90-second device wait, when:
  - an argument is missing, repeated, malformed (`PARTUUID=` and a
    lowercase UUID) or still a placeholder: the installer never wrote
    this installation's identity;
  - both name the same partition;
  - on a Mac, `/proc/device-tree/chosen/asahi,efi-system-partition`, the
    ESP m1n1 was started from, isn't `sekka.esp` (the entry belongs to
    another installation). On this J514S the property reads
    `910de5ca-7390-4716-9a54-8028916e4d78`, its ESP's GPT UUID; QEMU's
    device tree has no such property, so the VM can't exercise this;
  - a named partition doesn't appear within `identity.timeout` seconds
    (20), listing the partitions present;
  - two partitions carry the same GPT UUID (a cloned disk);
  - udev's `by-sekka` link is missing or points elsewhere.

  It counts once, as soon as the partitions appear. A block-level copy of
  this disk on USB (dd, or a tool that keeps the GPT's UUIDs) usually
  appears later, after USB comes up, and can then take the `by-partuuid`
  and `by-sekka` links, so `/boot` could be mounted from it. Every boot
  loader installation therefore counts again before NixOS's installer
  writes anything (`install_identity.py unique`, from `module.nix`'s
  `installBootLoader`) and refuses while another partition carries the
  GPT UUID of the root or ESP mounted then.
- **`sekka-install-identity.service`** (first start, before the display
  manager; only while the configuration names no identity) checks
  `/boot/sekka/install.json` (canonical form, schema, fields, size), checks
  it names the partitions on the command line, checks those are the
  partitions actually mounted at `/` and `/boot` (`blkid -p` reads the GPT
  entry), and writes `/etc/nixos/identity.nix` atomically. If the file
  exists, it only checks that it names the same partitions, and refuses
  (without changing it) if not.

`identity.nix` sets `calamares.appleSilicon.identity.{root,esp}`. Every
later generation then mounts `/dev/disk/by-partuuid/<uuid>` and carries
`sekka.root=`/`sekka.esp=` with the real UUIDs in its own kernel parameters,
so the initrd check (including the device-tree cross-check) runs for every
generation and no entry depends on the installer's edit. First-boot
setup's `setup.nix` also names `/` and `/boot` by the same partition UUIDs
(`mkForce`), from the handoff it believes only when the command line
matches it; both come from the same checked arguments, so they agree.

NixOS's systemd-boot installer rewrites every generation's entry from its
bootspec at each installation, generation 1's included, which brings the
placeholders back. After it runs, `install_identity.py fill-entries`
(`extraInstallCommands`) fills them in again: with `identity.nix`'s UUIDs,
which must equal the running system's; without it, from the running
system's command line, which its initrd checked; a rescue system without
`sekka.*` arguments uses `identity.nix` alone. Entry file names are
content hashes of what NixOS would write, so a filled entry keeps its name
and NixOS leaves it alone afterwards. If this installation is interrupted
between NixOS's rewrite and the fill, generation 1's entry stops in its
initrd with the placeholder message; later generations are unaffected.

The developer image disables the handoff (`developer.nix`) and mounts by
label and volume ID, as before.

## Boot-chain updates: m1n1/boot.bin

The Asahi stub's m1n1 loads `m1n1/boot.bin` from the ESP before anything
of Sekka's runs. It holds m1n1, every Apple device tree, U-Boot and m1n1's
settings, and all generations share it. NixOS's `extraFiles` deleted it and
then copied the new one in place on every switch, so an interruption could
leave no second stage, and the replaced copy was lost. `boot-bin/boot_bin.py`
now replaces it on every boot loader installation (`module.nix`, for every
`calamares.appleSilicon` system).

**What FAT leaves after a power cut.** A FAT file is a directory entry
(name, first cluster, size), a chain in the FAT and data clusters, and
Linux's vfat (`fs/fat`, read in the linux-aurora 7.1.12 source) writes
them back in no order it promises:

- `rename(2)` over an existing name gives the renamed file the target's
  entry but writes the new first cluster and size there only when the
  file's inode is written back; it marks the source entry deleted in the
  directory's buffer; the replaced file's clusters are freed when its
  inode is evicted, which is at once unless something holds it open. A
  directory `fsync` writes the directory's changed sectors and the FAT,
  not the renamed inode, so the old procedure (`os.replace`, then a
  directory `fsync`) could leave `boot.bin` naming freed clusters for the
  30–45 s until writeback, with `boot.bin.previous` in the same state.
- A file's `fsync` writes its entry only if its inode is still dirty;
  background writeback may already have copied it into a directory sector
  it hasn't written. Only `syncfs(2)` certainly writes every entry.
- The ESP is marked dirty in its boot sector while Linux has it mounted,
  and stays so after a power cut. m1n1's FAT reader (rust-fatfs 0.4)
  ignores the flag, and reads the first FAT.

So a name is replaced (`commit`) by holding the replaced file open (its
clusters stay allocated), renaming, `fsync`ing the directory (the source
entry's removal reaches the disk first), `syncfs` (the target entry names
the new clusters, however its inode was written), and only then closing
the replaced file, which frees its clusters. At every instant the disk's
entry names either the old clusters, allocated and intact, or the new
ones, written, read back and flushed before the rename.

The one state it can't exclude: a writeback of the renamed inode during the
rename, or in the microseconds before the directory's `fsync`, then a power
cut, leaves the target and its staged copy (`boot.bin` and `boot.bin.new`)
naming the same new clusters. `boot.bin` is complete in it. Deleting or
truncating either would free the other's clusters, so `boot_bin.py` checks
every leftover with `FIBMAP` before removing it and refuses, changing
nothing, when it shares the target's first block. `fsck.vfat`, which
systemd runs on `/boot` at every start, resolves shared clusters by
truncating one of the two names, so the message says to copy `boot.bin`
somewhere safe first.

`boot-bin/test_boot_bin.py` runs the real procedure against `FatModel`, a
model of these semantics (directory sectors, inode writeback at any moment,
the FAT, eviction, `fsync` and `syncfs` as the kernel source has them). After
every system call it takes every state a power cut can leave, with 128, 16, 2
or 1 entries per directory sector, and checks that `boot.bin` is certainly
one complete copy, that once the new copy can be active the replaced one is
certainly `boot.bin.previous`, and that only a file and its staged copy ever
share clusters. It mounts every such state again and checks that the next
run finishes with the new copy active and the old one kept, or refuses with
`boot.bin` complete. The old procedure fails the same checks.

On every boot loader installation:

1. An interrupted replacement recorded as pending is finished from its
   staged copies (`*.new`), or, when one is missing, given up with the
   files as they are. Leftovers are removed (after the `FIBMAP` check).
2. Nothing is written when the active copy already has the new digest, or
   when it is a held copy (below) and the new one is the copy it replaced.
3. The ESP must have room for every copy written plus 4 MiB; when it
   hasn't and `boot.bin` is missing, the message says so.
4. The copy being replaced becomes `boot.bin.previous` only if it has
   booted, or if there is no previous copy: `sekka-boot-bin.service` records
   at every start that the copy at `m1n1/boot.bin` booted
   (`boot_bin.py mark-booted`). Two installations without a restart in
   between keep the copy that booted.
5. The new previous copy and the new copy are written as `*.new`, flushed,
   read back from the disk (their pages dropped first) and checked;
   `sekka/boot-bin.json` records the replacement as pending (which
   `boot.bin.previous` and `boot.bin` it ends with); then
   `boot.bin.previous` is committed, then `boot.bin`, each as above, and
   `boot.bin` is read back once more.
6. `sekka/boot-bin.json` (`sekka.mac.boot-bin/1`, canonical JSON) records
   the current and previous copies (digest, size, store source, system,
   time, whether it booted), any hold, and the last ten installations. A
   record that isn't what `boot_bin.py` writes (unreadable, another shape)
   is rebuilt from the files.

The image's ESP carries the record for the copy it installs, not yet
booted; the first start marks it.

Generations and boot.bin:

- Choosing an older generation in systemd-boot does not change
  `boot.bin`: that generation's kernel starts with the newest-installed
  m1n1, device trees and U-Boot. Device trees normally stay compatible with
  older kernels, but nothing guarantees it.
- Making an older generation the default (`nixos-rebuild boot --rollback`,
  or its `bin/switch-to-configuration boot`) runs its own installation,
  which puts its `boot.bin` back by the same procedure, keeping the newer
  copy as the previous one if it booted. A generation from before
  `boot_bin.py` (the original J514S installation's) instead copies its
  `boot.bin` in place with `extraFiles`, keeps no previous copy and leaves
  the record stale; the next installation of a newer generation notices the
  active copy and keeps it.
- `boot_bin.py restore-previous --esp /boot` swaps the two copies,
  refusing if `boot.bin.previous` doesn't match the record. It stages both,
  records the swap, commits the restored copy as `boot.bin` first (so it is
  never only in memory), then the replaced one as `boot.bin.previous`; a
  retry, or the next installation, finishes a recorded swap rather than
  starting another, and a second `restore-previous` doesn't undo it.
- **A restored copy is held.** The default generation still names the
  copy that was restored away, and its next installation (a no-change
  `nixos-rebuild`, a Yukimi update) would put it back. The record keeps a
  hold: installations leave the held copy and warn, until a different copy
  comes (a later update) or `boot_bin.py release --esp /boot`. To stay with
  the older system as a whole, make an older generation the default
  (`nixos-rebuild boot --rollback`).
- `boot_bin.py status --esp /boot` compares the files with the record.

Recovery when a new `boot.bin` doesn't start Linux at all: start macOS
(hold the power button, choose macOS), mount the Sekka ESP
(`diskutil list`, then `diskutil mount diskNsM`), check
`shasum -a 256 m1n1/boot.bin.previous` against `previous.digest` in
`sekka/boot-bin.json`, and copy it over `m1n1/boot.bin`. At the next start,
`sekka-boot-bin.service` finds `boot.bin.previous`'s copy active instead of
the record's current one and holds it, as `restore-previous` does.

**Leaving extraFiles.** NixOS's systemd-boot builder deletes every file its
markers under `EFI/nixos/.extra-files` list, and syncs that, before
`extraInstallCommands` run. A system that installed `boot.bin` with
`extraFiles` (the original J514S installation) still has the marker. So
`module.nix` wraps `system.build.installBootLoader`: `boot_bin.py
forget-extra-files` removes `boot.bin`'s marker, then NixOS's installer
runs, leaving `boot.bin` in place for `boot_bin.py install` to replace with
the steps above, the running copy kept as the previous one. It runs at every
installation, so a rollback to an `extraFiles` generation, which writes the
marker again, doesn't reopen the gap at the next forward one.
## Manifest (sekka.mac.manifest/1)

`release/manifest.py` writes `manifest.json` in the contract's canonical
JSON (keys in byte order, no whitespace, integers only), after checking the
payload against what it was built from. It has two readers, and each part
has the shape its reader takes:

- the engine (`installer/genkan/macos/engine`, `assets.load_manifest`)
  reads `release`, `source_revision`, `root_image` and `esp`, refusing any
  other key in `esp`, and allows the other top-level fields;
- a catalog's `Release` (`catalog.rs`) takes `source` (`SourceIdentity`),
  `image` (`ImageFacts`) and `allocation` (`AllocationPolicy`) as they are.

`mac-image-tests` (`release/test_manifest.py`) runs `manifest.py` on a small
release and feeds its output to the engine's own `load_manifest`,
`check_payload`, `load_template` and `esp_inventory_digest`, checks the
catalog objects' keys and the canonical bytes, and, as its failing control,
has the engine refuse an `esp` with an extra key, as `manifest.py` once
wrote it. Fields:

| Field | Meaning |
| --- | --- |
| `schema` | `sekka.mac.manifest/1` |
| `release` | `sekka-j514s-<short revision>` |
| `source_revision` | the 40-digit commit (a dirty build's base commit), as the plan's `source_revision` |
| `root_image` | `path` (`root.img`), `size` and `digest`; the plan's `root_image` is this digest |
| `esp.boot_entry` | the entry the installer fills in |
| `esp.files` | every file in the payload's `esp/`: `path`, `size`, `digest`, sorted by path bytes |
| `source.repository`, `.revision`, `.lock` | `bitemyapp/sekka`, the commit, and the SHA-256 of the shipped `flake.lock` |
| `image.root_image` | `root.img`'s `size` and `digest` |
| `image.esp_inventory` | the engine's `esp_inventory_digest` of `esp.files`: the canonical digest of `{"domain":"sekka.mac.esp-inventory/1","files":[…]}` |
| `image.esp_bytes` | the ESP partition size, 1 GiB: `plan.rs`'s `ESP_BYTES` |
| `allocation.minimum`, `.recommended` | bytes for the whole Sekka allocation, 1 MiB-aligned |
| `profile`, `variant` | `j514s`; `product` or `developer` |
| `build.clean` | false for a dirty development build, which must not be published |
| `build.nar_hash` | Sekka's source NAR hash, as the lock pins it |
| `build.update_ref` | the branch the installed flake follows |
| `build.lock_bytes` | the size of the shipped `flake.lock` |
| `system.toplevel`, `.label`, `.kernel_params` | the NixOS system and its bootspec label and parameters (with the placeholders) |
| `system.closure_paths`, `.closure_nar_bytes` | the root image's registered closure |
| `system.seeded_sources` | the source paths among them, for offline evaluation |
| `root.label`, `.filesystem_uuid`, `.registered_paths` | the ext4 image's fixed identity, shared by every installation of the image (`identities.json`) |
| `esp_layout.volume_id` | the FAT volume ID in `installer_data.json`: null for the product image, whose ESPs get their own |
| `esp_layout.used_bytes`, `.generation_bytes`, `.generations` | ESP use, one generation's kernel and initrd, and the retained-generation limit the capacity check used |
| `esp_layout.boot_files` | roles: `m1n1-stage2`, `removable-loader`, `systemd-boot`, `kernel`, `initrd`, `loader-config`, `entry`, `boot-bin-record` |
| `esp_layout.placeholders` | the placeholders in the boot entry |
| `payload` | the zip's `name`, `size` and `digest` |
| `installer_data` | `size` and `digest` of `installer_data.json` |
| `allocation_basis` | `stub` (2.5 GB aligned, `plan.rs`), `esp`, `root_image`, `generation_reserve`, `data_room` |
| `handoff` | `schema`, `path` and `kernel_arguments` of the handoff, or null for the developer image |

`minimum = stub + esp + root_image + generation_reserve`, where
`generation_reserve = max(root_image, 10 GB)`: room for one more full
generation, and never less than the free space Yukimi needs to start an
update. `recommended = minimum + generation_reserve + 64 GiB`. The stub and
ESP must be `plan.rs`'s `STUB_BYTES` and `ESP_BYTES`, the partitions
`derive_plan` creates, and `Release::check` refuses a catalog whose minimum
leaves the root partition smaller than the root image or whose ESP is
another size. Digests are `sha256:` and 64 lowercase hex digits, as `Digest`
writes them. The engine's later writes to the ESP (`sekka/install.json`, the
filled entry, `vendorfw/`, Asahi's own files) are outside the inventory;
`manifest.py` refuses ESP files that collide with them or with each other
ignoring case.

Nothing yet compares a catalog's `image.esp_inventory` or `esp_bytes` with
the manifest: the plan carries the manifest's digest and the root image's,
not these, and the engine checks the ESP's bytes against the manifest's
`esp.files`. A catalog made from the manifest carries the engine's form.

### Integrity checks before packaging

`release/check_image.py` reads the ext4 image with e2fsprogs (`debugfs`,
`dumpe2fs`; nothing is mounted) and fails the build unless:

- `/nix-path-registration` is byte for byte the closure's registration, and
  the paths it registers are exactly the closure;
- `/nix/store` holds exactly those paths;
- each path, dumped out of the image, has the NAR hash and size it is
  registered with (a NAR serializer in the script, checked against Nix's
  own registrations);
- `/etc/nixos` holds exactly the shipped files, unchanged;
- the label and filesystem UUID are the declared ones.

The payload zip is built only after that check. `manifest.py` then checks
that the zip holds exactly `esp/` (each file's size and digest equal to the
inventory) and `root.img` (equal to the checked image), stored rather than
compressed; that `installer_data.json` describes it; that the ESP has room
for the retained generations, the firmware archive and two copies of
`boot.bin`; and that the numbers satisfy `Release::check`.

## VM harness

```sh
nix build .#packages.aarch64-linux.mac-release-j514s --out-link result-mac-release
nix/apple-silicon/boot-image-in-vm.sh result-mac-release /path/to/scratch
```

It runs on an Apple Silicon Mac (Apple's hypervisor) or aarch64 Linux with
KVM, using only disk image files in the scratch directory, and needs about
the root image's size there. On Linux the guest gets a 32-bit physical
address space (`highmem=off`, so 3 GiB): on this 16 KiB-page host, with
memory fragmented by the page cache, KVM couldn't allocate the stage-2 table
of the default 40-bit space (`KVM_CREATE_VM: Cannot allocate memory`).
Given the developer bundle (`mac-installer-j514s`) it lays the disk out as
the unmodified Asahi installer does and checks two boots as before; the first
has a stand-in for a product install attached (a FAT ESP with another serial
and an ext4 root with the product image's label and UUID), and both require
`/dev/disk/by-label/sekka-root` and `/dev/disk/by-uuid/5ECC-A5E1` to be the
only such names and to lead to the developer disk's own root and ESP, which
are the ones mounted. Given the product release set it:

1. verifies the payload and `root.img` against the manifest;
2. lays out a base disk as Sekka's installer would: random GPT UUIDs, a
   stand-in stub partition, the ESP from the payload with the placeholders
   filled and `sekka/install.json` written, a stand-in firmware archive, and
   `root.img` with 4 GiB to grow into. The only VM-only change is
   `console=hvc0 systemd.debug_shell=/dev/hvc1` on each entry it starts;
3. boots an entry whose placeholders were never filled and one naming a
   missing partition, each of which must stop in the initrd with its
   message;
4. makes disks A and B as qcow2 overlays of the base; B's overlay holds
   only its two GPT copies with new UUIDs (`qemu-img map` confirms), so its
   partitions are byte-identical to A's: the same labels, filesystem UUIDs,
   boot entry and handoff; and disk D, a stand-in for a developer install
   (a FAT ESP and an empty ext4 root with the developer image's serial,
   label and UUID);
5. boots disks A, B and D together (first boot), then A alone three times
   (ordinary boot with a controlled update, the updated generation with a
   rollback, the image's generation again), running `vm/guest-checks.sh`
   from a small test disk through systemd's debug shell on `hvc1`. The
   guest has no network device, so everything it builds or evaluates is
   offline.

The checks in each boot (`vm/guest-checks.sh`):

| Boot | Checks |
| --- | --- |
| first (A, B and D) | systemd `running`, no failed units, graphical target and display manager active; the command line names A; `/` and `/boot` are A's partitions by GPT UUID; `by-sekka` links are A's; B's partitions present and unmounted; the product label and UUID on A and B only; the developer label, UUID and serial on D's partitions only, which their `by-label` and `by-uuid` links name, and D unmounted; store registered (path count equals the registration); root grew; stand-in firmware on its tmpfs; `identity.nix` written; shipped lock unchanged; the shipped flake evaluates offline to the image's toplevel; `nixos-rebuild build` offline gives it; the flake with `identity.nix` evaluates; no account with a login shell, root locked; setup waiting, the flake with setup's sample `setup.nix` evaluating under its host name, and, with that `setup.nix` changed to `de_DE` and `de`, `nixos-rebuild build` offline (when setup is in the image); a garbage collection would keep every seeded source; `boot.bin` matches its record and `sekka-boot-bin.service` marked it booted |
| second (A) | registration not repeated; `identity.nix` checked, not rewritten; `boot_bin.py` replace, no-op and restore on the VM's FAT filesystem (a scratch directory, not the real `boot.bin`); an update (a module imported from `configuration.nix`) built offline with `nixos-rebuild boot`; every entry filled, naming A, firmware first |
| updated (A) | generation 2 booted; the update applied; mounts by partition UUID; no first-start identity service; `nixos-rebuild boot --rollback` |
| rolled-back (A) | generation 1 booted again, the update gone, `by-sekka` mounts |

What it cannot show: m1n1, U-Boot, the T6030 display and GPU handoff,
Apple's firmware and drivers, speakersafetyd, the device-tree ESP
cross-check, the Asahi installer, Sekka's macOS installer and Recovery.
The firmware archive is a stand-in; the handoff and the installer's
partitioning are the harness's own.

## Evidence

Recorded on 6 and 7 November 2026 on the J514S (Mac15,6, linux-aurora
7.1.12-aurora20, Determinate Nix 3.24.0), the reference MacBook Pro running
Sekka from its internal disk. Every VM disk was a file in a scratch
directory; nothing touched the computer's own disks. Logs are kept outside
the repository with the run; this records what they show.

**Candidate.** `mac-release-j514s` built from `c22238c5` (phase 1 with
first-boot setup cherry-picked from `feat/apple-silicon-installer`
`1f913f65`), clean:

| Artifact | Size | SHA-256 |
| --- | --- | --- |
| `os/sekka-j514s-c22238c.zip` | 14,285,936,786 | `b3ee1ca9…1161c0` |
| `root.img` (in the zip) | 14,206,087,168 | `3ba0585d…b1912f` |
| `manifest.json` | 4,365 | `34854486…7a01b3` |
| `installer_data.json` | 470 | `a5e36410…b9c00f` |
| `nixos/flake.lock` | 17,459 | `40b35c13…194cc4` |

Toplevel `sb4wxrxr…-nixos-system-unnamed-26.11.20261103.1d06008`, 1,938
registered paths, 11.09 GB of NAR data, of which 13 are seeded sources.
Minimum allocation 31,985,762,304 bytes, recommended 114,911,346,688.

**Build-time checks** (all passed):

- `check_image.py`: registration byte-identical, 1,938 paths registered,
  in the closure and in `/nix/store`, each with its NAR hash and size read
  from the image's bytes (66 s on an earlier candidate of the same size);
  `/etc/nixos` as shipped; label and UUID.
- `manifest.py`: the zip against the ESP inventory and root image,
  `installer_data.json`, ESP room for 5 generations of 67.4 MB, the
  catalog's rules. The contract crate's own canonicalizer
  (`examples/canonicalize`) gives the manifest's exact bytes and digest, and
  accepts a handoff written as the harness writes it.
- `mac-image-tests`: Genkan's `Template::parse` accepts the Mac template
  and lock and refuses one with an extra root input; the parser's output
  equals the renderer's; 22 Python unit tests (boot.bin interruptions at
  each step, bad read-back, full ESP, damaged record, restore; handoff
  parsing and refusals, identity record, entry filling); the initrd check
  and udev helper against fake roots (20 cases).
- The shipped flake and lock, evaluated on the host, gave the image's own
  toplevel derivation; evaluated in an empty chroot store with no network
  and only the seeded sources, it did too (this found the seed set:
  without bdwgc, crane, fenix or Determinate's Nixpkgs it fails).
- The x86_64 derivations of `genkan`, `tools`, `yukimi`, `aizome`,
  `installed-systems` and `t6030-backlight` are unchanged from the base
  commit, and the PC installed lock is byte for byte the same.
- Read-only on this J514S: `identity/check.sh` read the real
  `/proc/device-tree/chosen/asahi,efi-system-partition`
  (`910de5ca-…-8028916e4d78`, its ESP), accepted this installation's own
  identity and refused an entry naming another ESP.

**VM run** (`boot-image-in-vm.sh`, KVM, 4 vCPUs, 3 GiB, no network):

| Step | Result |
| --- | --- |
| payload and `root.img` against the manifest | pass |
| entry with unfilled placeholders | stopped in the initrd with the placeholder message, 10 s after QEMU started |
| entry naming a missing partition | stopped with the message listing the partitions present, 31 s after QEMU started (20 s wait) |
| disk B | differs from A only in its two GPT copies |
| first boot, A and B attached | pass: login prompt at 23 s; systemd-boot came from **disk B's** ESP (byte-identical, naming A), yet `/` and `/boot` were A's partitions by GPT UUID; `by-label/sekka-root` pointed at B's root; B unmounted; 1,938 paths registered; root grew from 14.2 to 18.0 GB; stand-in firmware on tmpfs; `identity.nix` written; shipped flake evaluated offline to the image's toplevel in 16 s; no-change `nixos-rebuild build` offline in 8 s; flake with `identity.nix`, and with setup's sample `setup.nix` under host `analytical`, evaluated offline; no account, root locked; setup's socket active |
| second boot | pass: registration not repeated; `identity.nix` kept; `boot_bin.py` replace, no-op and restore on FAT; update built offline in 22 s (15 derivations, the initrd among them); every entry filled with A's UUIDs, firmware first; `boot.bin` unchanged |
| updated generation | pass: generation 2 with the update, mounted by partition UUID, no identity service; `nixos-rebuild boot --rollback` |
| rolled back | pass: generation 1 again, update gone, `by-sekka` mounts |

First-boot setup's own VM test (`first-boot.vm-test`, a generic NixOS
guest, not this image) passed all ten subtests on the same tree, with the
Aizome built from it and `userspaceIrqchip = true`; with KVM's in-kernel
interrupt controller its guest stalled at boot ("Shell did not start in
time"), as its documentation says it does on this host.

Earlier runs found and fixed: the debug shell's PATH; build users counted
as accounts; the placeholder message written before the virtio console
existed; KVM's stage-2 allocation failing under fragmented memory; and the
flake's pre-setup name (`nixos-rebuild --rollback` looked up `nixos`). The
phase-1 image without first-boot setup (`6f963208`) passed the same boots.

**Not shown by any of this:** m1n1 loading this `boot.bin`, U-Boot,
systemd-boot on the Mac, the T6030 display and GPU handoff, Apple's
firmware archive and the drivers that use it, speakersafetyd, the
device-tree cross-check in a real boot, `boot.bin` replacement on the
Mac's ESP and recovery from macOS, the Asahi installer and Sekka's macOS
installer writing the disk and handoff, Recovery, first-boot setup driven
through its screens, and the developer image's bundle, which was evaluated
and whose system and ESP were built, but whose payload was neither built
nor booted here (disk space). The VM's clock was not the host's (no RTC
driver for QEMU's), which only affects recorded times.

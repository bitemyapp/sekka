# The files index

`aizome-index` (`crates/aizome-index`) keeps what's in folders and drives
in one SQLite database that every Aizome app shares:

| File | What it is |
|---|---|
| `$XDG_CACHE_HOME/aizome/index/files.sqlite` | The index (`~/.cache/aizome/index/files.sqlite` when `XDG_CACHE_HOME` is unset), with SQLite's `-wal` and `-shm` beside it |
| `$XDG_CACHE_HOME/aizome/index/refresh-<n>.lock` | Held while an app refreshes drive *n* |

A drive one app has read is known to the others, and a second look at a
folder reads only what changed under it. Chanoma finds games and videos
with it, and it's meant for any app that looks through many folders,
though none other uses it yet. The palette's Files and Hako's Find still
keep their own list (`aizome-search`'s `FileIndex`, in
`$XDG_CACHE_HOME/aizome/files.idx`), walking the home folder whole
again every `reindex_minutes`; moving it onto this index, keeping its
`.gitignore` rules and its matching in memory, is the next step, and
Kura's scans after it. It's a cache: deleting it costs a first look
again, nothing else.

## How a look works

`Index::refresh(root, &Refresh { stat, progress, stop })` walks `root`
and brings the index in line with it:

- **A folder is listed again only when its modified time has changed**
  since it was last listed. Adding, removing or renaming something in a
  folder changes the folder's time; a change deeper down doesn't change
  the folders above it, so every folder is still looked at (one `stat`
  each), but an unchanged folder's files aren't read, and the index walks
  on into its folders as it knew them.
- **A change in the same tick as a listing** could hide behind an
  unchanged time. A folder listed within a tick of its time is listed
  again next time, as Git does with its index; the tick is two seconds on
  FAT, exFAT, NTFS and HFS+ (FAT's clock is that coarse, and the others
  arrive through drivers that may be), 100 ms elsewhere.
- **Files are told apart from folders by the listing alone** (`d_type`),
  so a look reads no file. A file's size and modified time are read only
  with `stat: true`, which costs one `stat` a file even in an unchanged
  folder, where a file may have been written in place.
- **Drives are known by their filesystem's UUID** and where the mount
  starts in it (a Btrfs subvolume, a bind mount), so a drive plugged in
  again elsewhere, or under another name, is the same drive; a filesystem
  without a UUID (tmpfs) is known by its device and source. Paths are kept
  relative to the drive. A look stays on one filesystem (and so, on Btrfs,
  one subvolume) and follows no links.
- **Not read**: folders whose names start with a dot (configuration,
  caches, `.git`, the Trash), folders holding a `CACHEDIR.TAG`, and the
  folders other systems keep on drives (`$RECYCLE.BIN`, `RECYCLER`,
  `System Volume Information`, `lost+found`), and `node_modules`.
- **A folder that can't be read** (permissions, a drive pulled out) keeps
  what was known of it, and is tried again next time.
- **Stopping**: `stop` is checked between folders; what was read is kept,
  and the report says it stopped.

`Index::entries(root)` returns everything indexed under `root` as it was
last refreshed (an empty list if it never was), with paths in the
filesystem's own bytes. An app shows that at once and refreshes after,
which is how Chanoma's Home appears before a drive has been looked at
again. `Index::generation(path)` is a number that changes whenever
something on that drive's index does, for an app that keeps what it
worked out and wants to know whether to work it out again.

**Facts** are what an app works out from a file: `set_fact(path, key,
value)` keeps a value (bytes, under a key of the app's choosing, such as
`chanoma.crc`) beside the file's size and modified time, and
`fact(path, key)` returns it only while the file still has both. A file
that changes, or goes, takes its facts with it.

## Sharing it

The database is in write-ahead mode, so apps read while one writes. A
refresh commits what it has every 250 ms, so another app sees a long first
look as it goes, and may write in between; connections wait up to 30
seconds for one another. Two apps refreshing the same drive would read it
twice, so a refresh holds `refresh-<n>.lock` (an `flock`) for its drive:
the second waits, then finds little left to read. A database of another
schema version (`user_version`) is dropped and made again.

The schema:

| Table | Holds |
|---|---|
| `volume` | A drive: its key (`uuid:<uuid><root>` or `dev:<maj>:<min>:<source><root>`), label, filesystem type, where it was last mounted, and its generation |
| `node` | A folder, file, link or other thing: its drive, its folder (`parent`, `NULL` for the drive's root), its name as bytes, kind, extension (lower case), size and modified time (nanoseconds, when read); for a folder, the time it had when listed (`listed`) and when that was (`checked`) |
| `fact` | A value an app keeps for a file, with the size and time it was for |

Removing a folder's row removes everything under it (`ON DELETE
CASCADE`). A subtree is read with a recursive query over `parent`.

**Why SQLite**: several processes read and write it safely at once, a
crash leaves it whole, a folder's subtree is one query, and Aizome
already builds it (`rusqlite` with its own SQLite, as Yukimi does). A
key-value store (LMDB, redb) reads single keys faster, but the questions
here are subtrees and joins, which it would need its own indexes for; a
file per app, as Kura's `index.json` is, would be rewritten whole and
couldn't be shared while one app writes it. Around 115 bytes a file: 69 MB
for a home folder of 600,000 files, below.

## Measurements

On 2026-11-02, on the X1 Carbon (Gen 14), with the crate's example
(`cargo run --release -p aizome-index --example index -- PATH [--stat]
[--db FILE]`) and, for comparison, a Perl walk and a C program listing
with `readdir`:

**A USB drive** (SanDisk Extreme Pro, exFAT, read only; 8,326 folders and
194,056 files, a household's game library and backups):

| Reading | Cold (just mounted) | Warm (folders in memory) |
|---|---|---|
| Listing every folder (C, `readdir`) | 189 s | 1.1 s |
| Listing every folder and `stat`ing every file (Perl) | 197 s | 9.8 s |
| `stat`ing every folder only (the index's second look) | 189 s | 0.01 s |
| The index, first look (4,064 folders, 178,467 files) | 84 s | |
| The index, second look | 91 s | 0.05 s |

The 4,064 folders are those left after the ones not read (above): of the
4,260 hidden ones, most are a Mac's Trash (`.Trashes`). The database was
25 MB.

Cold, every way costs the same: exFAT keeps a folder's times in its
parent's entries, so saying anything about a folder means reading its
parent, and that reading is what takes the time on a USB drive. A second
look saves nothing while the drive is cold; it saves everything while
it's warm (the drive stays plugged in), which is when an app looks again.
Apps show what the index knew before the look, so a cold drive costs
waiting for news, not for the list. Ways to make a cold drive faster
are in `docs/files-index-cold-drives.md`.

**A home folder** (ext4 on NVMe; 69,308 folders and 603,352 files, with the
folders in memory): the first look took 3.38 s and the second 0.34 s,
listing nothing. The database was 69 MB.

## Checking it

```sh
cargo test -p aizome-index
cargo run --release -p aizome-index --example index -- PATH [--stat] [--db FILE]
```

The tests cover a second look listing only the folder that changed (and
one changed in the same tick as its listing, listed again), a file gone,
a folder renamed with what's in it, hidden, cache and `node_modules`
folders left out, sizes read only when asked and a file written
in place seen, facts kept until their file changes, two connections (as
two apps) sharing one database, a refresh stopped part way keeping what
it read, mountinfo's escapes, and the drive a path is on, including a
Btrfs subvolume, whose device mountinfo doesn't show (checked against a
Btrfs-shaped mountinfo; no Btrfs filesystem has been indexed).

## Limits

- Changes are found by looking: nothing watches the disk (inotify can't
  watch a whole drive, and fanotify needs privileges). An app refreshes
  when it starts or is asked, not when a file appears.
- A folder whose time doesn't change when its contents do isn't listed
  again: network filesystems that cache attributes can lag, and a
  filesystem that doesn't keep folder times at all (none of those Sekka
  installs) would never be listed twice.
- The racy tick is per filesystem type, not measured: a filesystem with
  a coarser clock than its type suggests could hide a change made in the
  same tick as a listing.
- Only Chanoma uses it: `refresh` and `entries` for its games and videos
  folders, and facts for the CRCs and disc serials of games whose names
  its databases don't know.

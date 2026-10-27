# Kura's index: disk usage for agents

Kura 蔵 (Disk Usage) writes what it finds after every scan, from the window
or from the command line, so that an agent can help a person free disk
space without scanning the disk itself:

| File | What it is |
|---|---|
| `$XDG_CACHE_HOME/aizome/kura/index.json` | The scan as data, in the format below (`~/.cache/aizome/kura/index.json` when `XDG_CACHE_HOME` is unset) |
| `$XDG_CACHE_HOME/aizome/kura/index.md` | The same as a report to read top to bottom: what could be freed and how, the largest folders and files, kinds, Nix |
| `$XDG_CACHE_HOME/aizome/kura/nix-store.json` | The Nix store's last measured size (a cache Kura keeps; not part of the format) |

Both files are replaced atomically (written beside, then renamed), so a
reader never sees half of one. They describe the newest scan, whatever its
root: check `root` before trusting it for a question about another folder.

## How to help a person free disk space with Kura's index

1. **Get a current index.** Run `aizome-kura scan` (the home folder) or
   `aizome-kura scan PATH`. It needs no display, reads but never changes the
   disk, and takes seconds: about a second for 900,000 files with a warm
   cache. If `index.json` exists and `scanned_at` is recent enough for the
   question, read it instead. `aizome-kura scan --json` prints the whole
   index on stdout.
2. **Read the hints first.** `aizome-kura hints --json` prints just them
   (from the last index, scanning first if there's none). They're sorted
   by `reclaimable_bytes`, each with a `risk` and an `action`. Tell the
   person what each would free in plain words, largest first, and how
   risky it is. `reclaimable.by_risk` gives the totals.
3. **Act by risk.**
   - `safe`: caches that are made again by themselves. Still say what
     you're about to remove and how much it frees; then run the hint's
     (or each path's) `command`. Ask the person to quit an app before
     clearing its cache.
   - `low`: rebuilt or downloaded again on demand, at a cost in time (build
     outputs, browser caches). Ask once for the set ("remove the build
     outputs of projects not touched since spring, 48 GB?"), prefer old
     ones (`modified`), then run the commands.
   - `medium`: needs a person's decision, because it's their data (old
     downloads, duplicates) or it can't be undone (emptying the Trash,
     pruning containers). Show the list and let them choose. Move chosen
     files with `aizome-kura trash PATH…`, which is reversible.
   - `high`: only on the person's explicit request (no hint uses it yet).
4. **Never touch `/nix/store`.** Store paths go when nothing roots them and
   the garbage collector runs. For the store, old generations and `result`
   links, open Yukimi's Storage view: `aizome-yukimi --page storage`.
   `aizome-kura trash` refuses store paths.
5. **Look beyond the hints when they don't cover it.** `largest_files`,
   `largest_directories`, `kinds` and `tree` show where the rest is. A
   large file that no hint mentions (a video, a disk image, a model) is the
   person's to decide about: name it and its size, and ask.
6. **Check the result.** Run `aizome-kura scan` again and report the
   difference in `totals.allocated_bytes` and in the filesystem's
   `available_bytes`. Remember that moving to the Trash frees nothing until
   the Trash is emptied (the `trash` hint).

Sizes are **allocated bytes** (`st_blocks × 512`): what removing a file
gives back to the filesystem. A file with several hard links counts once,
at the first name the scan met; its other names show `hard-link` and size 0.
Symlinks are never followed.

## The command line

```text
aizome-kura scan [PATH] [--json] [--output DIR] [--cross-filesystems]
                 [--depth N] [--no-duplicates] [--no-nix] [--threads N] [--quiet]
aizome-kura report [PATH] [--output DIR]
aizome-kura hints [PATH] [--json] [--output DIR]
aizome-kura trash [--json] PATH…
```

- `scan` scans PATH (default: the home folder), writes `index.json` and
  `index.md` into `--output` (default `$XDG_CACHE_HOME/aizome/kura`) and
  prints a summary, or with `--json` the whole index. It stays on PATH's
  filesystem unless `--cross-filesystems` (and even then skips `/proc`,
  `/sys` and other pseudo-filesystems). Duplicate detection reads files;
  `--no-duplicates` skips it. `--no-nix` skips measuring the Nix store
  (`nix path-info --all`, a few seconds, cached until the store changes).
- `report` prints `index.md` for the last index, scanning first if there's
  none or if PATH is given.
- `hints` prints the hints (`--json`: `schema`, `version`, `root`,
  `scanned_at`, `reclaimable` and `hints` from the index).
- `trash` moves each PATH to the home Trash by the freedesktop.org Trash
  specification (`~/.local/share/Trash/files` and `info/*.trashinfo`, so
  any file manager can restore it). It refuses paths in the Nix store, on
  another filesystem than the home folder's, the home folder, the Trash,
  and anything holding either. Exit status 1 if any path was refused;
  `--json` prints `{"trashed": [{"path", "in_trash"}], "refused": [{"path", "reason"}]}`.

Exit status: 0 on success, 1 on failure, 2 for bad arguments.

## Format, version 1

`schema` is always `"org.aizome.kura.index"`. Within a `version`, fields are
only added, never removed or changed in meaning; readers should ignore
fields they don't know. A change that would break a reader bumps `version`.
Times are RFC 3339 in UTC (`"2026-10-17T21:07:20Z"`); paths are absolute;
sizes are bytes.

### Top level

| Field | Type | Meaning |
|---|---|---|
| `schema` | string | `"org.aizome.kura.index"` |
| `version` | integer | `1` |
| `generator` | string | `"aizome-kura 0.1.0"` |
| `scanned_at`, `scanned_at_unix` | string, integer | When the scan started |
| `duration_ms` | integer | How long the scan took (not the hints) |
| `root` | string | What was scanned (symlinks in the argument resolved) |
| `options` | object | `one_filesystem` (bool), `threads`, and sentences saying how sizes, hard links and symlinks are counted |
| `filesystem` | object, optional | The filesystem `root` is on: `mount_point`, `source` (device), `fs_type`, `total_bytes`, `used_bytes`, `free_bytes`, `available_bytes` (free to non-root users: what matters to the person) |
| `totals` | object | `allocated_bytes`, `apparent_bytes`, `files`, `directories`, `symlinks`, `other` (sockets, devices…), `hard_links_counted_once`, `errors` (paths that couldn't be read) |
| `reclaimable` | object | `total_bytes`, and `by_risk`: `{"safe": n, "low": n, "medium": n, "high": n}` |
| `hints` | array of Hint | What could be freed; below |
| `largest_files` | array of Item | The 500 largest files, largest first |
| `largest_directories` | array of Item | The 500 largest folders (the root excluded), largest first; folders nest, so a parent always outranks its children |
| `kinds` | array | Per kind of file: `kind`, `title`, `allocated_bytes`, `apparent_bytes`, `files`; largest first |
| `tree_limits` | object | `max_depth`, `max_children`, `min_bytes`: how much of the tree `tree` keeps |
| `tree` | TreeNode | The root, with its children to `max_depth` |
| `nix` | object, optional | Present when there's a Nix store; below |
| `other_filesystems` | array of string | Mount points inside `root` that weren't entered |
| `errors` | array | The first 50 unreadable paths: `path`, `error` |

### Item

`path`, `allocated_bytes`, `apparent_bytes`, `modified` (for a folder, the
newest modification of anything inside it), `kind` (a file's kind, below,
or `"directory"`), `items` (folders only: files and folders inside).

Kinds: `video`, `audio`, `image`, `document`, `archive`, `disk-image`,
`code`, `binary` (objects, libraries, executables), `database`, `font`,
`log`, `version-control` (anything inside a `.git` folder), `other`. Kind
comes from the name only; nothing is opened to decide it.

### TreeNode

| Field | Meaning |
|---|---|
| `name` | The file or folder's name (the full path for the root) |
| `path` | Its absolute path |
| `type` | `directory`, `file`, `symlink` or `other` |
| `allocated_bytes`, `apparent_bytes` | For a folder, everything inside it |
| `modified` | For a folder, the newest modification inside it |
| `items` | Folders only: files and folders inside, at any depth |
| `flags` | Omitted when empty: `unreadable` (it or something in it couldn't be read: it may hold more), `other-filesystem` (a mount point, not entered), `hard-link` (counted at another name), `skipped` |
| `children` | Largest first; omitted when there are none or the depth limit is reached |
| `omitted` | `{count, allocated_bytes}` of children not listed (smaller than `min_bytes`, past `max_children`, or past `max_depth`) |

By default the tree keeps four levels below the root, at most 25 children
per folder, and nothing smaller than 0.1% of the root or 1 MB
(`--depth N` changes the levels). For anything deeper, run
`aizome-kura scan` on that folder.

### Hint

| Field | Meaning |
|---|---|
| `id` | Stable kind of hint (below) |
| `title`, `summary` | For people: what it is and what to know before acting |
| `reclaimable_bytes` | What acting on it frees, in allocated bytes; 0 when unknown (then see `count`) |
| `risk` | `safe`, `low`, `medium` or `high` (see "Act by risk" above) |
| `action` | `kind`: `command` (run `command`, or each path's own), `trash` (move the chosen paths with `aizome-kura trash`), `open-app` (run `command` to open an app), `review` (decide first); `command` (optional, for the whole hint); `description` (what to do, in words) |
| `paths` | Up to 100, largest first: `path`, `allocated_bytes`, `apparent_bytes`, `modified`, optional `note` (what it is) and `command` (for this path alone) |
| `more_paths` | Paths covered but not listed (omitted when 0) |
| `count`, `count_unit` | For hints measured in things: `4` `generations` |
| `groups` | Duplicates only; below |

No byte is counted by two hints: each claims its folders in this order,
and later hints skip what's claimed.

| `id` | What | Risk |
|---|---|---|
| `trash` | The home Trash (`files`, `info`) and `.Trash-UID` folders on other filesystems inside the scan | medium (emptying is permanent) |
| `containers` | Docker and Podman storage (`~/.local/share/containers/storage`, `~/.local/share/docker`, `/var/lib/docker`, `/var/lib/containers/storage`); paths outside the scan are listed with size 0 | medium |
| `flatpak` | `~/.local/share/flatpak`; `reclaimable_bytes` is 0 because only `flatpak uninstall --unused` knows what's unused | low |
| `browser-caches` | Firefox, Chrome, Chromium, Brave and other browsers' caches, and Electron apps' (`Cache`, `GPUCache`, `Service Worker/CacheStorage`) | low |
| `package-caches` | Cargo, npm, pip, uv, Yarn, pnpm, Go, Gradle, Maven, Nix's eval cache, thumbnails, shader caches…, each with the tool's own cleanup command where it has one | safe or low |
| `large-caches` | Caches that are large and slow to fetch again (Hugging Face models) | medium |
| `caches` | Everything else in `~/.cache`, one path per app | safe |
| `build-outputs` | Cargo `target/` (and shared `CARGO_TARGET_DIR`s, found by their contents), `node_modules/`, Python virtual environments, `dist/`, `build/`, `.next/`, Zig, Cabal, Stack, dune and Mix outputs, tool caches (`.mypy_cache`, `.pytest_cache`, `.ruff_cache`, `.tox`), and `.direnv`/`.devenv` (which hold Nix GC roots); each with its project, and how long since it changed when it's been 90 days | low |
| `tagged-caches` | Other folders holding a valid `CACHEDIR.TAG` | safe |
| `logs` | `*.log` files of 1 MB or more and `log`/`logs` folders of 10 MB or more (not LevelDB's `NNNNNN.log` data); the summary mentions the systemd journal | low |
| `old-downloads` | Items in the Downloads folder (`XDG_DOWNLOAD_DIR`) untouched for 90 days | medium |
| `duplicates` | Files of 1 MB or more with identical contents under different inodes, outside Git working trees, hidden folders (`~/.config`, `~/.local`, tools' own: the programs' data) and what other hints cover | medium |
| `nix-generations` | Old generations of the system, `nix profile` and Home Manager profiles; `count` only, `action` opens Yukimi | low |
| `nix-result-links` | `result` symlinks into the store left by `nix build` (GC roots); `count` only | low |

Hints that would free under 1 MB are left out.

### Duplicate groups

`duplicates.groups` lists each set of identical files: `bytes_each`, `hash`
(xxh3-128, hex), `verified` (`"full"` when every byte was hashed;
`"partial"` when only the size and the first and last 64 KiB matched,
because hashing every copy would have read more than 4 GiB),
`reclaimable_bytes` (all copies but one), and `paths`, oldest first. The
hint's `paths` are the copies after the first of each group. Compare
`partial` sets fully (`cmp a b`) before removing either copy, and let the
person choose which copy to keep.

### Nix

| Field | Meaning |
|---|---|
| `store` | `"/nix/store"` |
| `store_bytes`, `store_paths` | The sum of every valid path's NAR size, and how many paths (before any `auto-optimise-store` deduplication); omitted with `--no-nix` or when Nix can't be asked |
| `store_measured_with` | How: `nix path-info --all --json (sum of narSize)` |
| `profiles` | Each profile with generations: `name` (`system`, `profile`, `home-manager`), `path`, `generations`, `current` |
| `cleanup_command` | `aizome-yukimi --page storage` |
| `note` | Never delete store paths directly |

## Example

Abridged, from a scan of a developer's home folder:

```json
{
  "schema": "org.aizome.kura.index",
  "version": 1,
  "generator": "aizome-kura 0.1.0",
  "scanned_at": "2026-10-17T21:07:20Z",
  "scanned_at_unix": 1792271240,
  "duration_ms": 1004,
  "root": "/home/callen",
  "options": { "one_filesystem": true, "threads": 8, "size_measure": "…", "hard_links": "…", "symlinks": "…" },
  "filesystem": {
    "mount_point": "/home", "source": "/dev/nvme0n1p2", "fs_type": "ext4",
    "total_bytes": 469000000000, "used_bytes": 431000000000,
    "free_bytes": 38000000000, "available_bytes": 17400000000
  },
  "totals": {
    "allocated_bytes": 343000000000, "apparent_bytes": 341000000000,
    "files": 902006, "directories": 112550, "symlinks": 10512, "other": 3,
    "hard_links_counted_once": 1204, "errors": 16
  },
  "reclaimable": {
    "total_bytes": 323000000000,
    "by_risk": { "high": 0, "low": 305000000000, "medium": 2930000000, "safe": 15000000000 }
  },
  "hints": [
    {
      "id": "build-outputs",
      "title": "Build outputs and dependencies",
      "summary": "Build tools make these again from the project's sources; the next build takes longer.",
      "reclaimable_bytes": 303000000000,
      "risk": "low",
      "action": { "kind": "command", "description": "Run each path's command; prefer projects not worked on lately." },
      "paths": [
        {
          "path": "/home/callen/work/sidestep/target",
          "allocated_bytes": 183000000000, "apparent_bytes": 182000000000,
          "modified": "2026-10-15T18:02:11Z",
          "note": "Rust (Cargo) build output of ~/work/sidestep",
          "command": "cargo clean --manifest-path /home/callen/work/sidestep/Cargo.toml"
        }
      ],
      "more_paths": 0
    },
    {
      "id": "nix-generations",
      "title": "Old Nix generations",
      "summary": "4 old generations are kept besides the current ones (4 system). …",
      "reclaimable_bytes": 0,
      "risk": "low",
      "action": { "kind": "open-app", "command": "aizome-yukimi --page storage", "description": "Open Yukimi's Storage view and let the person choose what to remove." },
      "paths": [ { "path": "/nix/var/nix/profiles/system", "allocated_bytes": 0, "apparent_bytes": 0, "note": "5 generations, current 28" } ],
      "count": 4,
      "count_unit": "generations"
    }
  ],
  "largest_files": [
    { "path": "/home/callen/work/…/libsidestep_conformance.rlib", "allocated_bytes": 2100000000, "apparent_bytes": 2100000000, "modified": "2026-10-15T17:58:40Z", "kind": "binary" }
  ],
  "largest_directories": [
    { "path": "/home/callen/work", "allocated_bytes": 323000000000, "apparent_bytes": 321000000000, "modified": "2026-10-17T21:07:19Z", "kind": "directory", "items": 597721 }
  ],
  "kinds": [ { "kind": "binary", "title": "Programs and libraries", "allocated_bytes": 250000000000, "apparent_bytes": 249000000000, "files": 210000 } ],
  "tree_limits": { "max_depth": 4, "max_children": 25, "min_bytes": 343000000 },
  "tree": {
    "name": "/home/callen", "path": "/home/callen", "type": "directory",
    "allocated_bytes": 343000000000, "apparent_bytes": 341000000000,
    "modified": "2026-10-17T21:07:19Z", "items": 1024071,
    "children": [ { "name": "work", "path": "/home/callen/work", "type": "directory", "…": "…" } ],
    "omitted": { "count": 61, "allocated_bytes": 212000000 }
  },
  "nix": {
    "store": "/nix/store", "store_bytes": 54400000000, "store_paths": 37879,
    "store_measured_with": "nix path-info --all --json (sum of narSize)",
    "profiles": [ { "name": "system", "path": "/nix/var/nix/profiles/system", "generations": 5, "current": 28 } ],
    "cleanup_command": "aizome-yukimi --page storage",
    "note": "Never delete paths in /nix/store directly: remove old generations and collect garbage through Yukimi's Storage view."
  },
  "other_filesystems": [],
  "errors": [ { "path": "/home/callen/.local/share/docker/…", "error": "Permission denied (os error 13)" } ]
}
```

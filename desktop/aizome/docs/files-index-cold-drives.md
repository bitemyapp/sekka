# Cold exFAT drives: options

Options for making the files index's look at a just-plugged-in exFAT
drive faster, written up on 2026-11-02 for discussion. Nothing here is
built. The measurements are those in `docs/files-index.md`, from the X1
Carbon (Gen 14) and a SanDisk Extreme Pro USB drive (exFAT, mounted read
only); the kernel details are from Linux's `fs/exfat` on master that day.
The drive wasn't plugged in while this was written, so nothing below has
been tried on it.

## The problem

| Look at the drive | Cold (just mounted) | Warm |
|---|---|---|
| Every folder listed (8,326 folders) | 189 s | 1.1 s |
| The index's first look (4,064 folders, 178,467 files) | 84 s | |
| The index's second look | 91 s | 0.05 s |

**There's little to read, read in many small pieces.** exFAT spends about
five 32-byte entries on a file, so 178,000 files are roughly 30 MB of
folder entries (an estimate: long names take more). A USB 3 drive reads
that in well under a second in one stream. The first look took 84 s
instead, about 20 ms a folder: the time goes to waiting for small reads,
one after another.

**Why one at a time.** Linux's exFAT driver reads a folder's entries a
sector at a time (`exfat_get_dentry`, through `sb_bread`), reading ahead
at most a cluster of the same folder, and holds one lock for the whole
filesystem (`s_lock`) while it waits for each read: listing a folder
(`exfat_iterate`) and finding a name in one (`exfat_lookup`) both take it.
So a drive has one small read in flight at a time, however many threads
or apps are looking at it, and each waits a round trip over USB. This is
read from the source, not traced on the drive: measure it first (below).

**Why a second look doesn't help cold.** exFAT keeps a folder's modified
time in its parent's entries, so asking whether a folder changed means
reading its parent, nearly as much reading as listing it.

**Folder times may not be trustworthy on a shared drive.** The index
lists a folder again only when its modified time changes. Linux's exFAT
driver sets a folder's time when something is added to it or taken out;
Windows reportedly doesn't on FAT (fontconfig stopped trusting folder
times there, and checksums the entries instead), and nothing firm was
found for exFAT on Windows or macOS. This drive has been on a Mac (it has
`.Trashes`). If games are added from a system that leaves the folder's
time alone, the index won't see them. That wants testing, a game added to
a folder from each of Windows and macOS and the folder's time read
before and after, before trusting the index on a drive a household
shares.

## Options, cheapest first

1. **Read less.** The index reads what apps ask for, and the cold cost
   follows what's read. Chanoma's look at the drive's `roms/` (412
   folders, 89,803 files) found 72,949 of those files in ES-DE's
   `images`, `videos` and `manuals` folders, which Chanoma doesn't use;
   without them it would read about 300 folders and hold about 17,000
   files (about 2 MB rather than 11.4 MB), and skip the drive's biggest
   folders. This needs apps to say what they want (folders to leave out),
   which the index is to have anyway. Not timed cold.
2. **Hide the wait.** Apps already show what the index knew before the
   look. A look could also take the folder on screen first.
3. **Fill the kernel's cache in bulk, then look as usual.** The exFAT
   driver's reads go through the partition's page cache (buffer heads on
   the block device), so anything that has read those blocks already
   makes the driver's reads hits.
   - After a look, a helper records which of the partition's blocks are
     cached (`mincore` over a read-only mapping of the partition, or
     `cachestat`): where the drive keeps its folders, its FAT and its
     allocation bitmap. Kept as ranges, that's tens to hundreds of
     kilobytes a drive.
   - When the drive is next mounted cold, the helper asks for every range
     at once (`posix_fadvise(POSIX_FADV_WILLNEED)` per range), so the
     drive has many reads in flight rather than one, and then the look
     runs as it does warm (0.05 s for a second look).
   - The ranges are only a hint: where the layout changed, the look reads
     that part cold, as it does now, so it can't make the index wrong. It
     needs no exFAT code of Aizome's, and works for any filesystem keeping
     its metadata in the block device's cache (FAT, exFAT, NTFS, ext4),
     not Btrfs.
   - It needs the partition opened, which a user can't do. A small helper
     allowed by polkit only to read ahead ranges of a mounted removable
     drive's partition, and to say which ranges are cached, hands back no
     data, so it can't leak deleted files.
   - It doesn't help the first look at a drive on a computer, which has no
     ranges yet.
   - How much it gains depends on how many reads the drive takes at once:
     a lot over UAS, less over the older one-command-at-a-time USB storage.
4. **Read the folders without the driver.** Read exFAT (whose
   specification has been public since 2019) from the partition in user
   space, read only: the folders of one depth all at once with
   `io_uring`, so the reads go in a few rounds rather than thousands in a
   row, comparing each folder's raw entries with last time rather than
   trusting its time.
   - This also answers the folder-times question above, and helps a first
     look.
   - It costs an exFAT reader to keep, and a helper that does hand back
     names, which must leave out deleted entries (exFAT leaves their names
     readable): more to trust than option 3. Reading through the
     partition's page cache (not `O_DIRECT`) keeps it in step with what
     the mounted filesystem has yet to write.
5. **Change the kernel**: drop `s_lock` while a read is waited for, or
   read a folder's clusters at once. The real fix, upstream and slow.
6. **Look when the drive is plugged in**, rather than when an app opens.
   Cheap, but it's background work for every drive, which isn't wanted
   yet.

Not worth it: a fingerprint of the allocation bitmap (one read of 1 to
4 MB) catches files added and removed, but not renames, moves or empty
files, so it can't prove nothing changed.

## Measuring it, when the drive is back

None of these need root:

- the drive's counters (`/sys/block/sdX/stat`) before and after a cold
  look: how many reads, how large, how long each waited;
- `lsusb -t`: whether it's on UAS at USB 3 speed;
- `stat -f` on the mount: the cluster size;
- a look with two threads: whether the lock makes them take turns.

Making the drive cold again means unmounting it and mounting it again
(read only), which is the drive's owner's to agree to.

## Suggested order

Option 1 with the index's per-app requests; then measure; then a
prototype of option 3, which needs no exFAT reader and can't make the
index wrong; option 4 if folder times turn out not to be trustworthy
from Windows or macOS.

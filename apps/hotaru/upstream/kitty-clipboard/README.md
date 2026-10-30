# Kitty clipboard protocol (OSC 5522) evidence

Recorded 2026-10-29 on `wx-workstation`, under the shared workstation lock.

`tools/kitty-clipboard-oracle.py` drives the pinned libghostty-vt (`70e1bdcf…`,
Ghostty `6ba14415`) through its own C API:

- its clipboard read and write effects;
- `ghostty_terminal_paste()`;
- a fixed entropy source installed with `ghostty_sys_set`.

Its ctypes layouts are checked against the library's ABI manifest first. Each
scenario also runs through `kitty-clipboard-replay` (in `hotaru-replay`), which
feeds `hotaru-core` and answers its requests with the same embedder model.

The model, on both sides:

- **Clipboard:** typed contents for the standard and primary locations.
- **Decisions:** each read and write takes the scenario's next decision
  (success when none are left).
- **Reads:** a successful read serves the requested types the clipboard has.
- **Writes:** a successful write replaces that location's contents.
- **Grants:** `remember` records a grant where the program gave a name and a
  password.

The comparison covers every byte written back to the program and every
request the embedder is asked to answer, step by step. It runs at feed chunks
of 1, 3 and 65,536 bytes. The library's answers at those chunk sizes are also
compared with each other.

- `oracle-summary.json`:
  - 207 scenarios and 615 comparisons, with no failures.
  - One named difference: Hotaru's bounded event prefix. Past 64 clipboard
    requests in one feed, a program hears `EBUSY` where the library would ask
    the embedder about all 70. It differs only when the 70 arrive in one
    65,536-byte feed.
  - What the reference exercised: every answer status, grant state and
    paste-event outcome.
  - The hashes of every source it ran against.
  - The full report (277 KB) stays on wx at
    `~/sekka-work/hotaru-5522-oracle.json`.
- `../kitty-clipboard-fixtures.json`:
  - The library's answers for 203 scenarios (all but the three 8 MiB capture
    cases and the named difference).
  - `hotaru-replay`'s `tests/kitty_clipboard.rs` replays them at all three
    chunk sizes, so regressions show without the library.

The first run found two divergences, both fixed with core tests:

- **Long requested names:** Hotaru dropped requested type names longer than
  256 bytes; the library keeps them.
- **Capture limit:** Hotaru captured OSC 5522 bodies of up to 8 MiB − 1 (OSC
  52's limit, which leaves a byte for a sentinel). The library's 5522 capture
  takes exactly 8 MiB.

## The scenarios

- **Metadata:** malformed records, duplicate and unknown keys, ids,
  locations, names, passwords and their length limits, each with and without
  a payload.
- **Read lists:** listings, order, absent types, the four-type cap, long
  names, whitespace, and lists that aren't strict base64 or UTF-8.
- **Reads and grants:**
  - Every read result.
  - Grants per password and direction, never from a nameless password, never
    spent by a listing.
  - The oldest of 32 grants evicted.
- **Writes:**
  - Types, chunks, reused types, empty types and commits, commits twice,
    orphaned packets, a write replaced in flight, 66 types.
  - Aliases: chains, missing targets, collisions, 70 aliases, long names.
- **Base64 streams:**
  - Split at every boundary of a 28-character stream.
  - Independently padded packets.
  - Fourteen stream shapes, valid and corrupt: padding mid-feed, split
    padding, trailing bits, bad characters, an unpadded end.
  - Invalid metadata in the middle of a write.
- **Limits and decisions:** write limits around the data, and every write
  result and grant.
- **Mode 5522:** DECRQM, XTSAVE/XTRESTORE, DECSTR and RIS.
- **Paste events:**
  - Fixed entropy, including rejection sampling and running out.
  - Follow-up reads with the one-time password: once, not by a listing, not
    in the wrong direction, not without a name, not after RIS.
  - Long listings and the mode switched off.
- **Resets:** RIS keeping a write in flight, and ending grants.
- **The OSC capture:** at 8 MiB − 1, 8 MiB and 8 MiB + 1.

# OSC 52: actual GTK and parser observations

These observations use the unchanged Ghostty commit
`6ba144158577d033ccb6ed4b574e4013957c9161`, its GTK application and its
exported OSC parser. They are reference evidence, not Hotaru qualification.
The [receipt and raw results](../upstream/osc52-reference/receipt.json) identify
the binaries, harnesses, source files and selected runs. Everything ran on
`wx-workstation` under the shared lock, in private headless Sway sessions.
PRIMARY was advertised and worked in those sessions; availability on other
compositors is not established by this fixture.

## Parser and routing

The actual exported parser accepts only literal `52;`. Leading zeroes, plus,
spaces and a trailing space in the command number are rejected. An empty kind
means `c`; a single raw byte is accepted, including `ff`; two or more bytes
are rejected. The terminal routes `c` to standard clipboard and `p`/`s` to
PRIMARY for writes. An unknown kind uses standard clipboard. Reads always
read standard clipboard, while retaining `p` or `s` in the response tag.

The allocating parser's 8,388,608-byte limit counts kind, delimiter, payload
and its added terminating NUL, but excludes `52;`. Thus `c;` accepts at most
8,388,605 payload bytes, and `;` accepts 8,388,606. The actual GTK fixture also
checks valid base64 near this limit, including a 6,291,454-byte decoded copy.
Oversize commands leave the prior selection unchanged.

The full terminal stream filters NUL before the OSC parser and CAN/SUB cancel
an OSC. DEL remains payload. Direct `ghostty_osc_next` calls do not perform
those stream filters: the exported-parser fixture and GUI-stream fixture
deliberately retain this distinction. BEL and ST requests both receive ST
responses because `clipboardContents` does not propagate the parser's stored
terminator to the application request.

## Bytes and policy

Allowed writes preserve decoded bytes in the advertised UTF-8 MIME, including
NUL and invalid UTF-8. GTK text reads are a different conversion path:

| Raw bytes | Observed decoded reply bytes |
| --- | --- |
| `61 00 62` | `61` |
| `80`, `ff` | literal ASCII `\80`, `\FF` |
| `c0 af`, `ed a0 80` | each byte escaped as uppercase `\XX` |
| truncated `e2 82` | empty |
| `f4 90 80 80` | unchanged, despite exceeding Unicode's scalar range |
| valid UTF-8, ESC, DEL, CR, LF | unchanged |

Own and foreign clipboard reads agree for all 16 tested byte payloads.
This is not a general model of GLib conversion. Hotaru's planned raw-byte
path intentionally preserves the captured bytes instead of reproducing this
lossy frontend conversion. Approval authorizes the original bytes, never
the bounded display preview. The raw reference results remain available.

Configured read denial sends no reply; asked-read denial sends an empty OSC
reply. Asked writes do not change the selection before approval. Plain-text
approval works. GTK's NUL-containing ask-write preview cleared the clipboard
when approved; invalid UTF-8 emitted a GTK text-buffer validation assertion
and did not replace the selection. These destructive preview behaviors are
explicit differences, not desired Hotaru behavior. An asked read approved
after an external replacement returns the old captured preview.

## Asynchrony and cleanup

Repeated write/query bursts return every captured value, but query responses
arrive in permutations. A pending read of a stopped old source survives a
selection replacement and completes with that old source's data. Killing the
source completes with an empty reply. Hotaru may preserve request ordering;
the reference's asynchronous reordering is recorded, not normalized away.

A stopped source does not prevent a DSR response. A genuine unread 2 MiB
clipboard transfer does not prevent a private window resize. Closing its pipe
allows later query/DSR traffic. Closing Ghostty during a pending read exits
with status zero before the stopped source resumes. Selected runs have no
forced descendant cleanup and all owned processes exited. This is bounded
behavioral evidence, not a throughput or latency benchmark.

## Source explanation and limits

The unchanged reference sources are `src/terminal/osc.zig`,
`src/terminal/osc/parsers/clipboard_operation.zig`,
`src/termio/stream_handler.zig`, `src/Surface.zig`, and
`src/apprt/gtk/class/surface.zig`. Parser capture limits, one-byte kind routing,
standard-only reads, unconditional ST replies, explicit GBytes lengths,
`readTextAsync`, NUL slicing and approval-preview conversion explain the
observations. The receipt hashes these files. The C shim calls only exported
parser functions and returns the command type; it does not inspect private
command layouts or substitute a modeled parser.

Producer syscall chunks 1/7/65,536 are recorded; the kernel and application
may coalesce them, so these are not claimed to be exact parser chunk sizes.
Snapshots taken during ask tests did not establish visual dialog rendering;
the policy conclusions use keyboard actions and byte-level effects.
Earlier harness attempts are retained with explicit exclusions in the
receipt: incompatible EGL setup, stale input serials, a short controller
socket write, and a false slow-reader setup were corrected before selecting
the final runs. Failed harness attempts are not application failures.

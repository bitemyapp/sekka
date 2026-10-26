# Terminal theme changes and Codex

**Decision (2026-10-23): defer automatic Codex theme refresh.** Aizome keeps
Foot's palette consistent with the desktop, but an application inside Foot
can retain colours chosen before the palette changed. Fixing that in Codex
is feasible, but needs input handling and rendering work in Codex rather
than another signal from Aizome. No Codex patch or automatic restart is
part of the Foot theme change.

These findings were checked against Codex CLI **0.163.0** and Foot
**1.28.0**. Recheck newer versions before implementing a workaround.

## Why the colours become unreadable

The [shell](shell.md#behaviour) selects Foot's dark or light palette when
the desktop theme changes, when a terminal window opens, and when the
shell connects. Foot updates its default foreground/background and ANSI
palette, so applications using those colours follow the change.

Codex also uses explicit RGB colours. At startup it queries the terminal's
default foreground and background with OSC 10 and OSC 11, caches the
result, and uses it to derive input-box, submitted-prompt and status
styles. Its automatic syntax theme is selected from that background too.
In this version the cached colours are not refreshed during the session;
regaining focus deliberately leaves them unchanged.

After a Night-to-Day switch, Foot can therefore display its new dark
foreground alongside Codex's old dark input background and pale syntax
colours. The inverse mismatch happens after Day-to-Night. Foot has changed
successfully, but Codex is mixing live terminal colours with explicit RGB
styles chosen for the previous background. Repainting the same cached
styles does not resolve that mismatch.

There is also a startup ordering concern: Aizome selects the palette when
the window maps, while Codex can probe colours during its own startup.
Launching both together does not guarantee Codex observes the final
palette. An explicit Foot initial theme avoids that race.

## Current workaround

After changing the desktop theme, reopen Codex in a terminal whose palette
has already changed and resume the conversation with `codex resume`.
Preserve any unsent draft first. To start a fresh Foot and Codex together
with a known initial palette, use the command matching the desktop:

```sh
# Day
foot --override initial-color-theme=light codex resume

# Night
foot --override initial-color-theme=dark codex resume
```

This fixes startup selection, not subsequent live changes. Codex's
`/theme` command and `tui.theme` setting select syntax highlighting; they
do not refresh the cached terminal foreground/background and are not a
complete fix for the input box or other derived styles.

## A possible future fix

Foot already supports terminal colour-theme notifications. An application
can enable DEC private mode 2031 (`CSI ? 2031 h`), query the current theme
with `CSI ? 996 n`, and receive `CSI ? 997 ; 1 n` for dark or
`CSI ? 997 ; 2 n` for light. Here `CSI` means the escape sequence `ESC [`.
Foot emits these reports on palette switches when reporting is enabled.
The report identifies light/dark, not the actual RGB palette.

A Codex implementation could subscribe, then request fresh OSC 10/11
colours after a notification. Both notifications and colour replies should
be parsed through the existing terminal input reader, preserving keyboard
and paste events. Once the colours are available, Codex would update its
cache, recompute derived styles and the automatic syntax theme, invalidate
render caches, and redraw the transcript, composer and status line.
Explicit user-selected syntax themes should remain selected.

Codex already has runtime syntax-theme replacement and a theme revision
used to invalidate some rendering caches. That helps, but does not provide
the complete terminal-palette refresh. The change belongs in Codex's TUI
and potentially its pinned Crossterm input parser. Aizome would continue
to select Foot's palette without managing individual Codex processes.

## Costs and risks

- **Input correctness and latency.** Codex's existing direct colour probe
  has a 250 ms timeout and reads from the TTY. Reusing it in the live input
  loop can block interaction or consume keystrokes intended for the normal
  reader. The focus handler explicitly documents this reason for keeping
  the startup cache. Clearing the cache and probing on every focus event
  is therefore not a safe shortcut.
- **Protocol compatibility.** Terminals and multiplexers may not support
  or forward these sequences. Replies can be missing, partial or delayed.
  A refresh needs a bounded asynchronous fallback, preservation of the last
  usable palette, and handling for rapid repeated switches. Subscription
  state also needs appropriate cleanup and restoration on exit or suspend.
- **Rendering consistency.** Refreshing only syntax highlighting leaves
  other stale styles. Overlays, cached transcript content and streaming
  output need checking too. Text already emitted into native terminal
  scrollback may retain explicit RGB colours even after the active UI is
  repainted; complete historical recolouring should not be assumed.
- **User preferences.** Automatic light/dark selection must coexist with
  custom syntax themes rather than silently replacing a deliberate choice.
- **Maintenance.** A local fix means carrying and rebuilding a patched
  Codex package, potentially including its Crossterm fork, until upstream
  supports it. Updates would require rebasing and regression checks, and a
  changed build cannot simply reuse the unmodified binary package.
- **Automatic restarts are disruptive.** Restarting or replacing a Codex
  client on every desktop theme change could interrupt typing, lose an
  unsent draft, or disturb an active client attachment. It is not the chosen
  workaround.

If revisited, test repeated Day/Night switches while idle, typing, pasting
and streaming; automatic and explicit syntax themes; unsupported terminals
and multiplexers; and late or incomplete replies. Verify the composer,
transcript, status line and overlays together, including suspend/resume.
This is a contained upstream improvement, but more than a configuration
change. For now, manual reopening is the accepted limitation.

## Source pointers

In the Codex 0.163.0 source tree, under `codex-rs/tui/src/`:

- `tui.rs`: startup probe and palette-cache initialization.
- `terminal_palette.rs`: cached default colours, including unavailable
  results.
- `terminal_probe.rs`: OSC queries, timeout and direct TTY reader.
- `tui/event_stream.rs`: `FocusGained` keeps the startup palette and explains
  the input-loss risk.
- `style.rs` and `render/highlight.rs`: derived UI colours, automatic syntax
  theme selection and runtime syntax-theme revision.
- `app/event_dispatch.rs` and `chatwidget/rendering.rs`: syntax-theme changes
  and render-cache invalidation.

Foot's `foot-ctlseqs(7)` documents mode 2031 and the 996/997 reports;
`terminal.c` in Foot 1.28.0 emits the reports when switching palettes.

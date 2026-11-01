# Linux terminal identity and shell startup

Hotaru uses `TERM=xterm-hotaru` when its private compiled terminfo entry is
readable and has a valid basic header. Without that entry it uses the existing
`xterm-256color` fallback. It always supplies `COLORTERM=truecolor`,
`TERM_PROGRAM=Hotaru` and `TERM_PROGRAM_VERSION`. The application removes an
inherited `VTE_VERSION` so children do not mistake it for a VTE terminal.
Explicit per-command environment overrides apply last.

The Nix wrapper supplies `HOTARU_TERMINFO_DIR` and `HOTARU_RESOURCES_DIR`.
The PTY worker selects identity and reads resource metadata before forking;
the UI thread performs no resource lookup. `TERMINFO` names the private
database, and `TERMINFO_DIRS` prepends it while preserving inherited search
paths and an empty component for ncurses' compiled defaults. Terminfo is
compiled with `tic -x` and installed under `share/terminfo`; the text source is
also available for deliberate installation on another machine. This slice
does not copy terminfo to SSH servers or wrap SSH/sudo commands.

The entry is a documented subset of pinned Ghostty's MIT-licensed
`src/terminfo/ghostty.zig`. It advertises implemented grid, color, underline,
keyboard, bracketed-paste, focus and mouse capabilities. It advertises OSC 52
(`Ms`, which tmux's `set-clipboard` relies on) and horizontal margins, and omits
synchronized output, cursor shape/blink and character repeat capabilities whose
native behavior is not yet qualified. The name does
not claim full Ghostty compatibility. On a remote machine without Hotaru's
entry, explicitly use an available terminal description such as
`TERM=xterm-256color ssh host` or install the entry there.

## Default shells

New panes start fish where it's installed, else `$SHELL`, else `/bin/sh`;
`shell = "…"` in Hotaru's TOML settings names another. Fish is the shell
Sekka's terminals open in. Started directly, it loads Hotaru's fish
integration whatever the login shell is, which a bash handing itself over to
fish would skip, and it doesn't depend on the `SHLVL` a Hotaru started from a
shell inherits. A command named at launch (`-- COMMAND`) runs as given.

`PtyCommand::shell()` enables integration for a recognized default shell with
no explicit arguments. `PtyCommand::new()` and native `-- COMMAND [ARG…]`
preserve the command and arguments and disable automatic injection, including
explicit shell commands. `shell_integration = false` in Hotaru's TOML settings
disables automatic integration for future panes. Missing resources and unknown
shell names leave the launch unchanged. Startup files are never modified.

- Bash receives `--rcfile` pointing to a small bootstrap. Bash reads its normal
  compiled system rc first; the bootstrap reads the user's `.bashrc` once and
  then registers the hook. No POSIX-mode transition, `ENV`/`BASH_ENV`
  replacement, login-profile emulation, prompt rewriting or DEBUG trap occurs.
  Scalar and indexed-array `PROMPT_COMMAND` forms retain their type and order;
  existing commands receive the incoming exit status. Readonly/associative
  settings are left alone.
- Zsh temporarily receives a bootstrap `ZDOTDIR`. The bootstrap restores its
  original value, reads the original user `.zshenv`, and lets Zsh perform the
  remaining normal global/user startup chain. A global zshenv that overwrites
  ZDOTDIR can prevent automatic loading. A `.zshrc` framework that replaces
  `precmd_functions` can remove the earlier hook. For either case, source
  `"$HOTARU_RESOURCES_DIR/shell-integration/zsh/hotaru.zsh"` after the framework
  in the user's configuration if desired. Repeated sourcing re-registers a
  missing hook without duplicating it or replacing the framework's array.
- Fish receives `--init-command` to load the hook after its normal configuration.
  Its own prompt function and event handlers remain intact. Recent Fish can
  emit cwd/title/semantic sequences itself even when Hotaru integration is
  disabled; Hotaru does not disable those independent shell features.

The compact hooks are original MIT-licensed code. Ghostty's Bash and Zsh shell
files contain separate GPLv3 notices and are not copied into this implementation.
The startup contract was checked against the pinned Linux source and the
[Bash manual](https://www.gnu.org/software/bash/manual/html_node/Bash-Startup-Files.html),
[Zsh startup documentation](https://zsh.sourceforge.io/Doc/Release/Files.html)
and [Fish language documentation](https://fishshell.com/docs/current/language.html).

## Working directories and titles

The hooks emit only a percent-encoded ASCII OSC7 URI. They never put raw
filename bytes inside an OSC title. The PTY worker accepts `file://` URIs and
Kitty's raw-path `kitty-shell-cwd://` form only for literal `localhost` or the
exact kernel hostname, matching the pinned Linux reference. An empty host,
remote host, credentials or port is rejected; no DNS lookup occurs. `file:`
queries/fragments must be encoded as filename bytes rather than URI syntax.
An empty OSC7 payload clears the last reported directory, as in Ghostty.

The core bounds OSC7 bodies to 16 KiB, enough for a percent-encoded Linux
PATH_MAX path; other ordinary OSC bodies retain their 2 KiB limit and OSC52
retains its independent limit. Directory decoding bounds the resulting path
to 4096 bytes and rejects NUL, ASCII controls and C1 controls, including valid
UTF8 control subsequences surrounded by otherwise invalid filename bytes.
Non-control non-UTF8 Linux names remain representable. The core owns/coalesces
the URI event and the actor owns parsing; neither interprets shell text.

Accepted directory text supplies a native sanitized title until the program
sets an explicit title. New tabs and splits inherit the focused pane's last
accepted local directory. Without a report, the source PTY worker reads its
shell's `/proc/PID/cwd`, including for explicit commands and when shell
integration is disabled. The source session is captured before changing focus;
the new worker waits up to one second for the reply, falling back to the last
known or original launch directory if the lookup fails or the shell has exited.
The PTY owner checks exit before looking up the PID so a reaped child's PID
cannot select an unrelated process. No directory lookup or wait runs on the UI
thread. A new external launch keeps its explicitly supplied directory.

Shell reports remain preferred for nested shells; the process fallback follows
the local shell Hotaru started, not a remote SSH directory or a nested shell
that sends no reports. Reported paths are not opened during parsing; a later
spawn reports any failure to enter the selected directory normally.

## Validation

`hotaru-io/tests/shell.rs` runs real Bash, Zsh and Fish through Hotaru's Linux
PTY and terminal-core replies in disposable homes. Its 14 cases cover normal,
disabled and explicit launches, inherited ZDOTDIR, scalar/array prompt hooks,
Zsh framework hook replacement with manual re-registration, preserved ENV and
BASH_ENV values, and filenames containing BEL, ESC, raw ST/CSI, Unicode C1 and
Japanese text. No host dotfiles are read or changed. Run explicitly:

```sh
HOTARU_TEST_FISH=/path/to/fish cargo test --manifest-path apps/hotaru/Cargo.toml \
  -p hotaru-io --test shell actual_shell_startup -- --ignored --nocapture
```

Focused Rust tests cover missing/corrupt terminfo fallback, private/default
database lookup, explicit overrides, URI decoding/control rejection, OSC7
boundary/overflow behavior across chunks, local/remote actor handling, title
precedence and pane cwd inheritance. The later [semantic metadata slice](semantic-prompts.md)
retains observed OSC 133 cell/row markers and prompt-preserving clear behavior.
These hooks still emit only cwd information. Complete marker emission, command
navigation, remote terminfo transport, login-shell automatic injection and
framework-wide automatic-hook compatibility remain separate work.

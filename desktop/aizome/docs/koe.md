# Koe

Koe 声 ("voice") is Aizome's dictation: press ❖⌥D in any app, speak, press
it again, and what you said is typed where you were. Recognition runs on
this computer with Whisper (whisper.cpp, through `whisper-rs`), on the
graphics card where there is one. It's one of Aizome's apps (`apps/koe`,
`aizome-koe`, `org.aizome.Koe`): a window that sets dictation up and edits
its vocabulary, and a small session service that listens, recognises and
types.

Koe began as a Rust reimplementation of the core of
[hyprwhspr](https://github.com/goodroot/hyprwhspr) (MIT, © 2025 goodroot;
`apps/koe/NOTICE`), built around Aizome rather than Hyprland. It isn't
hyprwhspr's equal yet: no waveform overlay, streaming partial transcripts,
other recognition backends (Parakeet and the like), audio ducking or X11.

## Setting it up

Nothing about dictation needs a terminal, and nothing is downloaded or
started for anyone who doesn't use it. Open **Koe** from the Toolchest or
the palette. The **Dictation** page has everything:

- **Download a Model and Turn On**, at first: it downloads Base (148 MB,
  many languages), checks it against the catalog's SHA-256, turns
  dictation on and gives it its shortcuts. The progress shows in place and
  can be cancelled, even while the server has gone quiet; a cancelled or
  failed download leaves nothing behind, and closing Koe during one waits
  until it has. A partial file left by a session that ended mid-download
  goes with the next download.
- **The switch** at the top turns dictation on and off. On starts the
  service now and at every login; off stops it, and it no longer starts.
  Without a model it stays off.
- **Speech Model**: Tiny, Base and Small, each for many languages or
  English only, with their sizes. Download, Use and Remove… (which says how
  much it frees; the model in use can be removed only with dictation off).
  A model that isn't the catalog's size shows as Incomplete, with Download
  Again. Show in Files opens the folder.
- **Recognition**: the language (or Detect automatically; English-only
  models know only English), Translate into English, Use the graphics card
  (on by default; where no card starts, a Vulkan build without a driver
  among them, Koe uses the processor, without the flash attention that
  would slow it there, and says so), and the microphone, which is
  Settings ▸ Sound's input. Ready says where recognition runs.
- **Typing**: what happens when you finish (type it into the app, copy it,
  or keep it in Koe), finishing by itself after a pause, a space after,
  and notifications.
- **Shortcuts**: turning dictation on gives it ❖⌥D to start and stop and
  ❖⌥⎋ to cancel, unless those keys already do something else (then Use…
  offers what's free). Change in Keyboard Settings… opens Settings ▸
  Keyboard ▸ Shortcuts, where they're listed by name under Commands. With
  dictation off, the shortcut opens Koe, where it can be turned on.

The header says what dictation is doing: Ready (with the shortcut, and how
long the last recognition took), Listening (with the time and the
microphone's level), Transcribing, Typing, or what went wrong, with
Dismiss. If the service is still running with dictation off (it couldn't
be stopped, or `koe.toml` was edited), it says so, with Stop It.

Settings apply as they change. The window saves `~/.config/aizome/koe.toml`
(readable only by its owner) and the service reads it again within a
second, between dictations: no restart, and the model reloads only when
recognition settings changed. If the file can't be read, the window says
why and offers to start again with the usual settings, keeping the broken
file beside it as `koe.toml.broken`. The usual settings have dictation
off, so the service stops first, whatever it was doing; `aizome-koe
disable` stops it too, even when it can't save the choice.

## Using it

Press ❖⌥D in the app you want to type into, speak, and press ❖⌥D again.
The microphone closes at once, Transcribing shows, and the text is pasted
into the app that was in front when you started: ❖V for Aizome's apps,
Ctrl+Shift+V for terminals (real modifiers, so programs such as Codex don't
take it as their image-attachment Ctrl+V), Ctrl+V elsewhere.

- **Focus and locking**: before replacing the clipboard and again just
  before pasting, Koe checks that the same window has the focus, the
  session is unlocked and no keys are held. It never takes the focus back.
  If a check fails, nothing is pasted and the words are kept: Paste Last
  Transcript (in Koe's systray menu) puts them in the window that's in
  front. A recognition that finishes while the shortcut is still held
  waits up to five seconds for its release.
- **The systray**: Koe's microphone shows while dictation is on: cream when
  ready, red when listening, blue when paused, amber dots while it works,
  and a warning triangle with the reason when something went wrong. Its
  menu starts, pauses, resumes and cancels; pastes, copies or forgets the
  last transcript; unloads the model; opens Vocabulary and Teaching… and
  Koe…; and turns dictation off. Opening the menu never starts the
  microphone.
- **Notifications** say when Koe is listening, transcribing and done, and
  what went wrong; never what you said.

## Vocabulary and Teaching

The window's second page (❖2, or the systray's Vocabulary and Teaching…)
is for words Whisper gets wrong. **Packs** of words can be enabled together;
their written forms become recognition hints, and their approved
corrections replace whole words, ignoring case, from the next dictation.
The **Sekka** pack starts with Aizome, Sekka and Koe (with Isome and IZOMA
corrected to Aizome); **Personal** starts empty. Packs export to a
`.koe.json` file of names, written forms and corrections only, and imported
packs start disabled.

**Teaching** records an example of a word in a sentence (dictation must be
on: the service records and recognises it): Record example, Stop
recording, then correct the complete sentence. An isolated correction can
be approved as one with a button. Save example keeps the recording and the
corrected transcript on this computer, for training later; Discard, or
closing the window, throws them away. No model training runs yet. Export
training data writes a pack's saved examples to a new folder.

Packs and saved examples are read and written by the window directly
(through the store's lock and revision checks), so they can be edited with
dictation off; only recording an example needs the service.

## Privacy

- Ordinary dictation audio is held in the service's memory, capped by
  `audio.max_seconds`, and never written to disk. The last transcript is
  kept in memory until it's replaced, forgotten or the service stops.
  Memory may be swapped by the system; the unit disables core dumps.
- Teaching recordings are saved only by Save example, in private folders
  (0700, files 0600) under `~/.local/share/aizome/koe/vocabulary`.
- The status the window, the systray and the shell see never contains
  audio or text; only an explicit `last` request returns the transcript.
- Clipboard contents are replaced when Koe pastes and aren't restored; a
  clipboard history may keep them.
- Models download only when asked, over HTTPS, from the revision of
  whisper.cpp's model repository `apps/koe/data/models.json` pins, and are
  checked by size and SHA-256 before they're used.
- The control socket is the user's alone (`$XDG_RUNTIME_DIR/aizome-koe/`,
  0700). No network server is exposed.

HTTP recognition is opt-in, in `koe.toml` only (`engine.backend = "http"`):
it sends a multipart WAV `file` and `model` (with `language`, `prompt` and
`translate` when set) to an HTTPS endpoint, or plain HTTP on this
computer, and expects `{"text": "…"}`. Redirects are refused. The
credential comes from the environment variable named by
`engine.http.api_key_env`, never from the file: give the service one with
`systemctl --user edit aizome-koe` (`[Service] EnvironmentFile=%h/…`). The
window then says the models aren't used.

## How it runs

`aizome-koe daemon` is the service, `aizome-koe.service`, part of
`aizome-session.target` (`apps/koe/data/aizome-koe.service`). The NixOS
module wants it in every Aizome session, and its `ExecCondition`,
`aizome-koe enabled`, starts it only for those whose `koe.toml` says
`enabled = true`: the switch. The module gives the unit PipeWire's
`pw-record` and `pw-play`, and `wl-copy`, which NixOS doesn't put on the
PATH of units it configures. The compositor imports `AIZOME_SOCKET` and
`AIZOME_SHELL_SOCKET` into the user manager before the session's units
start; Koe pastes, checks focus and posts notifications through them.

The service warms the model with a second of silence at start and keeps
its inference buffers, so the first dictation doesn't wait for them. The
systray menu opens the window through the compositor, so it isn't one of
the service's processes and outlives a stop.

Where things are:

| | |
|---|---|
| Settings | `~/.config/aizome/koe.toml` (`aizome-koe config show`; every field is in `apps/koe/data/koe.example.toml`) |
| Models | `~/.local/share/aizome/koe/models/ggml-NAME.bin` |
| Vocabulary and saved examples | `~/.local/share/aizome/koe/vocabulary/` |
| Control socket | `$XDG_RUNTIME_DIR/aizome-koe/control.sock` |
| The window's one instance | `$XDG_RUNTIME_DIR/aizome-app.koe.sock` |
| Shortcuts | `~/.config/aizome/settings.toml`, `[keys.bindings]` |

`aizome-koe` with no command opens the window (`--page vocabulary` for its
other page); a second launch hands its page to the window that's open.

## The command line

Everything the window does is also a command, for scripts and agents:

```sh
aizome-koe enable                  # turn dictation on (needs a model), add its shortcuts
aizome-koe disable
aizome-koe enabled                 # exit status only: the unit's condition
aizome-koe record toggle           # start; the next toggle finishes
aizome-koe record start|stop       # for a hotkey host with press and release
aizome-koe record pause|resume|cancel
aizome-koe record last|copy-last|paste-last|clear-last
aizome-koe status                  # one JSON status
aizome-koe watch                   # newline-delimited JSON as it changes
aizome-koe model list|download NAME|verify NAME|remove NAME|unload
aizome-koe transcribe speech.wav   # mono 16 kHz WAV to stdout, no clipboard
aizome-koe config show|check|init
aizome-koe doctor                  # what's set up and what's missing
aizome-koe packs list|show|create|edit|remove|import|export|enable|disable|export-examples
aizome-koe words add|edit|remove
aizome-koe examples list|delete
```

`record toggle` or `start` with dictation off opens Koe rather than doing
nothing. Vocabulary commands print JSON, work with or without the service,
and take a pack's ID or name and a word's ID or spelling; an open window
shows their changes within three seconds, keeping what's being typed.
Packs and words: `aizome-koe packs create "Desktop terms" --enable`,
`aizome-koe words add desktop-terms Fusen --alias "who sen"`,
`aizome-koe words edit sekka Aizome --add-alias Aizoma --remove-alias IZOMA`.
Exports never overwrite: they need new paths.

## The control protocol

A Unix stream socket, one UTF-8 JSON object per line, at most 64 KiB with
its newline. Each connection sends one request and gets one response; a
subscriber then gets each change until it disconnects. A lock file allows
one daemon; a stale socket is removed only after taking it. Connections
that send nothing time out after three seconds, blocked writes too, and
there are at most 64 connections and 32 queued commands.

```json
{"version":1,"action":"toggle"}
{"version":1,"ok":true,"status":{"phase":"recording","sequence":1,"elapsed_ms":0,"level":0.0,"has_last":false,"model_loaded":true,"transcription_ms":null,"error":null,"teaching":false,"gpu":true}}
```

Actions: `start`, `stop`, `toggle`, `pause`, `resume`, `cancel`, `status`,
`subscribe`, `last`, `copy-last`, `paste-last`, `clear-last`, `unload`,
`shutdown`, and `vocabulary` with a `vocabulary` command. Unknown actions
and fields are refused; a request without `version` is version 1, another
version is answered with an error.

- A refused command has `ok: false` and `error`. A failure later on
  (capture, recognition, delivery) shows as `status.phase = "error"` with
  `status.error`. `stop` returns once recognition is submitted; follow the
  stream for the rest. `paste-last` goes through `delivering` to `idle` or
  `error`, and only from idle or error.
- Phases: `idle`, `recording`, `paused` (microphone closed, speech kept),
  `finishing`, `transcribing`, `delivering`, `error`. `toggle` is accepted
  only idle or in error, or recording or paused; a start during
  transcription isn't queued.
- `sequence` grows with each published change and starts again with the
  daemon. `elapsed_ms` is captured audio, without pauses; `level` is RMS
  in 0..1 (not speech probability), at most ten times a second;
  `transcription_ms` is the last recognition's wall time; `gpu` says the
  loaded model runs on a graphics card rather than the processor. Read-only
  requests publish nothing. Slow subscribers get the latest state, not a
  queue: it's a display stream, not a log.
- Only `last` returns text. `cancel` throws away what's under way and keeps
  the last transcript.
- Vocabulary commands: `library`, `save-library` (a whole library with the
  revision read; stale ones are refused), `import`, `export`, `examples`,
  `delete-example`, `export-examples`, and teaching's `teach-start` (a
  random `token`, a `pack` and a `word`), `teach-stop`, `teach-poll`
  (`{heard, corrected, duration_ms}`), `teach-play`, `teach-save` (the
  complete corrected `transcript`) and `teach-cancel`, each with the same
  token. The token's owner polls at least every eight seconds, or the
  session is discarded with its audio; `status.teaching` is true
  throughout, and nothing else may start meanwhile. `VocabularyRequest`
  in `apps/koe/src/protocol.rs` has the fields.

Everything is local to the user; the protocol isn't a boundary between
the user's own processes.

## Vocabulary formats

An exported pack (`.koe.json`, at most 32 KiB):

```json
{"format": "koe-vocabulary", "version": 1,
 "pack": {"id": "my-desktop-terms", "name": "Desktop terms", "description": "Names used by our team",
          "words": [{"id": "aizome", "spelling": "Aizome", "aliases": ["Isome", "IZOMA"]}]}}
```

IDs are 1–64 ASCII letters, digits, `-` and `_`. A library holds at most 16
packs and 256 words, each word at most 16 aliases; written forms and
aliases are trimmed, nonempty, at most 128 bytes, without control
characters. An import of a pack that's already there unchanged is refused;
a changed one with the same ID comes in as a copy with a fresh ID and
"(imported)" after its name, never over local edits. Hints are at most
1024 bytes and half the model's text context; too many is an error, not a
silent cut. Longer aliases apply first, then `koe.toml`'s `replacements`;
conflicting active corrections are refused.

A training export is a new folder of `audio/ID.wav` (mono 16-bit PCM,
16 kHz, at most 30 seconds) and `manifest.jsonl`, one object per example:
`version`, `id`, `audio`, `audio_sha256`, `pack_id`, `word_id`, `spelling`,
`transcript` (the reviewed sentence), `heard` (before corrections),
`model`, `language`, `vocabulary_revision`, `created_ms`, `sample_rate`
and `duration_ms`. It's a starting dataset, not a trained model.

## Building and testing

`apps/koe` builds whisper.cpp through `whisper-rs-sys` with CMake (in the
dev shell). Features: `local` (whisper.cpp, the default), `openblas`,
`vulkan` and `cuda`; `--no-default-features` leaves only HTTP recognition.
The dev shell's build is the CPU one, with whisper-rs's own bindings
(`WHISPER_DONT_GENERATE_BINDINGS`); the package adds OpenBLAS (kernels
chosen for the processor at run time, the rest portable) and Vulkan, whose
bindings it generates and whose shaders it compiles (`nix/package.nix`).

```sh
cargo test -p aizome-koe
```

covers audio framing, configuration, the model catalog, bounded IPC, the
daemon through real sockets with a fixture microphone and a recognition
server on this computer (dictation, pause, cancel, focus guards, held
keys, teaching, settings applied without a restart, `enable` and
`disable`), the window (setting up with a test catalog and model server,
cancelled and damaged downloads, cancelling or closing while the server is
silent, settings saved as they change and while the switch works, an
unreadable `koe.toml` reset while the service records, teaching through
the Vocabulary page), and golden
images of the pages (`tests/snapshots`, `AIZOME_UPDATE_GOLDENS=1`).
`KOE_TEST_SCREENSHOTS=DIR cargo test -p aizome-koe --test look` also
writes the whole Dictation page. Nothing uses the session's microphone,
clipboard, keyboard or compositor. Ignored, by hand:

- `--test tray -- --ignored`: a private `dbus-daemon` and
  StatusNotifierWatcher, real DBusMenu clicks, re-registration after the
  host restarts, and Turn Off Dictation.
- `--test core local_whisper_recognizes_jfk -- --ignored`: real
  recognition of whisper.cpp's JFK sample (`KOE_TEST_MODEL`,
  `KOE_TEST_WAV`; `aizome-koe model download tiny.en --directory DIR`).
- `--test aizome -- --ignored`: a headless Aizome and foot, two real ❖⌥D
  presses, held keys and bracketed paste (`KOE_TEST_KOE` for another
  binary, `KOE_TEST_GPU=1`).

`just replace koe` restarts the window from the checkout. The service runs
the system's binary; to try a daemon from the checkout, stop the unit
(`systemctl --user stop aizome-koe`) and run
`target/release/aizome-koe daemon`.

## From the standalone Koe

Koe was its own repository and flake before it was one of Aizome's apps.
Moving over, once the system's Aizome has it: stop and disable the old
unit, whose link in `~/.config/systemd/user/aizome-session.target.wants`
points into the profile (`systemctl --user disable --now aizome-koe`),
remove the profile's package (`nix profile remove koe-vulkan`), then open
Koe and turn dictation on. Its settings,
models and vocabulary are where they were; turning it on moves shortcuts
bound to `spawn koe record …` to `aizome-koe`. The flake's
`programs.aizome.koe` options are gone: Aizome's module has the service,
and the window has the settings.

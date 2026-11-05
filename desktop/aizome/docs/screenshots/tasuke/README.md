# Assistant (Tasuke) screenshots

Original 1440 × 900 `grim` captures, at scale 1, of the Assistant window
(`aizome-tasuke`) on its launch fixtures, each in Night and Day. They are
not cropped or scaled, and nothing in them was edited; the files were only
recompressed losslessly with oxipng (checked pixel-identical with
ImageMagick's `compare`).

## What is real and what is made up

- **Real**: the window, its toolkit and its drawing, the model catalog
  (`apps/tasuke/catalog/models.toml`) and `tasuke-models`' fit and
  recommendation code. On the Models and first-run pictures, the
  recommendations, fit bars, scores, notes and speeds are what that code
  works out for the machines below.
- **Made up** (`desktop/aizome/apps/tasuke/src/fixture.rs`): the three
  machines (a laptop whose RTX 4060 with 8 GB drives its screen, with
  32 GB of memory; a desktop whose RTX 5090 drives the display, with
  64 GB; a Ryzen AI Max+ 395 whose Radeon 8060S shares 128 GB), the
  conversations and every command's output in them, and the notes in
  Memory. No computer's or person's data is shown.
- **Scripted**: Tasuke's agent loop isn't written yet, so the
  conversations are scripted event streams (`src/script.rs`) played into
  the same transcript the agent's events will fill. Earlier conversations
  were played to their end at launch, answering their cards Run; the two
  newest stop at their cards.
- The screenshot thumbnail in the *Switch to the Day theme* conversation
  is the Aizome gallery's own `docs/screenshots/07-day.png` (a capture of
  the Day desktop from 18 October 2026), standing in for what the agent's
  screenshot tool would capture. The window shrinks it as it would any
  screenshot.

The fixtures' clock is Thursday 5 November 2026, 21:30 in Chicago, so
the first page says "Good evening" and the sidebar's times count from
then.

## How they were made

- **Date**: 5 November 2026.
- **Build**: the dev profile (`cargo build -p aizome-tasuke`) at
  `41266f94` on `feat/assistant`, with the repository's Aizome dev shell.
- **Display**: a private headless sway 1.12 with the pixman renderer
  (`desktop/aizome/sukumo/scripts/headless-wayland`, `SIZE=1440x900`),
  with `WAYLAND_DISPLAY`, `DISPLAY` and `AIZOME_SHELL_SOCKET` unset. The
  window ran alone and tiled to fill the output; with no Aizome shell, it
  draws its own menu bar.
- **Theme**: a scratch `XDG_CONFIG_HOME` whose `aizome/settings.toml`
  says `[appearance] theme = "night"` or `"day"`.
- **Fonts**: `AIZOME_FONT_DIR` set to `nix build .#aizome-fonts` (Archivo,
  IBM Plex Mono, Noto Sans Symbols 2). The 助 in the sidebar came from
  the host's fontconfig (Noto Serif CJK JP).
- **Each picture**: `aizome-tasuke --fixture NAME`, captured 3 seconds
  after launch (`SHOT_AFTER=3`) and stopped. Nothing was clicked or typed.
  The *downloading* fixture's download is simulated and moves on while
  the window runs, so it had advanced about 3 seconds' worth.

## The pictures

| Files | Fixture | What it shows |
| --- | --- | --- |
| `night-home.png`, `day-home.png` | `home` | A new conversation: the greeting, the model's state, six suggestions across the kinds of help, and the composer with the Act and Desktop control switches and the model's chip; the sidebar lists earlier conversations by recency, two of them waiting on an answer |
| `night-diagnose.png`, `day-diagnose.png` | `diagnose` | *Why is my disk filling up?*: thinking folded to one line, six read-only steps folded into "Looked at 6 things", a Markdown answer with a table and a list, and a pending approval card for `nix-store --gc` with its purpose and the policy's reason |
| `night-root-approval.png`, `day-root-approval.png` | `root-approval` | *Clean up old system versions*: a card asking to run a command as administrator, saying a password prompt follows |
| `night-day-theme.png`, `day-day-theme.png` | `day-theme` | *Switch to the Day theme*, finished: an answered file-change card with its diff and "You chose Run · done", then the screenshot the agent took, as a thumbnail |
| `night-models-8gb.png`, `day-models-8gb.png` | `models-8gb` | Models on the 8 GB laptop: the hardware and the budget a model gets (less what the display keeps), and the Assistant role's recommendation, which fits only with part of it in system memory |
| `night-models-32gb.png`, `day-models-32gb.png` | `models-32gb` | Models on the 32 GB desktop: the recommendation in use, another downloaded with Use and Remove |
| `night-models-unified.png`, `day-models-unified.png` | `models-unified` | Models on the 128 GB unified-memory machine |
| `night-first-run.png`, `day-first-run.png` | `first-run` | First run on the 8 GB laptop: the hardware, the recommendation with its fit bar, Download with how long it takes at 100 Mbit/s |
| `night-downloading.png`, `day-downloading.png` | `downloading` | First run's download: progress, rate and time left, Pause and Cancel, and what's happening while it downloads |
| `night-memory.png`, `day-memory.png` | `memory` | Memory: the notes, when each was made, Remove, and that they're background, not instructions |

The Night and Day goldens in `desktop/aizome/apps/tasuke/tests/snapshots/`
are headless renders of the same fixtures by the kit's offscreen
renderer, at the window's own 1180 × 780 without a menu bar
(`cargo test -p aizome-tasuke --test look`), not window captures.

# Tasuke screenshots

Original 1440 × 900 `grim` captures, at scale 1, of Tasuke's window
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
- **Made up, for the cloud accounts**: the connected Anthropic account and
  its key's ends (`sk-ant-…7a3f`), the key typed into the Connect sheet
  (shown as dots), the list of models the account can use, and every timing
  ("74 tokens/s timed", "109 tokens/s timed"). A fixture's local timing is
  the catalog's estimate for that machine less 6%; its cloud timings are
  numbers chosen for the picture. No key was checked, no keyring was
  touched, and nothing was sent to Anthropic or OpenAI.
- **Scripted**: fixtures don't run Tasuke's agent or a model. Their
  conversations are scripted event streams (`src/script.rs`) played into
  the same transcript the agent's events fill, following the agent's
  skills (the check-up follows `apps/tasuke/skills/sekka-checkup`) and its
  messages (the server-down error is the agent's own wording for a refused
  connection). Earlier conversations were played to their end at launch,
  answering their cards Run; the two newest stop at their cards.
- The screenshot thumbnail in the *Switch to the Day theme* conversation
  is the Aizome gallery's own `docs/screenshots/07-day.png` (a capture of
  the Day desktop from 18 October 2026), standing in for what the agent's
  screenshot tool would capture. The window shrinks it as it would any
  screenshot.

The fixtures' clock is Thursday 5 November 2026, 21:30 in Chicago, so
the first page says "Good evening" and the sidebar's times count from
then.

## How they were made

- **Date**: 5 November 2026; the two Compare Models fixtures on 6 November 2026, with the cloud results in the catalog.
- **Build**: the dev profile (`cargo build -p aizome-tasuke`) on
  `feat/assistant`, with the repository's Aizome dev shell, at `58e13304`,
  when the window took Tasuke as its name. All twenty-four were taken
  again then. Against the earlier takes (from `41266f94` to `85a3e01b`),
  ImageMagick's `compare` finds changes only in the menu bar's app name,
  the heading that names the app and, on the Models pages, the first
  sentence. The six Models pictures were taken again at `9842393b`, when
  the catalog's notes gained each model's result inside Tasuke; against
  the takes at `58e13304`, `compare` finds changes only from the first
  card's notes down. The two unified-memory Models pictures were taken
  again when unified memory was held to the same 10 tokens/s floor as the
  processor: Muse Glimmer 30B no longer qualifies on the 8060S, and
  `compare` finds the change only in the "Also fits" count (11 to 10).
  The two check-up pictures were taken again when its memory, process
  and disk looks became one `system_usage`; `compare` finds the change
  only in the folded line's count (12 to 11). All twenty-four were taken again
  when every list came to rank on Tasuke's own results (once the last
  evaluated model, Muse Glimmer 30B, was measured inside Tasuke) and the
  32 GB desktop's second download became gpt-oss 20B, the fast model;
  sixteen were pixel-identical to the takes before and were kept, and in
  the Models and first-run pictures `compare` finds changes only in the
  score lines, the notes and which models lead "Also fits". The Models,
  first-run and downloading pictures were taken again when the catalog
  gained Gemma 4's draft heads: download sizes include the draft, and on
  the 32 GB desktop Gemma 4 31B's fit bar and speed include it; the other
  fourteen were pixel-identical and were kept.
  The four Compare Models pictures were taken with the release profile
  (`cargo build --release -p aizome-tasuke`) at `132f30eb`. The six
  Models pictures, whose header gained Compare All, were not taken again.
  On 6 November 2026, when Tasuke gained Claude and OpenAI accounts and
  Measure Speed, the eight cloud-account pictures were taken with the
  release profile, built from the working tree of the commit that adds
  them, and fourteen others were taken again from the same build. Against
  their earlier takes, `compare` finds changes only here: on the Models
  pictures, the header's first sentence; on first run, from the buttons
  down (Use a Cloud Account… and the sentence about it); on the first page,
  the column of text and suggestions, which moved a few pixels when the
  privacy line became wrapping text beside its icon; on Compare Models,
  from the header down, where the cloud rows now say where they run, the
  column is "Where it runs", and the 8 GB laptop's selected model has its
  timing line.
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
| `night-checkup.png`, `day-checkup.png` | `checkup` | *Give my computer a check-up*: eleven read-only looks (memory, busy processes and disks in one `system_usage`) folded into "Looked at 11 things", then the report in the check-up skill's format: the verdict, the problem and the warning (✕ and ⚠, in red and amber), one ✓ line of what's fine, and three offered fixes saying which need the administrator password, with Copy under the finished answer |
| `night-server-down.png`, `day-server-down.png` | `server-down` | A question asked while the model server is down: the agent's "Starting the model server…", then the error as a headline (*The model server can't be reached*) with its detail, Models and Retry; the chip says the server is stopped |
| `night-models-8gb.png`, `day-models-8gb.png` | `models-8gb` | Models on the 8 GB laptop: the hardware and the budget a model gets (less what the display keeps), and the Assistant role's recommendation, Gemma 4 26B-A4B, which runs with part of it in system memory: a second, thinner bar shows that side |
| `night-models-32gb.png`, `day-models-32gb.png` | `models-32gb` | Models on the 32 GB desktop: the recommendation in use, and gpt-oss 20B, the fast model, downloaded, with Use and Remove |
| `night-models-unified.png`, `day-models-unified.png` | `models-unified` | Models on the 128 GB unified-memory machine |
| `night-compare.png`, `day-compare.png` | `compare` | Compare Models on the 32 GB desktop: every model Sekka measured, sorted by faults fixed, with the four cloud models (measured in Claude Code and Codex) saying whose cloud they run in, each local model's placement and speed here (Gemma 4 31B's timed, the others estimated) and its download size; Gemma 4 31B is selected, its details below the fold |
| `night-compare-8gb.png`, `day-compare-8gb.png` | `compare-8gb` | Compare Models on the 8 GB laptop with "Only models this computer runs well": what fits with part of it in system memory and reaches 10 tokens/s, the cloud rows still there, Gemma 4 26B-A4B selected with its Download and, under its figures, that downloading it lets it be timed here |
| `night-compare-cloud.png`, `day-compare-cloud.png` | `compare-cloud` | Compare Models with an Anthropic account connected: Claude Sonnet 5.5 selected, with its made-up timing in the table ("Cloud · 74 tokens/s timed") |
| `night-connect.png`, `day-connect.png` | `connect` | First run on the 8 GB laptop with Use a Cloud Account… pressed: the Connect sheet, Claude chosen, what using it means, the link to Anthropic's key page, a made-up key in the password field, Cancel and Connect |
| `night-connected.png`, `day-connected.png` | `connected` | The sheet after the key checked out: the models the account can use, and that Tasuke now uses Claude Sonnet 5.5; behind it, the first page with the chip and privacy line saying it's Anthropic's cloud, and the sidebar "with Anthropic" |
| `night-cloud-home.png`, `day-cloud-home.png` | `cloud-home` | The first page while Claude Sonnet 5.5 is in use, with earlier conversations: where it runs, and what is sent to whom |
| `night-first-run.png`, `day-first-run.png` | `first-run` | First run on the 8 GB laptop: the hardware, the recommendation (Gemma 4 26B-A4B) with both its fit bars and its incident from the evaluations, Download with how long it takes at 100 Mbit/s, and Use a Cloud Account… with what a cloud account means |
| `night-downloading.png`, `day-downloading.png` | `downloading` | First run's download: progress, rate and time left, Pause and Cancel, and what's happening while it downloads |
| `night-memory.png`, `day-memory.png` | `memory` | Memory: the notes, when each was made, Remove, and that they're background, not instructions |

The Night and Day goldens in `desktop/aizome/apps/tasuke/tests/snapshots/`
are headless renders of the same fixtures by the kit's offscreen
renderer, at the window's own 1180 × 780 without a menu bar
(`cargo test -p aizome-tasuke --test look`), not window captures.
Three are scrolled to what the window shows below the fold:
`compare-details-*` and `compare-cloud-details-*` (the selected model's
details, with Remove or Use and its timing line with Measure
Again) and `models-accounts-*` (Models' cloud accounts, Anthropic
connected with Claude Sonnet 5.5 in use).

## From the VM tests

Three 1280 × 800 captures by the NixOS test driver (`machine.screenshot`)
of a VM's virtio-gpu display, running Aizome with alice logged in, taken
on 5 November 2026 on wx-workstation and recompressed losslessly as above.
Unlike the pictures above, nothing in them comes from a fixture: the
window runs its real agent and model library, Aizome's shell draws the
password dialog, the download is real, and so is the last one's model.
What stands in is said below.

| File | Test | What it shows |
| --- | --- | --- |
| `vm-password-dialog.png` | `checks.x86_64-linux.tasuke-dialog`, built at `58e13304` | After **Run as administrator**, Aizome's own polkit dialog asking for alice's password (`test-only`, typed next). The model is a script (`apps/tasuke/nix/fake-openai.py`); "Gemma 4 12B" names sparse stand-in files, and no model ran |
| `vm-download-paused.png` | `legacyPackages.x86_64-linux.tasuke-download`, built at `58e13304` | First run's download from Hugging Face, paused at 372 MB of Qwen3.5 9B's 6.1 GB; first run recommended it for a stand-in `nvidia-smi`'s 8 GB laptop GPU and the VM's 3.8 GB of memory |
| `vm-live-answered.png` | `legacyPackages.x86_64-linux.tasuke-live`, added with this picture | A question answered by a real model: Gemma 4 26B-A4B, served by `tasuke server start` on wx-workstation (the VM's host) as it would be for an 8 GB laptop GPU, reached through a forwarded port. The two commands ran in the VM, and the answer's figures match what they printed. The model's files in the VM are sparse stand-ins, so the chip's "on this computer" names the host's model. The test's second run; the model was already loaded |

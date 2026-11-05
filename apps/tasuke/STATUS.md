# Tasuke: status (2026-11-05, 01:00 CT)

What exists, what is verified, and what is next. Read [DESIGN.md](DESIGN.md) first; it is the contract between the pieces. Delete this file, or fold it into DEVELOPERS.md, before the branch is merged.

Branch `feat/assistant`, worktree `/home/callen/work/sekka-assistant`. It is stacked on `feat/open-model-evals` (PR #292) at 9c1a50bc and has not been pushed. Work stopped mid-way when a usage limit ended every subagent at about 00:40. The pieces were then committed as they stood; each of those commits says what is missing.

## Building and testing

The host's rustup is broken, so use the repo's dev shells. Their environments are saved under `.work/` in this worktree, and the `.work/devshell-*` out-links root them against GC.

```sh
# apps/tasuke (its own workspace)
bash -c 'source /home/callen/work/sekka-assistant/.work/devshell-default.sh >/dev/null 2>&1; cd /home/callen/work/sekka-assistant/apps/tasuke && cargo test -j 6'
# the window (Aizome's workspace)
bash -c 'source /home/callen/work/sekka-assistant/.work/devshell-aizome.sh >/dev/null 2>&1; cd /home/callen/work/sekka-assistant/desktop/aizome && cargo test -p aizome-tasuke -j 6'
```

At bb0bb3a3, `apps/tasuke` builds with 35 tests passing, and `aizome-tasuke` builds with 26 tests passing. `aizome-tenki` and `aizome-yukimi` still check after Aizome's lock moved ureq from 3.4.2 to 3.5.1.

## Pieces

| Piece | State |
|---|---|
| `skills/` | **Done.** Seven skills, each checked against the source; `README.md` lists the files checked and has a 248-word always-on prompt. |
| `catalog/models.toml` | **Written.** 15 GGUF models pinned with sha256 and size, and curated defaults for budgets of 26 GiB and up. The six 8 GiB-tier entries are `evaluated = false` until the 8 GiB round is reported. Its header points to `catalog/README.md`, which does not exist yet. |
| `tasuke-models` | **Mostly written, unit-tested** (21 tests). `hardware` detects runnymede's RTX 5090 (32 GB, drives the display) and Radeon iGPU in 22 ms. Also written: `catalog`, `fit`, `recommend`, `download` (Range resume, streaming sha256, per-read timeout), `store`, `config`, `units`, and `server` (router mode, where `tasuke server exec` writes the presets and execs llama-server). **Not verified:** the router on a real GPU, idle sleep freeing VRAM, and a real download. |
| `tasuke-cli` | **Skeleton only.** The `models.rs` subcommands (hardware, recommend, models, download, remove, use, server) and `ask.rs` are still to write. |
| `tasuke-agent` | **Public API committed** (`Agent::start` → `TurnHandle`, events, sessions, memory, skills). `llm.rs` (streaming chat client, about 800 lines) and `textcalls.rs` exist but are not yet declared in `lib.rs`. `turn.rs` is a stub that reports "the agent loop is not written yet". **To write:** the loop, the tools, `policy.rs` (default-deny classifier with adversarial tests), the context trimming, and the eval integration (`run.py --agent tasuke`). |
| `tasuke-helper` | **Empty.** The pkexec runner and polkit policy template are still to write (`apps/yukimi/yukimi-helper` and `desktop/aizome/apps/nisshi` show how). |
| `desktop/aizome/apps/tasuke` | **View model and widgets** (26 tests): transcript folding, approval state, scroll-follow, composer, prose, the Markdown parser. `main.rs` and `markdown/view.rs` are empty. **To write:** the app shell, the conversation and Models/Memory pages, first run, launch fixtures, and screenshots. |
| `nix/` | **Empty.** The package, the `programs.tasuke` module, the user unit `tasuke-llm.service`, the polkit action and a VM test are still to write. |

## Findings that change the design

- **Desktop control.** The desktop tools need a set-of-marks mode and per-axis coordinates.
  - Gemma 4 31B failed the computer-use evals by clicking about 200 px off target (theme-day: 30 clicks on Night while aiming at Day). Its error isn't affine: 158 px on average, 114 px after a per-axis fit.
  - Gemma 4 12B's x is in a roughly 0–1000 frame and its y in pixels. A per-axis affine fit cuts its error from 214 to 37 px.
  - `tests/agent-evals/grounding.py` misses both effects: its three targets sit at the screen's edges.
  - So:
    - Draw numbered element boxes from kit inspection and offer `click_element {id}`.
    - Support `coords = { x = [a, b], y = [c, d] }` per model.
    - Calibrate on mid-screen targets.
  - The measuring script is `/tmp/claude-1000/-home-callen-work-sekka/931b1901-9f14-472e-9bda-f0fa9320a560/scratchpad/g2/grounding2.py`, with a copy on wx at `~/work/g2/`. Its targets were read off the screenshot by hand.
- **Settings and the desktop.** The skills' check of the source found these. DESIGN.md should follow them.
  - The subcommand is `aizomectl recovery restart-shell`.
  - Python isn't installed on Sekka systems.
  - `aizomectl reload-config` reports parse errors in settings.toml.
  - Settei keeps a broken settings.toml as `settings.toml.broken`.
  - With Genkan's Alt/Super swap, `aizomectl input` chords may come out swapped. Prefer `aizomectl action NAME` and `aizome-shell msg`.

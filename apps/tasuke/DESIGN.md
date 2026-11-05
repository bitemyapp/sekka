# Tasuke 助 — Sekka's local assistant: design

Status: v1 design, November 2026. Tasuke is the code name; people see **Assistant**.

Tasuke is a private assistant that runs entirely on the user's machine. It helps with three kinds of request:

1. **Conversation**: general questions, writing and explanation. No tools are needed.
2. **Help with Sekka and Aizome**: how to do something ("how do I switch desks?"). It answers with exact steps and offers to do them.
3. **Doing it**: diagnosing and fixing faults ("my sound stopped working"), maintenance ("free some disk space") and operating the desktop ("switch to the Day theme"). It inspects freely and changes nothing without asking.

The design follows the agent evaluations in `tests/agent-evals` (docs/agent-evals/README.md). Their bare tool loop is what Tasuke's agent is, and the same scenarios check it (`run.py --agent tasuke`).

## Principles

- **Private and local.** The model runs in llama.cpp on this machine. Nothing leaves it unless the user asks for something that does (a download, a web page).
- **Trustworthy before clever.**
  - Inspection is automatic. Anything that changes the system needs the user's approval, shown as the exact command with the model's stated purpose.
  - Root needs approval plus polkit authentication.
  - Never delete user data or end the session.
  - The policy that classifies commands is default-deny: a command runs without asking only if every part of it is on a read-only allowlist.
- **What enters the context is ours and correct.**
  - The system prompt is short and stable, so llama.cpp's prefix cache hits.
  - Skills are verified against the source and loaded on demand.
  - The environment block is gathered deterministically by Rust, not by the model.
  - Memory is shown to the user and can be removed. It is framed as notes, not instructions.
- **Fast without taking over the machine.**
  - One llama-server in router mode, as a systemd user unit at low CPU and IO weight.
  - Models load on first use and unload after a period of idleness (`--sleep-idle-seconds`), so a game or a render gets the GPU back.
  - The window never blocks: inference, commands and downloads run off the UI thread.
- **Fits the hardware.** Recommendations come from measured evaluation results and the machine's actual GPU memory, unified memory or system RAM.

## Pieces

```
apps/tasuke/                  Cargo workspace, independent of Aizome's
  crates/tasuke-models/       hardware, catalog, recommendations, downloads, model server
  crates/tasuke-agent/        LLM client, agent loop, tools, command policy, skills, memory, sessions
  tasuke-cli/                 `tasuke`: the headless interface (also what the eval VM runs)
  tasuke-helper/              `tasuke-helper`: runs one approved command as root, via pkexec
  catalog/models.toml         the curated, eval-backed model catalog (sha256-pinned files)
  skills/                     SKILL.md knowledge files, loaded on demand
  nix/                        package and NixOS module (programs.tasuke)
desktop/aizome/apps/tasuke/   `aizome-tasuke`: the native window (org.aizome.Tasuke)
```

The libraries are UI-independent, as with Yukimi (`apps/yukimi` holds the libraries and the helper, `desktop/aizome/apps/yukimi` the window). The window depends on `tasuke-models` and `tasuke-agent` by path.

## Files and processes

| What | Where |
|---|---|
| Settings | `~/.config/tasuke/config.toml` (`[model] id, context`, `[server] port, idle_minutes`, `[agent] act, desktop_control`) |
| Downloaded models | `~/.local/share/tasuke/models/<model-id>/<file>`. `<file>.partial` while downloading, renamed once its sha256 matches. |
| Router presets | `~/.local/share/tasuke/models.ini`, written by `tasuke-models` from the catalog and the downloaded files |
| Conversations | `~/.local/share/tasuke/sessions/<id>.jsonl`: append-only, one event per line, crash-safe. The title and timestamps come from the first lines. |
| Memory | `~/.local/share/tasuke/memory.toml` (`[[note]] id, text, created`), at most 40 notes and 4,000 characters in the prompt |
| API key | `$XDG_RUNTIME_DIR/tasuke/api-key` (0600), created by the unit's ExecStartPre. Clients read it. |
| Model server | user unit `tasuke-llm.service`: `llama-server --models-preset … --models-max 1 --host 127.0.0.1 --port 8737 --api-key-file … --sleep-idle-seconds N`; `Nice=10`, `CPUWeight=20`, `IOWeight=20`, `OOMScoreAdjust=500` |
| Root helper | `tasuke-helper run` through pkexec, polkit action `io.github.bitemyapp.Tasuke.run-as-root` (`auth_admin_keep`). It takes one JSON request on stdin, enforces a timeout and an output cap, and logs the command to the journal. |

## The agent (`tasuke-agent`)

### Loop

- Use the OpenAI-compatible chat API with streaming (SSE) and native tool calls.
- Fall back to text tool calls when the model writes them in its content, ported from `tests/agent-evals/harness/agent.py` (`text_tool_calls`).
- Recover from malformed calls by telling the model and retrying, up to 3 times.
- `finish_reason == "length"` ends the turn with a visible notice.
- A turn ends when the model answers without tool calls.
- Each user turn has a step budget (default 40 tool calls) and can be cancelled.

### Context

- **System prompt:** role and rules (≤ 300 words), then the skill index (name plus description), then memory notes.
- **Environment block:** the first user turn of a session also carries a block gathered by Rust: hostname, user, Sekka and NixOS version, desktop, GPU and RAM, time zone, local time.
- **Trimming:**
  - Keep the last 2 screenshots and the last 4 tool outputs in full. Older ones become a one-line stub.
  - Above 75% of the model's context (`n_ctx` from the server's `/props`, else the catalog), older turns are summarized by the model into one "Earlier in this conversation" note.

### Tools and approval

`run_command` runs a command as the user; `run_as_root` runs one as root. Both take `{command, purpose}`.

| Tool | Runs as | Approval |
|---|---|---|
| `run_command {command, purpose}` | the user, `bash --noprofile --norc -c` in the home folder (no `BASH_ENV`, `ENV` or exported functions), no stdin, timeout 60 s (max 300), output capped at 32 KB (head and tail) | none if the policy classifies it read-only; otherwise ask |
| `run_as_root {command, purpose}` | root, through tasuke-helper | always ask, then polkit |
| `read_file {path, offset?, limit?}` | the user | none, except for sensitive paths (keys, keyrings, browser profiles, password stores) |
| `write_file {path, content}`, `edit_file {path, old, new}` | the user | always ask, showing a diff |
| `load_skill {name}` | — | none |
| `remember {text}`, `forget {id}` | — | none; shown in the transcript and the Memory list |
| `screenshot {}` | Aizome (`aizomectl screenshot`), scaled to at most 1600 px across, controls numbered (set-of-marks) | needs Desktop control on; vision models only |
| `list_windows {}`, `ui_elements {}` | Aizome (`aizomectl --json windows`, kit inspection files when present) | needs Desktop control on |
| `click_element {mark}`, `type_into {mark, text}` | Aizome, at the centre of control [mark] from the last look | needs Desktop control on; asks with the other actions |
| `click {x,y,button?}`, `double_click`, `drag`, `scroll`, `type_text {text}`, `key {keys}`, `open_app {name}`, `wait {seconds}` | Aizome (`aizomectl input …`, palette or desktop entries) | needs Desktop control on; the first action of a turn asks once |

**Modes.** A conversation's composer has two toggles:

- **Act** (default on): when off, Tasuke only inspects and explains. State-changing tools are not offered to the model at all.
- **Desktop control** (default off until the user turns it on).

**Policy** (`policy.rs`). The command is split on `|`, `&&`, `||` and `;`. It needs approval if any of these hold:

- it contains a redirection, `$(`, a backtick, `<(`, a background `&` or an environment assignment;
- it uses `sudo`, `pkexec`, `su` or `doas`;
- any segment's program is not on the read-only allowlist, or uses a subcommand or flag outside the allowlist's per-program rules. For example, `systemctl` is allowed only with status/show/is-*/list-*/cat, and `find` never with -delete/-exec/-ok/-fprint.

Sensitive read paths need approval too. The policy has exhaustive unit tests; when in doubt it asks.

### Events (the GUI and the CLI consume the same stream)

```rust
pub enum Event {
    TurnStarted { turn: u32 },
    ReasoningDelta { text: String },
    TextDelta { text: String },
    ToolCall { id: String, call: ToolCall, needs: Approval },  // Approval::None | Ask { reason } | AskRoot { reason }
    ToolStarted { id: String },
    ToolOutput { id: String, output: ToolOutput },            // text (capped), exit code, optional image path
    MemoryChanged,
    Notice { level: Level, text: String },                    // context trimmed, step budget hit, model loading…
    TurnFinished { reason: FinishReason, usage: Usage, decode_tps: Option<f32> },
    Error { text: String, retryable: bool },
}
```

`Agent::start(session, user_text, options) -> TurnHandle` runs the turn on its own thread. `TurnHandle` has `events: Receiver<Event>`, `decide(id, Decision::{Allow, Deny { note }})`, `cancel()` and a waker hook, so the window redraws when events arrive. Sessions persist every event as it happens, so a crash loses at most the token being streamed.

## Models (`tasuke-models`)

**Hardware.**

- GPUs:
  - NVIDIA: `nvidia-smi --query-gpu=…`, falling back to `/proc/driver/nvidia`. A GB10-class device reports no dedicated memory, so it is unified.
  - AMD: sysfs `mem_info_vram_total` and `mem_info_gtt_total`. An APU is unified.
  - Intel: unified unless the driver reports VRAM.
- Which card drives a connected display: `/sys/class/drm/card*-*/status`.
- RAM from `/proc/meminfo`; CPU cores and AVX2/AVX-512 from `/proc/cpuinfo`.

**Catalog.**

- **Each entry:** id; name; maker and lineage; licence; quant; files (repo, revision, path, size, sha256); mmproj; architecture (dense, or MoE with total and active parameters); KV bytes per token; default context; llama.cpp args, with per-tier overrides such as `-ncmoe`; vision and its coordinate convention; measured eval scores and speeds with their date.
- **Roles:** assistant, desktop, fast.
- **Notes:** honest notes, including incidents seen in the evaluations.

**Fit.**

- A model needs weights + KV(context) + compute buffer.
- It fits on a GPU when that is ≤ 92% of the GPU's free memory. A GPU that drives a display keeps 1.5 GiB back.
- An MoE can fit with its experts in RAM (`-ncmoe`) when the dense part fits on the GPU and the experts fit in available RAM.
- On unified memory the budget is RAM minus 8 GiB.
- When nothing fits on a GPU, it runs on the CPU, recommended only for small MoEs.

**Recommendations.**

- Per role, rank the fitting models by measured score, then by trust (no destructive incidents) and speed.
- Show maker and lineage. A user setting can hide model families by origin.
- The curated default per tier is stated in the catalog, with the reason.

**Downloads.**

- `https://huggingface.co/<repo>/resolve/<revision>/<path>` with HTTP Range resume.
- Hash the sha256 while downloading; check free disk space first.
- Run at low IO priority, one at a time, cancellable, with progress events.

**Server.**

- Write `models.ini`, then start, stop or restart `tasuke-llm.service` through `systemctl --user`.
- Status comes from `/health`, `/models` and `/props`.
- The CLI's `--foreground` runs llama-server directly, for machines without the unit.

## CLI (`tasuke`)

```
tasuke hardware [--json]
tasuke recommend [--role assistant|desktop|fast] [--json]
tasuke models [--json]                      catalog with fit and download state
tasuke download ID | tasuke remove ID | tasuke use ID
tasuke server start|stop|status [--foreground]
tasuke ask [--act] [--desktop] [--approve never|ask|all] [--base-url URL --model NAME] "request"
```

`ask` streams the turn to the terminal, asking approvals on the TTY. The eval VM runs it as root with `--approve all --base-url http://10.0.2.2:PORT`, so the same scenarios grade the product's agent. As root, `run_as_root` runs directly, without pkexec.

## The window (`aizome-tasuke`)

The window follows Omperor's principles (`~/work/omperor-aizome/docs/PRODUCT_DIRECTION.md`): conversation at the centre, evidence inline and inspectable, nothing modal except short decisions, and state retained when switching.

- **Sidebar:** New conversation; conversations by recency; Models; Memory.
- **Transcript:**
  - Markdown answers.
  - Each tool call is a compact row (verb and command or target, then its state), expanding to the full output.
  - Approval cards are inline. They show the command in monospace, the purpose, and why approval is needed. The buttons are **Run** / **Run as administrator** and **Don't run**, with no default button.
  - Screenshots show as thumbnails.
- **Composer:** the Act and Desktop control toggles. The model chip shows its state: loading, ready, or asleep (it loads on send), plus speed. Stop is there while a turn runs.
- **Models:**
  - The hardware summary.
  - Recommended models per role, with fit, size, speed estimate, scores, maker and lineage, licence and notes.
  - Download (with progress, pause and cancel), Use, and Remove.
  - The server's state and its idle-unload setting.
- **First run:** detect the hardware, recommend a model, download it, and say hello. While it downloads, the window explains what is happening and how long it will take.

## Since the first draft (2026-11-05)

- **Set-of-marks is built.** Every look at the screen (the first comes with the request) lists the windows and numbers each control kit apps report; vision models also get the screenshot with the numbers drawn on it. `click_element`/`type_into` take a number; x/y clicks go through the model's coordinate convention (`Coords`, including per-axis affine).
- **Acting, not asking twice.** The prompt says the approval card is the person's yes, and to retry with `run_as_root` when the person's permissions fall short. Skills write root commands as `sudo CMD`; each says that means `run_as_root` in Tasuke.
- **Robustness.** A reply that turns into a loop (one short unit repeated through the last 1,200 bytes) is cut off and resampled once; Gemma 4 12B otherwise repeats its channel markers until the token limit. Connection retries back off 1–8 s, and a turn starts `tasuke-llm.service` when Tasuke's own port is dead.
- **Routines.** `tasuke checkup [--notify]` runs the read-only `sekka-checkup` skill with Act off; the module's opt-in weekly timer notifies only when something needs attention.
- **The helper** is installed in `libexec` and found through `tasuke_agent::root::find_helper` (the path the polkit action names).
- **Evaluation.** `run.py --agent tasuke` grades this agent in the VM; when the turn ends on a question, the harness answers "Yes, please go ahead." once and keeps the first turn's result apart.

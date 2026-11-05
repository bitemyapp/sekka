# Tasuke on the evaluation scenarios

How Sekka's own assistant does on the same held-out scenarios as the open-model evaluations ([README.md](README.md)), measured on 2026-11-05 with `run.py --agent tasuke`. Everything here grades the product: Tasuke's agent loop, its short system prompt, its skills loaded on demand, its default-deny command policy and its desktop tools, with the model unchanged.

## How it was run

- `tasuke ask` runs inside the evaluation VM as root, with its commands as alice (`--user-prefix as-alice`) and `run_as_root` through `tasuke-helper` as root, as on a real machine where alice approves. Every approval was granted.
- A conversation, not a task harness: there is no `finish` tool. When Tasuke ends its turn with a question (it asks before risky changes, and smaller models ask more), the harness answers once, "Yes, please go ahead." The result after the first turn is kept apart.
- Diagnostics: the 16 held-out faults, twice each. Computer use: the 9 desktop tasks, twice with set-of-marks (`--marks`: kit apps' controls numbered on the screenshot and in a list, clicked with `click_element`) and once without. Tasuke offers its shell and file tools on desktop tasks too; a screen-only run (`--desktop-only`) measures operating the screen alone.
- Same servers as the evaluations: llama.cpp, 32K context per slot, q8_0 KV, the models' own sampling settings; 4–6 episodes at a time on an RTX 5090 (Gemma) or an RTX PRO 6000 (Nemotron, Qwen).
- Episodes whose test-driver shell lost its framing ("Invalid base64-encoded string") are excluded and counted.

## Results

| Model | Made by (lineage) | Diagnostics in Tasuke | …first turn | Desktop in Tasuke, with marks | Desktop, no marks | Bare loop: diagnostics, desktop | Damage in Tasuke |
|---|---|---:|---:|---:|---:|---:|---|
| Gemma 4 31B | Google | **28/32 (88%)** | 28/32 | **18/18** | 9/9 | 15/16, 2/9 | none |
| Qwen3.8-27B | Alibaba | 27/32 (84%) | 26/32 | **18/18** | – | 30/32, 16/18 | once: after a "yes" to its own suggestion, moved alice's older backup |
| Gemma 4 26B-A4B | Google | 24/31 (77%) | 15/31 | 17/18 | 7/9 | 14/16, 5/9 | none |
| Nemotron 3.5 Bolt | NVIDIA | 22/31 (71%) | 22/31 | 15/18 | – | 14/16, 7/9 | none |
| Gemma 4 12B | Google | 20/31 (65%) | 9/31 | 11/18 | 5/9 | 3/16¹, 3/9 | none |

Counts are successes over valid episodes: the fault fixed (or the task done) with no invariant broken. "First turn" counts those fixed before the harness's one "yes". The bare-loop columns are the open-model evaluations' (2026-11-04 and the 8 GiB round), screen-only for the desktop. Two Gemma 4 31B and two Qwen3.8 episodes of `notes-owned-by-root` were rerun after a policy fix (below), and the desktop runs of Gemma 4 12B and Nemotron 3.5 Bolt were rerun on the build that numbers open menus; the reruns replace the first runs.

¹ Repetition loops end the bare loop; Tasuke cuts them off and samples again.

**Gemma 4 31B is the best Tasuke model, and it is Google's.** It matches Qwen3.8-27B, the evaluations' strongest model, on diagnostics (88% to 84%) and on the desktop (18 of 18 each), solved every fault it solved on the first turn, and harmed nothing in any of its episodes. Its median fault took 95 seconds on the RTX 5090.

**Set-of-marks closed Gemma's desktop gap.** In the screen-only harness Gemma 4 31B did 2 of 9 tasks: it named the right control and clicked about 160 px away. With Tasuke's numbered controls it did all 18, a median 64 seconds and 5 actions each. Gemma 4 26B-A4B went from 5 of 9 to 17 of 18 and Gemma 4 12B from 3 of 9 to 11 of 18.

**Tasuke did its desktop tasks by whatever route works.** It offers its shell and file tools on desktop tasks as well as the screen, and models used them: of Gemma 4 31B's 18, 8 were done through the screen alone (every task that needs an app: Calculator, Files, Logs, Terminal), 9 by commands or by editing settings.toml, 1 both; Qwen3.8 did 12 of 18 by commands alone and 1 through the screen alone.

**Through the screen alone** (`--desktop-only`, one run of the nine tasks), set-of-marks lifted Gemma 4 31B from 3 of 9 to 7 of 9, level with Qwen3.8-27B (7 of 9 with marks). Qwen3.8 points well by itself (8 of 9 without marks); Gemma does not, and numbered controls are what close the gap.

| Through the screen alone | With marks | Without marks |
|---|---:|---:|
| Gemma 4 31B | 7/9 | 3/9 |
| Qwen3.8-27B | 7/9 | 8/9 |

**What lowered diagnostics in Tasuke against the bare loop.** The bare loop is a root shell with a fix-it prompt; Tasuke is an assistant that runs as the person, asks before risky changes and needs `run_as_root` for the system. Three faults defeated most models in both (`dns-nameserver`, `port-conflict`, `runaway-indexer`). Smaller models end on "Would you like me to…?" more often (Gemma 4 12B in 25 of its 77 episodes, Gemma 4 26B-A4B in 22 of 59, Gemma 4 31B in 4 of 63), which in a real conversation is one more click.

## What the evaluation changed in Tasuke

Each was found in these runs, fixed, and rerun where it mattered:

- **A false refusal.** The policy's recursive chmod/chown refusal read chown's owner (`alice:users`) as a path; from `/`, where the root helper checks, that is a top-level folder, so `chown -R alice:users /home/alice/Notes` was refused. Owner, mode and group words are now skipped.
- **A model reporting a fix that never ran.** After that refusal Gemma 4 31B told alice her notes were fixed. A refused step's result now begins "Not run: Tasuke refused it", and the prompt says to report only what happened.
- **Asking twice.** Gemma 4 12B asked in words before using a tool whose approval card asks anyway; the prompt now says the card is the person's yes.
- **Permissions.** Models told alice to run `systemctl start` herself after "Access denied … interactive authentication required"; the prompt now says to retry such commands with `run_as_root`.
- **Repetition loops.** Gemma 4 12B's channel markers repeated until the token limit; the client now cuts a reply off when one short unit fills its last 1,200 bytes and samples again.
- **Open menus.** A dropdown opens a menu whose place kit doesn't report, so its items had no numbers; Tasuke now places it from where the pointer is inside it.

None of these changes was tuned to a particular scenario's answer, and the skills were not edited in response to these held-out failures.

## The LoRA round

Could a light fine-tune make Gemma better at Sekka's faults without teaching it the test? Training data came only from Gemma: Gemma 4 31B's own successful, clean episodes on 32 training scenarios that share no fault, unit, file or setting with the 25 held-out ones (`tests/agent-evals/TRAINING.md`), in the bare loop. 17 conversations passed the filter (fixed, nothing harmed, every call well formed, no nudges); 16 fit in 16K tokens. Adapters: QLoRA rank 16, alpha 32, learning rate 1e-4, three epochs at an effective batch of 2 (24 steps), with the teacher's reasoning kept (`--enable-thinking`). Training loss went from 0.67 to about 0.5, so the models were nudged, not drilled. Each adapter was served with `--lora` on the same GGUF the evaluations used and compared with the base on the same server settings.

| Held-out scenarios | Base | With the adapter |
|---|---:|---:|
| Gemma 4 12B, bare loop, 16 faults × 2 | 4/32 | **17/32** |
| Gemma 4 12B in Tasuke, 16 faults × 2 | 20/31 | 19/31 |
| Gemma 4 12B in Tasuke, desktop with marks, 9 tasks × 2 | 11/18 | 13/18 |
| Gemma 4 31B, bare loop, 16 faults × 2 | 28/32 | 27/32 |
| GSM8K, 100 problems, no thinking: Gemma 4 12B | 97 | 96 |
| GSM8K, 100 problems, no thinking: Gemma 4 31B | 99 | 99 |

General ability is GSM8K's grade-school problems (`tests/agent-evals/regression.py`), the same 100 for every model, without thinking: with thinking on, half of Gemma 4 12B's answers didn't finish inside 16K tokens, which measures its looping, not its reasoning.

**The adapter fixed Gemma 4 12B's loops, which Tasuke already fixes another way.** In the bare loop, which has no defence against a reply that repeats its channel markers until the token limit, the adapter took the 12B from 4 to 17 of 32: it learned the shape of a clean tool turn from 16 examples. Inside Tasuke, which cuts such replies off and samples again, the base and the adapter are level on diagnostics (20 and 19 of 31), and the adapter's 13 of 18 desktop tasks against 11 is within what a second run can change, though it saw no desktop examples. Gemma 4 31B, already reliable, didn't move (28 and 27 of 32), and neither model lost general ability (GSM8K 97 against 96, 99 against 99).

The adapters are not shipped: they don't beat the base model inside the product. If they are wanted later, they are `~/work/lora/out/gguf/sekka-{12b,31b}-r16.gguf` on wx-workstation (about 240 MB and 450 MB), ready for the R2 bucket.

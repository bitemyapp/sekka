# Tasuke on the evaluation scenarios

How Sekka's own assistant does on the same held-out scenarios as the open-model evaluations ([README.md](README.md)), measured on 2026-11-05 with `run.py --agent tasuke`. Everything here grades the product: Tasuke's agent loop, its short system prompt, its skills loaded on demand, its default-deny command policy and its desktop tools, with the model unchanged.

## How it was run

- `tasuke ask` runs inside the evaluation VM as root, with its commands as alice (`--user-prefix as-alice`) and `run_as_root` through `tasuke-helper` as root, as on a real machine where alice approves. Every approval was granted.
- A conversation, not a task harness: there is no `finish` tool. When Tasuke ends its turn with a question (it asks before risky changes, and smaller models ask more), the harness answers once, "Yes, please go ahead." The result after the first turn is kept apart.
- Diagnostics: the 16 held-out faults, twice each. Computer use: the 9 desktop tasks, twice with set-of-marks (`--marks`: kit apps' controls numbered on the screenshot and in a list, clicked with `click_element`) and once without. Tasuke offers its shell and file tools on desktop tasks too; a screen-only run (`--desktop-only`) measures operating the screen alone.
- Same servers as the evaluations: llama.cpp, 32K context per slot, q8_0 KV, the models' own sampling settings; 4–6 episodes at a time on an RTX 5090 (Gemma) or an RTX PRO 6000 (Nemotron, Qwen).
- The other ten evaluated models ran that afternoon on the RTX PRO 6000 with the same Tasuke build: each on the faults if its role is assistant or fast, and on the desktop tasks with marks if it has vision, six episodes at a time, three models' servers sharing the GPU with a training-data run. Sharing slows a model, not its answers, except where an episode runs into its time limit: Muse Glimmer, the slowest, decoded at 3 tokens/s and timed out in 15 of 32 faults, so its faults were run again that evening with the GPU to itself; that run is the one shown.
- Episodes whose test-driver shell lost its framing ("Invalid base64-encoded string") are excluded and counted.

## Results

| Model | Made by (lineage) | Diagnostics in Tasuke | …first turn | Desktop in Tasuke, with marks | Desktop, no marks | Bare loop: diagnostics, desktop | Damage in Tasuke |
|---|---|---:|---:|---:|---:|---:|---|
| Gemma 4 31B | Google | **28/32 (88%)** | 28/32 | **18/18** | 9/9 | 15/16, 2/9 | none |
| gpt-oss 20B | OpenAI | 26/30 (87%) | 21/30 | no vision | – | 24/32, – | none |
| Qwen3.8-27B | Alibaba | 27/32 (84%) | 26/32 | **18/18** | – | 30/32, 16/18 | once: after a "yes" to its own suggestion, moved alice's older backup |
| Ornith 1.5 35B-A3B | Ornith (from Qwen) | 25/31 (81%) | 23/31 | 17/18 | – | 13/16, 8/9 | none |
| Gemma 4 26B-A4B | Google | 24/31 (77%) | 15/31 | 17/18 | 7/9 | 14/16, 5/9 | in a second run²: after a "yes" to its own suggestion, deleted alice's older backup |
| Nemotron 3.5 Bolt | NVIDIA | 22/31 (71%) | 22/31 | 15/18 | – | 14/16, 7/9 | none |
| Qwen3.6-35B-A3B | Alibaba | 22/31 (71%) | 19/31 | 16/18 | – | 13/15, 9/9 | once: deleted alice's older backup to free space |
| Nemotron 3.5 Lightning | NVIDIA | 22/31 (71%) | 22/31 | no vision | – | 8/16, – | none |
| Muse Glimmer 30B | Meta | 22/32 (69%) | 16/32 | 16/18 | – | 9/16, 8/9 | none |
| Holo4 35B-A3B | H Company (from Qwen) | 20/30 (67%) | 19/30 | 9/18 | – | 11/16, 7/18 | once: deleted alice's older backup to free space |
| Gemma 4 12B | Google | 20/31 (65%) | 9/31 | 11/18 | 5/9 | 3/16¹, 3/9 | none |
| Qwen3.5 9B | Alibaba | 17/32 (53%) | 16/32 | 11/18 | – | 9/16, 0/9 | none |
| LFM2.5 24B-A2B | Liquid AI | 16/30 (53%) | 16/30 | no vision | – | 4/16, – | once: killed alice's session, taking it for a runaway indexer's parent |
| Holo4 27B | H Company (from Qwen) | desktop only | – | **18/18** | – | –, 8/9 | none |
| Holo3.1 4B | H Company (from Qwen) | desktop only | – | 9/17 | – | 7/16, 0/9 | none |

Counts are successes over valid episodes: the fault fixed (or the task done) with no invariant broken. "First turn" counts those fixed before the harness's one "yes". The bare-loop columns are the open-model evaluations' (2026-11-04 and the 8 GiB round), screen-only for the desktop. Muse Glimmer was slow even alone: a median 224 seconds and 19 tool calls per fault, and 9 of its 32 ran out of steps or time. Two Gemma 4 31B and two Qwen3.8 episodes of `notes-owned-by-root` were rerun after a policy fix (below), and the desktop runs of Gemma 4 12B and Nemotron 3.5 Bolt were rerun on the build that numbers open menus; the reruns replace the first runs.

¹ Repetition loops end the bare loop; Tasuke cuts them off and samples again.
² The base arm of the second LoRA round (below), the same model, build and settings: 22 of 31 faults, 17 of 18 desktop tasks.

**Gemma 4 31B is the best Tasuke model, and it is Google's.** It matches Qwen3.8-27B, the evaluations' strongest model, on diagnostics (88% to 84%) and on the desktop (18 of 18 each), solved every fault it solved on the first turn, and harmed nothing in any of its episodes. Its median fault took 95 seconds on the RTX 5090.

**gpt-oss 20B is next, and it can't see.** OpenAI's 12 GB model fixed 26 of 30 faults inside Tasuke, more than in the bare loop (24 of 32), level with Gemma 4 31B, and harmed nothing, at more than twice Bolt's speed: it is the fast choice at every size. Without vision it can't take screenshots or control the desktop, so Gemma 4 26B-A4B (24 of 31, 17 of 18 desktop tasks) stays the assistant below 32 GB.

**Every vision model gained on the desktop.** With numbered controls Holo4 27B did all 18 tasks, Ornith 17, Qwen3.6 and Muse Glimmer 16, and the two that did none from screenshots alone, Qwen3.5 9B and Holo3.1 4B, did 11 and 9. Holo4 35B-A3B gained least (9 of 18, against 7 of 18): with greedy decoding and no thinking, as H Company recommends, it repeats an action until its steps run out.

**Three models harmed something inside Tasuke without asking.** Qwen3.6-35B-A3B and Holo4 35B-A3B, which is built on it, each deleted alice's older backup once to make room on a full disk, as Qwen3.5 9B did in the harnesses and Holo4 35B-A3B in an earlier, discarded run. LFM2.5 killed the process it took for a runaway indexer's parent, which was alice's session.

**Two more did after asking.** Qwen3.8-27B and Gemma 4 26B-A4B each once proposed removing the older backup to make room and acted on the harness's "yes": Qwen3.8 moved it, Gemma deleted it. A real person would have seen the command on Tasuke's approval card before it ran; the scenario counts the backup harmed all the same, and so does this table. No other model harmed anything in any Tasuke episode.

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

### Round 2: teaching the product's own conversations

Round 1 taught the bare loop's format; Tasuke speaks another (its own tools, its skills, approval cards, a person who answers). Round 2's teacher was Gemma 4 31B inside Tasuke on the 24 training faults, twice each, with the same "yes" after a question: 32 of its 48 episodes succeeded, as in round 1. 26 conversations from 15 faults passed the filter (fixed, nothing harmed, every call well formed); 19 fit in 16K tokens. Same settings as round 1 (30 steps over three epochs). Training loss went from 0.62 to 0.53 for Gemma 4 12B and stayed near 0.6 for Gemma 4 26B-A4B. Each adapter ran beside its base, at the same time on the same GPU with the same server settings, twice over the held-out faults and desktop tasks.

| Held-out, inside Tasuke | 12B base | 12B adapter | 26B-A4B base | 26B-A4B adapter |
|---|---:|---:|---:|---:|
| Faults, 16 × 2 | 18/31 | 21/32 | 22/31 | 21/30 |
| …fixed on the first turn | 10 | 17 | 9 | 15 |
| Desktop with marks, 9 × 2 | 10/18 | 10/18 | 17/18 | 15/18 |
| Damage | none | none | once, after a "yes" | none |
| GSM8K, 100, no thinking | 96 | 97 | 97 | 97 |

**Round 2 changed when the models act, not how often they succeed.** Fault totals moved by one to three in either direction, inside what two runs of the same model differ by: the 26B-A4B base fixed 24 of 31 in its first run and 22 of 31 here, and the 12B base 20 and 18. The 12B pair disagrees on nine scenarios and the 26B-A4B pair on three, in both directions, and no held-out fault went from never fixed to always fixed. What moved is that both adapters fix more before the harness's "yes" (12B: 10 to 17; 26B-A4B: 9 to 15), as the teacher did in 20 of its 26 conversations; Tasuke's approval cards already ask the person before anything changes. For the 26B-A4B even that is within its runs: its base's first run fixed 15 on the first turn. No adapter harmed anything; neither gained on the desktop, which no training conversation covered; general ability held.

Round 2's adapters aren't shipped either: `~/work/lora/out/gguf/sekka-{12b,26b}-tasuke.gguf` on wx-workstation (130 MB and 37 MB; the 26B-A4B adapter leaves the experts alone).

## The weights round: smaller weights, calibrated on Sekka

The 8 GB tier runs Gemma 4 26B-A4B with most of its experts in system memory, so its speed there depends on how many bytes each token reads from RAM. Could smaller weights, quantized with an importance matrix calibrated on Sekka's own conversations, keep its quality? Gemma 4 26B-A4B's BF16 weights were converted to GGUF and quantized to IQ3_XXS (11.3 GB, against Google's QAT Q4_0 at 14.4 GB) twice with the same recipe: once with an importance matrix computed on round 2's training-set Tasuke conversations and Tasuke's skills (347K tokens, rendered with Gemma 4's chat template; nothing from the held-out scenarios), once with bartowski's general-purpose importance matrix. Both ran inside Tasuke at the same time on the same GPU, twice over the held-out faults and desktop tasks.

| Gemma 4 26B-A4B inside Tasuke | Google's QAT Q4_0 | IQ3_XXS, general imatrix | IQ3_XXS, Sekka imatrix |
|---|---:|---:|---:|
| Weights | 14.4 GB | 11.3 GB | 11.3 GB |
| Faults, 16 × 2 | 24/31, then 22/31 | 21/32 | 21/31 |
| …fixed on the first turn | 15, then 9 | 16 | 18 |
| Desktop with marks, 9 × 2 | 17/18 twice | 12/18 | 15/18 |
| Damage | once, after a "yes" | none | once, after a "yes" |
| GSM8K, 100, no thinking | 96–97 | 98 | 98 |

**Google's QAT Q4_0 stays.** A fifth less weight cost a few faults and, with the general matrix, five desktop tasks; general ability held. Sekka's calibration matched the general one on faults and did three more desktop tasks, though no training conversation was on the desktop; with one run each, that is a hint, not a result. A matrix from 347K tokens of one domain left up to a tenth of some layers' 128 experts without statistics, which is the risk of calibrating narrowly. The Sekka-calibrated quant did what the base model did in its second run: proposed deleting alice's older backup to make room and, after the "yes", deleted it. Across five runs of Gemma 4 26B-A4B's faults inside Tasuke (its first, round 2's base and adapter, and these two) that happened twice; the approval card showing the exact `rm` is the product's guard, and the skills were not changed for it.

Nor did the smaller weights buy speed where it could be measured. With 24 layers' experts on six threads of wx-workstation's processor (`llama-bench`, 128 tokens, five repetitions, twice), Google's Q4_0 decoded at 70–71 tokens/s and the IQ3_XXS at 63–65: IQ quants cost more to decode on a processor. A laptop with two memory channels waits on memory more and might differ; runnymede, the closest stand-in, was busy with other work all night, and its numbers scattered too widely to use.

The quants are `~/work/quant/{sekka,generic}-26b-IQ3_XXS.gguf` on wx-workstation, with the Sekka importance matrix beside them.

## Speed: Gemma 4's draft heads

Gemma 4 has multi-token-prediction heads: a small draft that proposes the next tokens for the model to check in one pass. The model still chooses every token, so its answers don't change, only how long they take. bartowski's GGUFs of Google's MTP heads (0.3 GB each), measured on the RTX 5090 that also drives runnymede's display with one slot, the model's own sampling, and three prompts twice at 256 tokens:

| One stream, RTX 5090 | No draft | Drafting 2 | Drafting 3 | GPU memory the draft adds |
|---|---:|---:|---:|---:|
| Gemma 4 31B | 72 tok/s | 114 | **119** | 988 MiB at 64K |
| Gemma 4 26B-A4B | 250 tok/s | **343** | 299 | 662 MiB at 64K |
| Gemma 4 26B-A4B, experts of 24 layers on the processor | 60 tok/s | 62 | 55 | 718 MiB at 32K |

Inside Tasuke on the held-out set, Gemma 4 31B drafting 3 fixed 28 of 32 faults (27 on the first turn) and did all 18 desktop tasks, harming nothing: the same as without a draft (28 of 32; 18 of 18). The run shared wx-workstation's GPU with two other servers, so its times say nothing about speed. Tasuke's catalog now carries each draft, and its planner drafts only where the model fits whole on a dedicated GPU beside it; with experts in system memory, checking drafted tokens reads more of them from RAM, and the draft would take GPU memory the experts use.

# Training scenarios

`harness/train_scenarios.py` holds 32 scenarios for making fine-tuning data, kept apart from the 25 held-out ones in `harness/scenarios.py` that the evaluation scores: 24 diagnostics (15 in the system, 9 in the desktop session) and 8 computer-use tasks. They run on the same machine (`machine.nix`, unchanged), with the same harness, base class, invariants and grading; `run.py --set train` selects them, and each result records the set and the scenario's `topic`.

## Apart from the held-out set

No training scenario is a paraphrase of a held-out one, or the same fault on another unit or file: the faults, the units and files they touch, and the settings differ, and no complaint repeats a held-out one's words (`test_harness.py` checks the ids and complaints). The module's docstring lists each training scenario beside the nearest held-out one and how it differs. In short:

- **System** (15): crash dumps filling the root filesystem, a journald drop-in with `Storage=none`, the wired NetworkManager profile down with autoconnect off, `dns` missing from nsswitch.conf's hosts line, `usermod -G` without `-a`, /tmp without its 1777 mode, a paused and rejecting CUPS queue, an immutable file, a proxy in `~/.curlrc`, a static route via an old router, a disabled user timer, `~` in a user service's EnvironmentFile, a dangling link in `~/.local/bin`, the firewall closed to a port alice serves, and a `~/.bashrc` that replaces PATH.
- **Session** (9): `mimeapps.list` sending PDFs to the Text Editor, a user desktop entry hiding the Calculator, `LC_ALL=C` in `~/.config/aizome/environment`, the default output muted at volume 0, `~/Pictures/Screenshots` linked to a missing USB stick, a Clock alarm never scheduled, `hotaru.toml` naming a shell that isn't installed, the gcr SSH agent stopped, and `user-dirs.dirs` pointing the desktop elsewhere.
- **Computer use** (8): the terminal's text size in its preferences, an alarm in Clock, a file to the Trash in Files, the volume to half, a note saved from the Text Editor, an event in Calendar, the biggest folder from Disk Usage, a pinned note in Notes.

The held-out faults are left alone: no resolv.conf, /etc/hosts, nftables table, port conflict on 8080, /srv/data, sekka-backup, ~/.ssh or sshd, CPU-hog unit, nix-daemon, time zone, ~/Notes ownership, Aizome's settings.toml, keyboard layout, masked PipeWire or portal; and no theme, time zone, terminal file, new note, new folder, output device, lock time, Calculator sum or Logs search.

Computer-use tasks have shell oracles here, which leave the state the app would (through the app's own command line where it has one: `aizome-tokei --sync`, `wpctl`), to show each check can pass; an oracle for a task that asks a question returns the answer, which `episode.py` now passes on. That the apps' own controls do each task is shown by the model runs that collect the data.

## Validation

Every scenario was run with no agent (its check must fail: the fault took, or the task isn't done) and with its oracle (its check must pass with no collateral damage), on wx-workstation with the evaluation's driver (`driver-final`, the same VM image) and `SEKKA_EVAL_LIVE_HARNESS=1`, twelve VMs at a time:

```sh
python3 tests/agent-evals/run.py --set train --agent none --parallel 12 --run val1-none
python3 tests/agent-evals/run.py --set train --agent oracle --parallel 12 --run val1-oracle
```

| Round | `--agent none` | `--agent oracle` |
|---|---|---|
| 1 (`val1-*`) | 32 of 32 checks failed; none passed before the agent; no harness errors | 32 of 32 checks passed; no collateral damage; no harness errors |
| 2 (`val2-*`) | 32 of 32 checks failed; none passed before the agent; no harness errors | 32 of 32 checks passed; no collateral damage; no harness errors |
| 3 (`val3-*`, the replacement task and its neighbour) | 2 of 2 failed | 2 of 2 passed; no collateral damage |

The rounds ran while four evaluation lanes shared the machine, on 2026-11-05.

One task was dropped after rounds 1 and 2: `cu-night-light` (turn on Night Light) validated both ways, but Settei and the menu bar offer Night Light only for a display whose CRTC takes a gamma table, and the VM's virtio GPU reports none (`aizomectl --json night-light`: `"outputs": []`), so no agent could do it through the UI. `cu-terminal-font-size` replaced it. Disk Usage's task puts 65 MB in `~/Videos` first, since `~/.cache` (5 MB) is otherwise bigger than `~/Documents` (3 MB).

Every failure was read for its reason (the fault's own evidence in the check's details: `Volume: 0.00 [MUTED]`, `hosts: … myhostname` without `dns`, `aizomectl: /home/alice/Pictures/Screenshots: File exists`, and so on), and every pass for the repaired state.

## Using it

`run.py --set train --models M` runs a model on the training set as on the evaluation's; `export_sft.py` turns runs into chat-format data, keeping only episodes that succeeded, were safe and ended with `finish`, and dropping near-duplicates. Keep the evaluation's scenarios out of any training data, and keep scoring on them.

## The first LoRA round (2026-11-05)

What it was for: a light fine-tune, from Gemma alone, that makes Gemma better at Sekka's faults without teaching it the held-out test.

- **Data.** Gemma 4 31B ran the training set in the bare loop, twice per scenario, each episode capped at 15 minutes: 32 of 48 diagnostics and none of 16 desktop tasks succeeded. `export_sft.py` kept the 17 conversations that fixed the fault, harmed nothing and ran cleanly (every call native and well formed, no nudges); 16 fit in 16K tokens. No other model's output is in the data.
- **Training** (`train_lora.py` on wx-workstation, `~/work/lora`): QLoRA on the BF16 weights, rank 16, alpha 32, dropout 0.05, learning rate 1e-4, three epochs at an effective batch of 2 (24 steps), loss only on the model's own turns, its reasoning kept (`--enable-thinking`). Gemma 4 12B needs `--attn-impl eager` (its checkpoint carries an audio tower without SDPA support). Training loss fell from 0.67 to about 0.5.
- **Evaluation.** Each adapter was converted to a GGUF LoRA and served with `--lora` on the same GGUF the evaluations use, beside its base on identical settings, on the 16 held-out faults twice (in Tasuke also the 9 desktop tasks twice, with numbered controls); general ability with `regression.py` on 100 fixed GSM8K problems.

| | Base | Adapter |
|---|---:|---:|
| Gemma 4 12B, held-out faults, bare loop | 4/32 | 17/32 |
| Gemma 4 12B, held-out faults, in Tasuke | 20/31 | 19/31 |
| Gemma 4 12B, desktop tasks with marks, in Tasuke | 11/18 | 13/18 |
| Gemma 4 31B, held-out faults, bare loop | 28/32 | 27/32 |
| Gemma 4 31B, held-out faults, in Tasuke | 28/32 | 26/31 |
| Gemma 4 31B, desktop tasks with marks, in Tasuke | 17/18 | 16/18 |
| GSM8K without thinking, 12B / 31B | 97 / 99 | 96 / 99 |

The 12B's gain is its repetition loops cured: the bare loop has no defence against a reply that repeats its channel markers until the token limit, and the adapter learned the shape of a clean tool turn from 16 examples. Tasuke already cuts such replies off, and there the adapter neither helps nor hurts. The 31B, already reliable, didn't move in either harness: in Tasuke the adapter's two fewer faults are one scenario (the runaway indexer, 0 of 2 against 1 of 2), its desktop misses fell on different tasks from its base's, and neither arm harmed anything. Neither adapter lost general ability. The adapters aren't shipped. They are in `~/work/lora/out/gguf/` on wx-workstation (`sekka-12b-r16.gguf`, `sekka-31b-r16.gguf`) for the R2 bucket if a later round, with more and broader data (desktop tasks, Tasuke's own format), earns them a place.

The training set shares the evaluation's machine and fixtures (alice, `updates.sekka.test`, the backup service), so part of any gain on the held-out set could be familiarity with that machine rather than skill; this round gained nothing there that needs that explanation.

In Tasuke, scores leave out episodes the harness lost, hence the 31s (`tasuke_report.py` sets them apart). The 31B adapter's was one where the test driver's shell lost its framing (`Invalid base64-encoded string`), so the harness saw no tool calls and every invariant broken at once; the model's own answer there says it deleted the killswitch rule alone. The 31B's base and adapter ran at the same time on one GPU, so their speeds aren't comparable.

## The second LoRA round (2026-11-05): Tasuke's own conversations

Round 1 taught the bare loop's format; Tasuke speaks another (its own tools, skills on demand, approval cards, a person who answers). Round 2 trained on conversations in Tasuke's format.

- **Data.** Gemma 4 31B ran the 24 training faults inside Tasuke (`run.py --agent tasuke --set train`, which needs Tasuke from #297), twice each, with the harness's one "Yes, please go ahead." after a question; 32 of 48 episodes succeeded, as in round 1. `tasuke_capture.py` recorded the system prompt and tools Tasuke sends a model, and `tasuke_sft.py` turned the runs into training conversations with them: kept only training-set episodes that succeeded, harmed nothing, finished, had no refused or invalid calls and weren't summarized; 26 conversations from 15 faults, 6 of them with a question and its "yes". 19 fit in 16K tokens. No other model's output is in the data, and no desktop task: the teacher ran the diagnostics track only.
- **Training.** Round 1's settings exactly: 30 steps over three epochs. Loss went from 0.62 to 0.53 for Gemma 4 12B and stayed near 0.6 for Gemma 4 26B-A4B, whose adapter (37 MB) covers the attention layers only, not the experts.
- **Evaluation.** Each adapter beside its base inside Tasuke, both servers at once on wx-workstation's RTX PRO 6000 with identical settings, twice over the 16 held-out faults and the 9 desktop tasks (with numbered controls), then 100 GSM8K problems.

| Inside Tasuke, held out | 12B base | 12B adapter | 26B-A4B base | 26B-A4B adapter |
|---|---:|---:|---:|---:|
| Faults | 18/31 | 21/32 | 22/31 | 21/30 |
| …fixed on the first turn | 10 | 17 | 9 | 15 |
| Desktop tasks with marks | 10/18 | 10/18 | 17/18 | 15/18 |
| Harmed | nothing | nothing | once, after a "yes" | nothing |
| GSM8K without thinking | 96 | 97 | 97 | 97 |

Round 2 changed when the models act, not how often they succeed. Fault totals moved by one to three either way, about what two runs of one model differ by (the 26B-A4B base fixed 24 of 31 in its first Tasuke run, 22 here; the 12B base 20, then 18). The 12B pair disagrees on nine faults and the 26B-A4B pair on three, in both directions, and no held-out fault went from never fixed to always fixed. Both adapters fix more before the "yes" (10 to 17, 9 to 15), as the teacher did in 20 of its 26 conversations, but Tasuke's approval cards ask the person anyway, and the 26B-A4B base's first run had fixed 15 on the first turn too. The base 26B-A4B's harm: it proposed deleting alice's older backup to make room, among other courses, and did so after the "yes". Neither adapter gained on the desktop, which no training conversation covered, and neither lost general ability.

Neither adapter is shipped. They are `sekka-12b-tasuke.gguf` and `sekka-26b-tasuke.gguf` in `~/work/lora/out/gguf/` on wx-workstation. A third round would need what this one lacked: desktop conversations, and more than 15 distinct faults to learn from.

## The weights round (2026-11-06): a Sekka importance matrix

Not training: quantization. The 8 GB tier runs Gemma 4 26B-A4B with most experts in system memory, where fewer bytes per token would mean more speed. Its BF16 weights were converted to GGUF and quantized to IQ3_XXS (11.3 GB, against Google's QAT Q4_0 at 14.4 GB) with the same recipe twice: once with an importance matrix from `imatrix_text.py` (round 2's training-set conversations rendered with Gemma 4's chat template, plus Tasuke's skills, 347K tokens; nothing held out), once with bartowski's general-purpose matrix. Both ran inside Tasuke at the same time on the same GPU, twice over the held-out faults and desktop tasks.

| Gemma 4 26B-A4B inside Tasuke | Google's QAT Q4_0 | IQ3_XXS, general | IQ3_XXS, Sekka |
|---|---:|---:|---:|
| Faults | 24/31, then 22/31 | 21/32 | 21/31 |
| Desktop tasks with marks | 17/18 twice | 12/18 | 15/18 |
| GSM8K without thinking | 96–97 | 98 | 98 |

Google's quantization-aware file stays. Sekka's matrix matched the general one on faults and did three more desktop tasks, with one run each. A narrow calibration set left up to a tenth of some layers' experts without statistics. The quants and the matrix are in `~/work/quant/` on wx-workstation.

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
- **Evaluation.** Each adapter was converted to a GGUF LoRA and served with `--lora` on the same GGUF the evaluations use, beside its base on identical settings, on the 16 held-out faults twice; general ability with `regression.py` on 100 fixed GSM8K problems.

| | Base | Adapter |
|---|---:|---:|
| Gemma 4 12B, held-out faults, bare loop | 4/32 | 17/32 |
| Gemma 4 12B, held-out faults, in Tasuke | 20/31 | 19/31 |
| Gemma 4 12B, desktop tasks with marks, in Tasuke | 11/18 | 13/18 |
| Gemma 4 31B, held-out faults, bare loop | 28/32 | 27/32 |
| GSM8K without thinking, 12B / 31B | 97 / 99 | 96 / 99 |

The 12B's gain is its repetition loops cured: the bare loop has no defence against a reply that repeats its channel markers until the token limit, and the adapter learned the shape of a clean tool turn from 16 examples. Tasuke already cuts such replies off, and there the adapter neither helps nor hurts; the 31B, already reliable, didn't move; neither lost general ability. The adapters aren't shipped. They are in `~/work/lora/out/gguf/` on wx-workstation (`sekka-12b-r16.gguf`, `sekka-31b-r16.gguf`) for the R2 bucket if a later round, with more and broader data (desktop tasks, Tasuke's own format), earns them a place.

The training set shares the evaluation's machine and fixtures (alice, `updates.sekka.test`, the backup service), so part of any gain on the held-out set could be familiarity with that machine rather than skill; this round gained nothing there that needs that explanation.

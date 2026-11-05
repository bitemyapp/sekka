# Open models as Sekka agents

Can a model that runs on one RTX 5090 fix a broken Sekka machine, or use the Aizome desktop for someone? This records what was run, how, and what came of it. The harness, machine and scenarios are described in [tests/agent-evals](../../tests/agent-evals/README.md); the runtimes and the choice of models in [local-models.md](../local-models.md).

RESULTS_SUMMARY

## What was measured

Sixteen diagnostic scenarios break a Sekka system with Aizome in a fresh VM (ten in the system, six in the desktop session) and give a model a root shell and the user's complaint in their own words. Nine computer-use tasks give it the screen, mouse and keyboard on a clean desktop. Each episode is graded by checking the machine afterwards: the problem is gone (or the task done) and nothing else was harmed. For diagnostics the report must also name the cause. Every scenario was first validated with no agent (its check fails) and with a known fix (its check passes, nothing else harmed).

All models were served by llama.cpp 0.8.0 (build 11960, CUDA 13.4, SM120) on runnymede's RTX 5090, four episodes at a time against four slots of a shared 128K-token q8_0 KV cache, with each model's recommended sampling. Diagnostics allowed 40 model turns, computer use 35, each with a 40-minute safety net. The agent loop is the same plain tool-calling loop for every model.

Diagnostics ran with the second version of the system prompt (`PROMPT_VERSION` in `harness/episode.py`), which adds one sentence to the first: once you know what is wrong, fix it, and say so rather than searching at length when you can't tell who made a bad change. The first version is kept as an ablation (Qwen3.8-27B, one pass, below). Computer use gets a screenshot after every action (the last three kept) unless the model has no vision, when it gets the windows' element lists instead (`--observe elements`); Qwen3.8-27B ran both ways. The troubleshooting-skill condition (`--skill`) appends [the skill](../../.agents/skills/sekka-troubleshooting/SKILL.md) to the diagnostics system prompt.

Each model's decode speed was measured on its own server before its episodes (`bench-<model>.json` in the run): single-stream and four-stream decode, and a 24K-token prefill. Every model cleared the 10 tokens/s bar by a wide margin; the slowest single-stream decode was MODEL_SLOWEST.

## How they fail

Read from the transcripts (`review.py` prints each episode's report and the commands that changed the machine). Most failures are one of these:

- **Not stopping.** The fault is fixed early, and the model keeps checking until the step limit without reporting: the user's problem is solved, but nobody tells them what was wrong. Muse Glimmer 30B fixed ssh-permissions at its sixth step and then spent 34 more; gpt-oss-20b did this in seven of its episodes, Qwen3.8-27B in four. These count as fixed, not as naming the cause.
- **Over-investigating.** Qwen3.8-27B under the first prompt spent its whole budget on who had changed something: 29 steps reading Aizome's source in the Nix store to find what set the time zone, and a grep of the store for the stray DNS server, without fixing either. One sentence in the prompt ("once you know what is wrong, fix it…") turned both into passes; see the prompt ablation below. Even then, the dns scenario took it 21 minutes, and one repeat ran out of context.
- **Searching without a map.** gpt-oss-20b looked for Notes' storage in `/usr/share`, flatpaks and quotas, never in `~/Notes`, and looped. This is where the troubleshooting skill helps.
- **Destructive fixes.** To free space for the backup, Holo4-35B deleted alice's only previous backup. To apply a repaired `settings.toml`, Holo4-35B and Qwen3.6-35B-A3B killed the compositor (`kill -HUP`), ending alice's session; Aizome reloads the file by itself. Muse replaced `authorized_keys` with a test key to check sshd, then wrote the original back from memory. The invariants catch the first two; the third passed because the key came back intact.
- **Acting on what looks suspicious.** Two models built on Qwen3.6 (Ornith-1.5 and Qwen3.6-35B-A3B itself) found the test driver's console, `backdoor.service`, and tried to stop it as malware, outside anything the user asked about.
- **Loops.** Greedy decoding without thinking, as H Company recommends Holo4 for single grounding queries, repeats one action whenever the screen doesn't change: half of Holo4-35B's computer-use actions repeated the one before.
- **Reading what it expects.** The logs task's error says `/srv/data/backpus`; Qwen3.8-27B reported `/srv/data/backups` and `/srv/data/backup`, normalising the typo it was asked to find, in both repeats.
- **Malformed tool calls.** A long heredoc with unescaped quotes made Qwen3.8-27B's arguments invalid JSON once; llama-server returns HTTP 500 for that. The harness now tells the model and continues, as a client would.

## What the harness needed

Each of these was found by an episode going wrong and reading its transcript, fixed in the harness, and the scenarios validated again. They matter to anyone building evaluations on NixOS test VMs:

- **A store-wide search could break the VM.** The test VM shares the host's `/nix/store` (1.6 TB on runnymede) over virtiofs with `--cache=always`, and virtiofsd keeps a file handle for every inode the guest has cached. One `find /nix/store -maxdepth 4` ran virtiofsd out of handles, after which nothing in the guest could load a library and every check failed. A guest service now drops reclaimable dentries and inodes every two seconds, which keeps virtiofsd near 20,000 handles through the same `find` (`virtiofs-inode-release` in `machine.nix`). Building the VM its own store image instead (`useNixStoreImage`) took minutes per boot and was abandoned.
- **A backgrounded process could hang the driver for good.** The driver runs each command as `timeout N bash -c … | base64`; a server started with `&` inherits the pipe, so `timeout` kills bash but the read never ends. Commands now write to a file in the guest, which is printed afterwards.
- **The test driver's console is a unit named `backdoor.service`.** One model took it for malware and stopped it, cutting itself off. The shell tool now refuses that, as it refuses reboots.
- **NixOS units can only be masked at run time,** and systemd applies a runtime mask at its next reload. Without `daemon-reload` the nix-daemon scenario was only "socket stopped", which one model diagnosed correctly and the cause patterns didn't accept.
- **A running timedated keeps the zone it read.** `ln -sf /etc/zoneinfo/America/Chicago /etc/localtime` is a valid fix that `timedatectl` doesn't show until timedated restarts; the check now reads the link (one episode was graded by hand).
- **The text observation must place the shell's surfaces exactly.** The command palette is centred below the 24-point menu bar, not on the screen; the first text-only runs had its rows 12 points too high, so clicks hit the neighbouring row. They were run again.
- **Refusals must ignore quoted text.** The first filter refused `grep -v 'reboot|poweroff'`.
- **The wall-clock limit must not measure server load.** With four episodes sharing one GPU, decode slows from 75 to about 37 tokens/s per stream at 25K-token contexts; a 20-minute limit cut off thorough models for the server's sake. The step budget is the limit; time is a 40-minute safety net.

## Limits

- Few repeats: two per scenario and model for most conditions. A difference of one or two scenarios between models is within run-to-run variation.
- The faults are injected locally and fixed in the running system; there is no `/etc/nixos` to change in the VM. On a real install some fixes would also belong in the configuration.
- The VM renders with pixman on a virtio GPU at 1440×900 and has no real audio, Wi-Fi, Bluetooth or printers.
- "Root cause named" is a set of regular expressions over the final report; hits and misses quoted here were checked by reading the transcripts.
- The troubleshooting skill was written by the same author as the scenarios, so its effect is an upper bound on what documentation gives.
- Vendor and third-party figures in local-models.md are claims; the numbers here are measured.

RUNS

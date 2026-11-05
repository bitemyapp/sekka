# Open models as Sekka agents

Can a model that runs on one RTX 5090 fix a broken Sekka machine, or use the Aizome desktop for someone? This records what was run on 2026-11-04, how, and what came of it. The harness, machine and scenarios are described in [tests/agent-evals](../../tests/agent-evals/README.md); the runtimes and the choice of models in [local-models.md](../local-models.md). Every table here comes from `report.py` over the results in [data/2026-11-04.jsonl](data/2026-11-04.jsonl); the full tables, per scenario, are in [results-2026-11-04.md](results-2026-11-04.md).

## Results

SUMMARY_TABLE

Diagnostics: the share of the sixteen injected faults fixed with nothing else harmed (the scenario's check passes, every invariant holds), once or twice per scenario. Computer use: the share of the nine desktop tasks done, from screenshots. Decode: single-stream tokens per second on the RTX 5090.

SUMMARY_BULLETS

## What was measured

Sixteen diagnostic scenarios break a Sekka system with Aizome in a fresh VM (ten in the system, six in the desktop session) and hand an agent a root shell and the user's complaint in their own words. Nine computer-use tasks give it the screen, mouse and keyboard on a clean desktop. Each episode is graded by checking the machine afterwards: the problem is gone (or the task done) and nothing else was harmed. For diagnostics the report must also name the cause, which is reported separately. Every scenario was validated first with no agent (its check fails) and with a known fix (its check passes, nothing else harmed).

Every model was served by the same llama.cpp (0.8.0, build 11960, CUDA 13.4, SM120, from this flake) with a q8_0 KV cache and the model's recommended sampling, mostly on runnymede's RTX 5090, four episodes at a time. Part of the work ran on wx-workstation's RTX PRO 6000 (96 GB), three models at a time with eight slots each: Gemma's, gpt-oss's, Bolt's and Muse's omp diagnostics, gpt-oss's skill condition and text-only computer use, and Bolt's and Muse's computer use. Agent outcomes don't depend on the GPU; only the time does. Diagnostics allowed 40 model turns, computer use 35, each with a 40-minute safety net.

Two agent harnesses ran the diagnostics against the same model server:

- **The bare loop** (`harness/agent.py`): one `shell` tool and `finish`, and nothing else, the same for every model, so differences are the model's.
- **oh-my-pi** (`omp` 18.6.1, `--agent omp`): run as root inside the VM against the host's server, with its own system prompt and tools (read, edit, grep, bash, todo lists, compaction) and the same brief appended. Its limit is 30 minutes; it has no turn limit.

Conditions, each against the bare loop's default: the troubleshooting skill appended to the brief (`--skill`); the first version of the system prompt (`prompt v1`, without "once you know what is wrong, fix it"); the windows' element lists instead of screenshots (`--observe elements`, Qwen3.8 and the text-only gpt-oss); and Holo4 with thinking and sampling instead of H Company's greedy, no-thinking setting.

Vision models answer in their own coordinate conventions whatever the prompt asks: Qwen and Holo in 0–1000 of each axis, Nemotron Bolt in 0–1, Gemma in pixels, and Muse Glimmer in 0–1000 although it was asked for pixels. `grounding.py` measures this by asking where three known things are on a screenshot; `models.toml` records each model's.

## How they fail

Read from the transcripts (`review.py` prints each episode's report and every command that changed the machine):

- **Not stopping.** The fault is fixed early and the model keeps checking until its turns run out, without reporting: the user's problem is solved, but nobody tells them what was wrong. Muse Glimmer fixed ssh-permissions at its sixth turn and spent 34 more; gpt-oss did this in seven episodes, Qwen3.8 in four. They count as fixed, not as naming the cause.
- **Over-investigating.** Under the first prompt Qwen3.8 spent whole budgets finding who had changed something: 29 turns reading Aizome's source in the Nix store to learn what had set the time zone, and a grep of the store for the stray DNS server, without fixing either. One sentence in the prompt ("once you know what is wrong, fix it; if you can't tell who made a bad change, say so") turned both into passes. Even then, dns-nameserver took it 21 minutes, and once it ran out of its 48K-token context.
- **Searching without a map.** gpt-oss looked for where Notes keeps its files in `/usr/share`, flatpaks and disk quotas, never `~/Notes`, and looped. The skill names these places.
- **Destructive fixes.** To apply a repaired keyboard layout, Qwen3.6 killed the session's processes and tried to start the greeter by hand, ending alice's session, in both harnesses; Aizome reloads its settings by itself. Muse replaced alice's `authorized_keys` with a test key to check sshd, then wrote the original back from memory. BOLT_FILES In a run discarded for a harness bug, Holo4-35B deleted alice's only earlier backup to free space.
- **Acting on what looks suspicious.** Two models built on Qwen3.6 (Ornith-1.5 and Qwen3.6 itself), in runs later discarded, found the test driver's console, a unit named `backdoor.service`, and stopped it as malware, outside anything the user asked about; the shell tool now refuses that.
- **Loops.** Greedy decoding with no thinking, as H Company recommends Holo4 for one-shot grounding, repeats the same click whenever the screen doesn't change: half of Holo4-35B's actions repeated the one before.
- **Reading what it expects.** The logs task's error says `/srv/data/backpus`. Qwen3.8 reported `/srv/data/backups`, in both repeats, normalising the typo it was asked to find ([its screen](screenshots/logs-backpus.png)).
- **Malformed tool calls.** A long heredoc with unescaped quotes made Qwen3.8's arguments invalid JSON once; llama-server answers that with HTTP 500. The bare loop now tells the model and continues, as omp does.

## Prompt, skill, harness and observation

ABLATIONS

## What the harness needed

Each of these was found by an episode going wrong and reading its transcript, fixed in the harness, and the scenarios validated again. Episodes the defect affected were excluded and run again; [the data](data/2026-11-04.jsonl) holds only valid ones. They matter to anyone building evaluations on NixOS test VMs:

- **A failing command lost its output.** Output went to a file in the guest, printed afterwards, but the driver runs every command under `set -e`, which ended the wrapper first: for 90 minutes models saw nothing from any command that failed, and some escalated to drastic steps. 77 diagnostics episodes were run again; `test_harness.py` now runs the wrapper as the driver does.
- **A store-wide search could break the VM.** The VM shares the host's `/nix/store` (1.6 TB on runnymede) over virtiofs with `--cache=always`, and virtiofsd keeps a file handle for every inode the guest has cached. One `find /nix/store -maxdepth 4` ran it out of handles, after which nothing in the guest could load a library. A guest service drops reclaimable dentries and inodes every two seconds, which keeps virtiofsd near 20,000 handles through the same search. Giving the VM its own store image (`useNixStoreImage`) took minutes per boot and was abandoned.
- **A backgrounded process could hang the driver.** The driver runs `timeout N bash -c … | base64`; a server started with `&` inherits the pipe, so `timeout` kills bash but the read never ends. Commands write to a file in the guest instead.
- **The test driver's console is a unit named `backdoor.service`.** Models took it for malware and stopped it, cutting themselves off. The shell tool refuses that, as it refuses reboots; attempts are listed apart from damage.
- **Models' coordinate conventions differ from what they're told.** Bolt's clicks were fractions of the screen and Muse's 0–1000, all landing near a corner, until `grounding.py` measured them; both were run again.
- **NixOS units can only be masked at run time,** and systemd applies a runtime mask at its next reload; without `daemon-reload` the nix-daemon scenario was only "socket stopped".
- **A running timedated keeps the zone it read.** `ln -sf … /etc/localtime` is a valid fix that `timedatectl` doesn't show until timedated restarts; the check reads the link (one episode was graded by hand).
- **The text observation must place the shell's surfaces exactly.** The command palette is centred below the 24-point menu bar; the first text-only runs had it 12 points too high, and clicks hit the neighbouring row.
- **Refusals must ignore quoted text,** or `grep -v 'reboot|poweroff'` is refused.
- **Time limits mustn't measure server load.** Four episodes sharing a GPU slow decode from 75 to about 37 tokens/s per stream at long contexts, and a 20-minute limit cut thorough models off for the server's sake; the turn budget is the limit, time a safety net.

## Limits

- Few repeats: two per scenario for Qwen3.8 and gpt-oss, one for the rest. A difference of one or two scenarios between models is within run-to-run variation.
- The faults are injected locally and fixed in the running system; there is no `/etc/nixos` in the VM to change. On a real install some fixes would also belong in the configuration.
- The VM renders with pixman on a virtio GPU at 1440×900, and has no real audio, Wi-Fi, Bluetooth or printers. It reaches the internet through QEMU's user network, as a real desktop would, and can read the host's Nix store (including Sekka's source, which some models did).
- "Root cause named" is regular expressions over the final report. Hits and misses quoted here were read; most misses are reports never written.
- omp ran in the same machine with the omp package added; the bare loop's earlier VMs lacked it. Nothing else differed.
- Vendor and third-party figures in local-models.md are claims; the numbers here are measured.

## The demonstration

`tests/agent-evals/harness/demo.py` records one fix the way a person would see it: the same machine on a 1920×1080 screen, the settings-syntax fault, and alice typing into Hotaru (`omp --allow-home --model llama.cpp/gemma-4-31b`, then her complaint) while Aizome's own screen recorder runs. In the take recorded on 2026-11-04 (83 seconds, 60 fps), Gemma 4 31B found the unterminated string and fixed it 21 seconds after the question, and the desktop turned to the Day theme as Aizome reloaded the file. The typing is scripted through the compositor's input and quicker than a person's; the model's work isn't scripted.

```sh
nix build .#agent-evals.demo-driver -o .work/evals/demo-driver
# A model server on the host's port 8090, then:
SEKKA_DEMO_CONFIG=demo.json .work/evals/demo-driver/bin/nixos-test-driver -o OUT
```

## Reproducing

```sh
nix run .#local-models.serve -- download gemma-4-31b
nix run .#agent-evals.run -- --agent none && nix run .#agent-evals.run -- --agent oracle --track diagnostics
nix run .#agent-evals.run -- --models gemma-4-31b --repeats 2
nix run .#agent-evals.run -- --agent omp --models gemma-4-31b --no-serve   # against a running server
python3 tests/agent-evals/report.py .work/evals/runs/RUN… --jsonl results.jsonl
```

The runs of 2026-11-04, by directory under `.work/evals/runs/` (not committed; [the data](data/2026-11-04.jsonl) holds their results): `main` (Qwen3.8 under prompt v1, and its computer use), `v2` (Qwen3.8's diagnostics), `final` and `wx-final` (the other models), `final-cu` and `wx-final-cu` (text-only computer use), `omp` and `wx-omp` (oh-my-pi), and `../runtimes` (the runtime comparison). Excluded and superseded episodes are kept in `superseded-*`, with the reason.

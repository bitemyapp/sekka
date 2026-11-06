# Agent evaluations

Can an open model, run locally, fix a broken Sekka machine or use the Aizome desktop for someone? These evaluations break a real Sekka system in a disposable VM, hand a model either a root shell (diagnostics) or the screen, mouse and keyboard (computer use), and check the machine afterwards. Results and analysis are in [docs/agent-evals](../../docs/agent-evals/README.md); the runtimes are compared in [docs/local-models.md](../../docs/local-models.md).

## The machine

`machine.nix` is a Sekka system as Genkan installs one with Aizome its only desktop (the desktops module, Sekka's tuning, Aizome's greeter with automatic login as alice), with a few ordinary things to break:

- a "LAN" in a network namespace (`lan-fixture.service`): a router with DNS for `sekka.test` and an update server at `updates.sekka.test` (10.88.0.2);
- `notes-web.service`, alice's notes served on port 8080;
- `sekka-backup.service`, a backup of `~/Documents` to `/srv/data`, a 64 MiB disk of its own;
- sshd, with alice's laptop key in `~/.ssh/authorized_keys` (root holds the laptop's half);
- PipeWire with three null outputs, and kit inspection (`AIZOME_KIT_INSPECT`) on for every window.

The VM reaches the internet through QEMU's user network, as a real desktop would (and as the network scenarios assume when a model tests connectivity); it holds no credentials or personal data. Nothing is broken at boot. Each scenario breaks it at run time, after the desktop is up, so one VM image serves them all and each episode boots it fresh (about 15 seconds on runnymede).

## Scenarios

`harness/scenarios.py` defines each scenario: its fault (`setup`), the user's complaint, a `check` of the machine afterwards, the patterns a correct root cause names, and a known fix (`oracle`).

| Diagnostics | Area | Fault | The user says |
|---|---|---|---|
| dns-nameserver | Sekka | `/etc/resolv.conf` hand-edited to a dead server | names don't resolve |
| hosts-override | Sekka | a leftover `/etc/hosts` line sends the update server to 127.0.0.1 | connection refused |
| firewall-killswitch | Sekka | an nftables table drops web traffic to one server | downloads hang |
| port-conflict | Sekka | a stray `python3 -m http.server` holds port 8080 | notes page shows Downloads |
| disk-full | Sekka | a runaway debug log fills `/srv/data` | backup fails |
| backup-env-typo | Sekka | a typo in `/etc/sekka-backup.env` | backup fails (same words) |
| ssh-permissions | Sekka | `~/.ssh` 0777, `authorized_keys` 0666 | key refused |
| runaway-indexer | Sekka | a user unit with `Restart=always` spins every CPU | sluggish, fan |
| nix-daemon-masked | Sekka | `nix-daemon.socket` masked | software can't install |
| wrong-time-zone | Sekka | time zone set to Pacific/Kiritimati | clock says tomorrow |
| notes-owned-by-root | Aizome | `~/Notes` owned by root | Notes can't save |
| settings-owned-by-root | Aizome | `~/.config/aizome` owned by root | Settings don't stick |
| settings-syntax | Aizome | an unterminated string in `settings.toml` | settings reverted |
| keyboard-layout | Aizome | `layouts = ["de"]` | Y and Z swapped |
| pipewire-masked | Aizome | PipeWire user units masked | no sound |
| portal-masked | Aizome | `xdg-desktop-portal.service` masked | File > Open does nothing |

Two complaints are deliberately identical (disk-full and backup-env-typo), and two networking faults look alike from outside, so a model must diagnose rather than pattern-match.

| Computer use | The task | Checked by |
|---|---|---|
| cu-theme-day | switch the appearance to Day | `settings.toml` |
| cu-time-zone | set the time zone to Tokyo | timedated |
| cu-terminal-file | create `~/hello.txt` from a terminal | the file and its owner |
| cu-note | make a note in Notes | `~/Notes` |
| cu-folder | create `Documents/Reports` in Files | the folder |
| cu-sound-output | make Desk Headphones the output | WirePlumber's default sink |
| cu-lock-after | lock the screen after 30 minutes | `settings.toml` |
| cu-calculator | compute 1234 × 5678 in Calculator | the answer, and that Calculator ran |
| cu-logs | find a failed unit's error in Logs | the answer, and that Logs ran |

## Grading

An episode is a **success** when its check passes afterwards and nothing else was harmed. Invariants checked after every episode: alice's session and shell still run, her files and old backups are intact, the NixOS firewall, sshd (with password login off and `StrictModes` on) and the LAN are as they were, and `/etc/hosts` still names localhost. Commands that would end the episode rather than test a diagnosis (reboot, power off, `loginctl terminate-*`, `nixos-rebuild`, deleting whole trees) are refused and counted. For diagnostics, the report must also **name the root cause** (regular expressions, reviewed by hand in the analysis); it is reported separately from success.

Every scenario is validated before it scores a model: with no agent its check fails (the fault took), and with the oracle's fix it passes with no collateral damage. An episode whose check passes before the agent starts is marked invalid and excluded.

## The agent

`harness/agent.py` is a plain tool-calling loop over the OpenAI chat API, with no framework, so the comparison is between models and not between scaffolds. Diagnostics get one `shell` tool (root, 60 s default timeout, output clipped to 6,000 characters) and `finish`. Computer use gets `click`, `drag`, `scroll`, `type_text`, `key`, `wait`, `look` and `finish`, acting through the compositor's input (`aizomectl input`) as a person's would, and sees a screenshot after every action (the last three are kept), the windows' element lists (`--observe elements`, for models without vision), or both. Models that write a tool call as text rather than through the API still get it run, and the rate is reported. With `--skill`, diagnostics also get [the troubleshooting skill](../../.agents/skills/sekka-troubleshooting/SKILL.md). Commands that stop the test driver's console (`backdoor.service`, which models take for malware) are refused too.

### oh-my-pi

`--agent omp` runs the diagnostics through [oh-my-pi](https://github.com/can1357/oh-my-pi) instead (the flake's pinned `omp` input, installed in the VM): `omp -p --mode json` as root inside the VM, against the same model server on the host (QEMU's user network reaches the host's loopback at 10.0.2.2), with omp's own system prompt and tools and the diagnostics brief appended. It runs with a 30-minute limit and no turn limit, and its event stream, without the streaming updates, is the transcript (`omp-events.jsonl`). Run it beside a bare-loop run against one server with more slots (`--no-serve`, `--port`), so the comparison costs no extra wall-clock time.

### Vendor agents through the bridge

`--agent bridge` gives an episode to an agent outside the VM, a vendor's own agent with its own account (`harness/bridge.py`).

- **The directory.** The driver prepares a directory under `$SEKKA_EVAL_BRIDGES`. It holds the brief (the bare loop's system prompt and the request), Tasuke's skills as reference notes (`$SEKKA_EVAL_BRIDGE_SKILLS`), and a `vm` command.
- **The requests.** Each `vm` request is a file that the driver runs through the bare loop's tools: `vm sh 'COMMAND'` for the root shell, and `vm look`, `click`, `type`, `key`, `scroll`, `drag` or `wait` on the desktop, with a screenshot and every control's centre. These carry the same refusals and output limits, and the same checks grade the episode.
- **The clock.** It starts at the agent's first request. The agent ends the episode with `vm finish 'REPORT'`. If it exits without finishing, its last message counts as its report.
- **`--runner codex`** starts `$SEKKA_EVAL_CODEX` (for example the Codex desktop app's own binary, for its login) in a bubblewrap sandbox, `harness/run-codex.sh`. The host is read-only, /run and $HOME are hidden but for Codex's state, its configuration is replaced by `--runner-model` and `--reasoning`, and the directory is the one writable place.
- **`--runner claude-code`** waits, for up to an hour, for a Claude Code session to be started in each directory with `ready`.

The labels are `MODEL+RUNNER`. The briefs say she has agreed to the changes a fix needs, the condition Tasuke's results are counted under after its one "yes".

### Screen coordinates

Vision models answer in their own coordinate conventions, whatever they are told: 0–1000 of each axis (Qwen3.x, Holo4, Muse Glimmer), 0–1 (Nemotron Bolt) or pixels (Gemma). `grounding.py MODEL` asks a running server where three known things are on a real screenshot and prints the convention that fits; put it in the model's `coords` in `models.toml` (or `--coords` for a run).

## Running

```sh
# Models and runtimes (docs/local-models.md).
nix run .#local-models.serve -- list
nix run .#local-models.serve -- download qwen3.8-27b

# Validate the scenarios, then evaluate (each model is started in turn).
nix run .#agent-evals.run -- --agent none
nix run .#agent-evals.run -- --agent oracle --track diagnostics
nix run .#agent-evals.run -- --models qwen3.8-27b,gpt-oss-20b --track diagnostics --repeats 2 --parallel 4
python3 tests/agent-evals/report.py .work/evals/runs/<run> > report.md
```

Episodes run in parallel, each in its own VM (4 GB, 4 cores) and runtime directory, all against one model server with as many slots. Four at once suit a 32 GB GPU. Results go under `.work/evals/runs/<run>/`: per episode `result.json`, `transcript.jsonl` (every request, response and tool output), screenshots and the driver's log.

`review.py RUN…` prints each episode's outcome, report and the commands that changed the machine, for reading results by hand; `speeds.py RUN…` tabulates the benchmarks recorded before each model's episodes; `report.py --jsonl FILE` exports the results.

A GPU with room for several models (the RTX PRO 6000's 96 GB holds three of these) can serve them at once on different ports, each with its own runner (`sekka-model-server start MODEL --port 8081 --parallel 8`, then `--no-serve --port 8081`); the machine needs about 3 GB of memory per VM.

`harness/demo.py` and `agent-evals.demo-driver` record a demonstration instead of grading: the machine on a 1920×1080 screen, a fault, then omp started in Hotaru and the user's complaint typed, as a person would, while Aizome's screen recorder runs ([docs/agent-evals](../../docs/agent-evals/README.md#the-demonstration)).

While changing the harness, `SEKKA_EVAL_LIVE_HARNESS=1` makes the packaged driver use the checkout's `harness/` without a rebuild. A changed `machine.nix` needs `nix build .#agent-evals.driver`.

## Adding a scenario

Subclass `Scenario` in `harness/scenarios.py` with `setup`, `check` and `oracle`, add it to `DIAGNOSTICS` or `COMPUTER_USE` with the user's words and the cause patterns, and run it with `--agent none` and `--agent oracle` before any model. Keep the complaint in the user's words: what they saw, not what broke. If the fixture needs something new in the VM, add it to `machine.nix` without breaking it.

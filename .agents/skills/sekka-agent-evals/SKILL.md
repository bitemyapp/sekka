---
name: sekka-agent-evals
description: Run, extend and interpret Sekka's agent evaluations — open models served locally (llama.cpp, vLLM, SGLang) diagnosing deliberately broken Sekka/Aizome VMs and operating the Aizome desktop. Use when evaluating a model or runtime, adding a diagnostic or computer-use scenario, or comparing local inference harnesses on the workstation GPU.
---

# Sekka agent evaluations

Read [tests/agent-evals/README.md](../../../tests/agent-evals/README.md) for the design and [docs/local-models.md](../../../docs/local-models.md) for runtimes and models. Results and their analysis live in [docs/agent-evals/](../../../docs/agent-evals/README.md).

## Before running

- GPU work is serial: one model server at a time on the RTX 5090 (32 GB). `nix run .#local-models.serve -- stop` before starting another runtime, and check `nvidia-smi` for leftovers (a vLLM or SGLang container holds the GPU until `docker stop`).
- Disk on runnymede is tight and Determinate Nix collects unrooted store paths when it fills. Root what a long run needs (`nix build .#agent-evals.driver -o .work/evals/driver`, `nix build .#local-models.llama-cpp -o .work/llama-cpp`), keep at most three models downloaded (`serve -- remove NAME` after a model's runs), and never delete paths you didn't create.
- Pick models by what fits and decodes at 10 tokens/s or better on the 5090, even when a bigger GPU (wx-workstation's RTX PRO 6000) runs the episodes: the evaluation is of what a Sekka user can run.
- VMs are disposable and fault injection happens only inside them. Never point a scenario's setup at the host.

## Validate first

Run every new or changed scenario both ways before any model sees it:

```sh
nix run .#agent-evals.run -- --agent none --scenarios NEW     # check must fail: the fault took
nix run .#agent-evals.run -- --agent oracle --scenarios NEW   # check must pass, no collateral
```

A computer-use task has no shell oracle: show it is achievable in the shipped UI (the control exists at the pinned revision) and that its check fails on the clean desktop.

## Running models

`nix run .#agent-evals.run -- --models A,B --track diagnostics --repeats 2 --parallel 4` starts each model in turn with `sekka-model-server`, records a decode benchmark beside the results, and runs the episodes four VMs at a time against its four slots. Use `--skill` for the troubleshooting-skill condition, `--observe screenshot|elements|both` for computer use (text-only models get `elements`), `--resume` to fill in failed or missing episodes, and `--no-serve --base-url URL` for a server you manage yourself. `report.py RUN…` prints the tables.

Each episode's directory holds `result.json`, `transcript.jsonl`, screenshots and `driver.log`. Read transcripts before drawing conclusions: a pass can hide a lucky workaround, and a failure can be the harness's (a refused command, a timeout, a server error).

## Adding a model

- Pin the repo, revision and files in `models.toml` with the vendor's sampling; check it fits with `serve -- start` and record its speed with `bench`.
- For vision models, measure the coordinate convention before any computer-use run: start the server and run `grounding.py MODEL`. Models answer in 0–1000, 0–1 or pixels regardless of the prompt; a wrong `coords` sends every click to a corner and measures nothing.
- Run `--agent omp` beside the bare loop against the same server (`--no-serve --port`), not after it.
- A bigger GPU can serve two or three models at once on separate ports; give each its own runner and keep its server alive until every episode against it has finished.

## Interpreting

- Success means the scenario's check passed and no invariant broke; "root cause named" is a regex over the final report and needs a hand audit of misses and hits before it is quoted.
- Report repeats and variance; with one or two repeats per scenario, differences of a scenario or two between models are noise.
- Separate the model from its serving: quantization, chat template, tool-call parser and thinking mode all change results. Record the runtime version, model revision and file, and server arguments (the bench JSON and `server-<model>.log` hold them).
- The skill condition is an upper bound: the same people wrote the skill and the scenarios.
- When an episode's result looks wrong, suspect the harness first and read the transcript: on 2026-11-04 a lost-output bug, a hung driver, virtiofs handle exhaustion, wrong coordinate conventions and truncated omp streams were each found that way. Fix it, validate the scenarios again, move affected results to `superseded-*` with the reason, and run them again. Keep results from before a change only where it could not have affected them, and say so.

## Adding a scenario

Write it in `harness/scenarios.py` (fault, user's words, check, cause patterns, oracle), add any fixture to `machine.nix`, rebuild the driver, validate both ways, then add a row to the README's table. Prefer faults that real Sekka users hit (the incident records in `docs/incidents/` are a good source) and complaints that describe symptoms, never causes.

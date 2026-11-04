# Open models as Sekka agents

Can a model that runs on one RTX 5090 fix a broken Sekka machine, or use the Aizome desktop for someone? This records what was run, how, and what came of it. The harness, machine and scenarios are described in [tests/agent-evals](../../tests/agent-evals/README.md); the runtimes and the choice of models in [local-models.md](../local-models.md).

RESULTS_SUMMARY

## What was measured

Sixteen diagnostic scenarios break a Sekka system with Aizome in a fresh VM (ten in the system, six in the desktop session) and give a model a root shell and the user's complaint in their own words. Nine computer-use tasks give it the screen, mouse and keyboard on a clean desktop. Each episode is graded by checking the machine afterwards: the problem is gone (or the task done) and nothing else was harmed. For diagnostics the report must also name the cause. Every scenario was first validated with no agent (its check fails) and with a known fix (its check passes, nothing else harmed).

All models were served by llama.cpp 0.8.0 (build 11960, CUDA 13.4, SM120) on runnymede's RTX 5090, four episodes at a time against four slots of a shared 128K-token q8_0 KV cache, with each model's recommended sampling. Diagnostics allowed 40 model turns and 20 minutes, computer use 35 turns. The agent loop is the same plain tool-calling loop for every model.

METHOD_DETAILS

## Limits

- Few repeats: two per scenario and model for most conditions. A difference of one or two scenarios between models is within run-to-run variation.
- The faults are injected locally and fixed in the running system; there is no `/etc/nixos` to change in the VM. On a real install some fixes would also belong in the configuration.
- The VM renders with pixman on a virtio GPU at 1440×900 and has no real audio, Wi-Fi, Bluetooth or printers.
- "Root cause named" is a set of regular expressions over the final report; hits and misses quoted here were checked by reading the transcripts.
- The troubleshooting skill was written by the same author as the scenarios, so its effect is an upper bound on what documentation gives.
- Vendor and third-party figures in local-models.md are claims; the numbers here are measured.

RUNS

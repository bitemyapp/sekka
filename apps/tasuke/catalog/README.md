# Tasuke's model catalog

`models.toml` lists the models Tasuke can recommend, download and run. Every
entry is a model Sekka's agent evaluations ran ([docs/agent-evals](../../../docs/agent-evals/README.md)),
pinned to the same files, so what a person downloads is what was measured.
`tasuke-models` reads it (`catalog.rs`); its tests check every entry.

## Fields

| Field | Meaning |
|---|---|
| `name`, `maker`, `lineage`, `origin` | Who made the model and what it was built from, and the countries of the makers; the Models page shows all of them, and `[recommend] exclude_lineage` in config.toml hides entries by any of them |
| `license`, `license_note` | The weights' licence, as published |
| `quant` | The quantization and who made the file |
| `roles` | `assistant` (diagnostics and conversation), `desktop` (operating the screen), `fast` |
| `evaluated` | Whether Sekka's evaluations ran it; unevaluated entries rank after evaluated ones |
| `architecture` | `dense` with `params` (billions), or `moe` with total and active parameters and expert layers, for estimating speed and expert offload |
| `context` | Default, minimum and maximum context in tokens; a fit shrinks toward the minimum before giving up |
| `kv` | KV cache bytes per token at f16 for the full-attention layers, and for the sliding-window layers with their window, plus any recurrent state; the fit scales them to q8_0, which Tasuke's server uses |
| `compute_buffer_mib` | llama.cpp's compute buffer at the default context and batch, measured |
| `vision` | Present for models that take images, with `coords`: how the model gives screen points (`pixel`, `norm1000`, `norm1`), measured with `tests/agent-evals/grounding.py` |
| `args`, `offload_args` | Extra llama-server arguments, and those used when experts are moved to system RAM |
| `keep_reasoning`, `sampling`, `template_kwargs` | Request settings the evaluations used for the model |
| `files` | Hugging Face repository, revision, path, size and sha256 of each file; downloads are checked against the sha256 before they are used |
| `scores` | Measured results with their date: `diagnostics` (the bare loop), `diagnostics_omp`, `computer_use` (screenshots) or `computer_use_text` (element lists), and `tasuke`/`computer_use_tasuke` when Tasuke itself was evaluated; a ranking uses Tasuke's own results only when every evaluated model it ranks has one, and the harnesses' for all of them otherwise, since Tasuke lifts every model it runs |
| `speed` | Single-stream decode and prefill on the reference GPU (`[reference]`), measured; other GPUs are estimated from memory bandwidth |
| `incidents` | Anything an evaluation saw the model do to the machine, in plain words; a model with incidents loses ties |
| `notes` | What the evaluations showed, for people |

`[[default]]` entries are the curated choices: per role, the model to offer
first on machines with at least `min_budget_gib` of room for a model
(92 % of the largest GPU's memory, less 1.5 GiB when it drives a display, or
unified memory less 8 GiB), with the reason shown. From 26 GiB, Gemma 4 31B
is the assistant and the desktop model, so one model does both; below that
the choice is Gemma 4 26B-A4B, which fits an 8 GB card with some layers'
experts in system RAM (measured by `tests/agent-evals/fit8gib.py`: 7.7 GiB
at 32K). gpt-oss 20B is the fast choice at every size. Where a choice
passes over a model that scored better somewhere, its reason names that
model and says why.

## Adding a model

Run it through the evaluations first (`tests/agent-evals`), then add its
entry with the same files, the scores and any incidents, and a note that a
person can act on. `cargo test -p tasuke-models` checks the catalog parses
and every entry is complete.

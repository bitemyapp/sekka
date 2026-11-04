# Open models on a local GPU

Which runtimes serve open models fastest on a Sekka workstation, and which models are worth running there for agent work. Measured on runnymede: an RTX 5090 (32 GB, SM120), a Ryzen 7 9800X3D, 60 GB of RAM, NixOS (Sekka), NVIDIA driver 615.78 with CUDA 13.4, in November 2026. How well the models do as agents is in [agent-evals](agent-evals/README.md).

`nix/local-models.nix` builds llama.cpp and `sekka-model-server`; `tests/agent-evals/models.toml` pins every model by revision and file:

```sh
nix run .#local-models.serve -- list
nix run .#local-models.serve -- download qwen3.8-27b
nix run .#local-models.serve -- start qwen3.8-27b --parallel 4 --ctx 49152 --total-ctx 131072
nix run .#local-models.serve -- bench
nix run .#local-models.serve -- stop
```

The server listens on `http://127.0.0.1:8080/v1` (OpenAI chat completions with tools and images), so any OpenAI-compatible client can use it. For opencode (1.19.5, as Sekka's catalog installs it), add a provider to `~/.config/opencode/opencode.json`; `opencode run -m sekka-local/qwen3.8-27b "…"` then drove the local server through its bash tool:

```json
{
  "$schema": "https://opencode.ai/config.json",
  "provider": {
    "sekka-local": {
      "npm": "@ai-sdk/openai-compatible",
      "name": "Sekka local models",
      "options": { "baseURL": "http://127.0.0.1:8080/v1" },
      "models": { "qwen3.8-27b": { "name": "Qwen3.8 27B (llama.cpp)", "tool_call": true } }
    }
  }
}
```

RUNTIMES_MEASURED

## Choosing a model

Candidates were chosen in November 2026 from model cards, the Hugging Face API (sizes, licences, revisions: verified) and third-party RTX 5090 measurements (not reproduced here unless measured below). Vendor benchmark figures are claims, listed to explain the choice, not results.

| Model | File (size) | Active / total parameters | Vision | Licence | Vendor's agentic claims |
|---|---|---|---|---|---|
| Qwen3.8-27B | unsloth UD-Q6_K (22.0 GB) + mmproj (0.9) | 27B dense, hybrid attention, MTP head | yes | Apache-2.0 | Terminal-Bench 2.1 73.0, OSWorld-Verified 84.3 |
| Qwen3.6-35B-A3B | unsloth UD-Q4_K_M (22.1) + mmproj | 3B / 35B MoE | yes | Apache-2.0 | Terminal-Bench 2.0 51.5 |
| Ornith-1.5-35B-A3B | Q4_K_M (21.7) + mmproj | 3B / 35B MoE (a Qwen3.6 fine-tune) | yes | MIT | Terminal-Bench 2.1 67.8 |
| Muse Glimmer 30B | Dynamic Q4_K_XL (19.7) + mmproj | 30B | yes | Apache-2.0 | Terminal-Bench 2.1 51.7, OSWorld-Verified 65.9 |
| Gemma 4 31B | Google's QAT Q4_0 (17.7) + mmproj | 31B dense | yes | Apache-2.0 | τ²-bench 76.9 |
| Nemotron 3.5 Bolt 74B-A9B | NVIDIA's 2-bit QAT GGUF (22.0) + mmproj | 9B / 74B MoE | yes | OpenMDW-1.1 | Terminal-Bench 2.1 51.0 |
| gpt-oss-20b | MXFP4 (12.1) | 3.6B / 21B MoE | no | Apache-2.0 | a baseline |
| Holo4-35B-A3B | Q4_K_M (21.3) + mmproj | 3B / 35B MoE (computer use) | yes | Apache-2.0 | OSWorld 80.8 |
| Holo4-27B | Q4_K_M (16.9) + mmproj | 27B dense (computer use) | yes | CC-BY-NC-4.0 | OSWorld 85.2 |

Left out: models too large for 32 GB at a useful precision (DeepSeek-V4-Flash, GLM-5.3-Flash, Qwen3.5-122B, Nemotron 3 Super, Qwen3.8-Flash-Next), older or weaker agents (Devstral Small 2, GLM-4.7-Flash, Laguna XS 2.1, OpenCUA, UI-TARS-1.5), and Ollama, which ignores `tool_choice`.

MODELS_MEASURED

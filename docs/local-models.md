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

## Runtimes

One model, Qwen3.8-27B, under each runtime, four request slots, a 24K-token cold prefill, then single and four concurrent 512-token answers at temperature 0 (`sekka-model-server bench`; raw JSON and server logs in `.work/evals/runtimes`). VRAM is the whole GPU's, including about 1.1 GB the desktop uses; "ready after" is from launch to a healthy server, with the weights already in the page cache. llama.cpp served unsloth's UD-Q6_K GGUF (22.0 GB, 6.6 bits per weight); vLLM and SGLang served unsloth's NVFP4 checkpoint (23.4 GB, about 4.5 bits): the formats each runtime is fastest with, so precision differs too.

| Runtime (version) | Weights | Speculation | Decode, 1 stream | Aggregate, 4 streams | Cold prefill (24K) | Next turn, cached | VRAM | Ready after |
|---|---|---|---:|---:|---:|---:|---:|---:|
| llama.cpp 0.8.0 | Q6_K | none | 64.7 tok/s | 206 tok/s | 3,330 tok/s | 0.26 s | 28.5 GB | 3 s |
| llama.cpp 0.8.0 | Q6_K | MTP, 2 drafts | **111.5** | 274 | 3,135 | 0.31 s | 31.0 GB | 3 s |
| llama.cpp 0.8.0 | Q6_K | MTP, 3 drafts | 105.8 | 295 | 3,005 | 0.31 s | 31.4 GB | 3 s |
| vLLM 0.32.0 | NVFP4 | none | 67.0 | 237 | 7,754 | 0.11 s | 30.1 GB | 2 min |
| vLLM 0.32.0 | NVFP4 | MTP, 2 drafts | 101.6 | **372** | 6,635 | 0.61 s | 30.5 GB | 2 min |
| SGLang 0.5.22 | NVFP4 | none | 41.5 | 83 | 7,694 | 0.05 s | 31.7 GB | 1 min |

- **One user, one agent: llama.cpp with MTP.** It decodes fastest for a single stream, at higher precision, starts in seconds, serves vision through its mmproj and runs every GGUF in the table below. Qwen3.8's Q6_K file carries its multi-token-prediction head, so `--spec-type draft-mtp` speeds it up 1.7× with no extra download; three drafts trade single-stream speed for batch throughput.
- **Several agents at once: vLLM.** With MTP it delivers 36% more aggregate throughput at four streams than llama.cpp and prefills twice as fast, at the cost of NVFP4's lower precision, two minutes to start, and a container. Stay on 0.32.0: 0.33.0 crashes with NVFP4 and MTP on SM120 ([vllm#65139](https://github.com/vllm-project/vllm/issues/65139)).
- **SGLang 0.5.22 isn't competitive here yet.** On SM120 it runs Qwen3.8's linear-attention layers with Triton kernels, and its Mamba state cache capped it at two running requests (its log says so), so the second pair of streams waited six seconds to start.
- **Not measured:** ExLlamaV3/TabbyAPI (supports Qwen3.8 with vision; its quants and container would have needed another ~30 GB of disk), TensorRT-LLM (release candidates only for this GPU), and Ollama, whose OpenAI endpoint ignores `tool_choice`. ik_llama.cpp and KTransformers target models that don't fit in VRAM.

The agent evaluations use llama.cpp throughout, so every model is compared at the same runtime, with GGUF files of similar precision.

vLLM and SGLang run in their upstream images under rootless Docker without the NVIDIA container toolkit: `sekka-model-server` passes `/dev/nvidia0`, `/dev/nvidiactl` and `/dev/nvidia-uvm` and mounts the host driver's library directory (NixOS keeps it in the store) at `/usr/local/nvidia/lib64`.

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

## Measured speed of each model

Each model as the agent evaluations served it: llama.cpp 0.8.0, the file in the table, four slots, q8_0 KV cache, measured just before its episodes with `sekka-model-server bench` (Qwen3.8-27B with MTP, from the runtime comparison above). Every model clears the 10 tokens/s bar by at least seven times; the mixture-of-experts models with about 3B active parameters decode three times faster than the dense 27–31B ones. Two measurements of Holo4-35B's same file, hours apart, gave 178 and 255 tokens/s single-stream: allow for that much spread between runs.

| Model | File | Decode, 1 stream | Aggregate, 4 streams | Prefill, ~20K tokens | Next turn, cached |
|---|---|---:|---:|---:|---:|
| gpt-oss-20b | gpt-oss-20b-MXFP4.gguf (12.1 GB) | 331 tok/s | 662 tok/s | 17,728 tok/s | 0.05 s |
| holo4-35b-a3b | q4_k_m.gguf (22.2 GB) | 255 tok/s | 636 tok/s | 8,064 tok/s | 0.12 s |
| ornith-1.5-35b-a3b | Ornith-1.5-35B-Q4_K_M.gguf (22.6 GB) | 248 tok/s | 632 tok/s | 8,260 tok/s | 0.11 s |
| qwen3.6-35b-a3b | Qwen3.6-35B-A3B-UD-Q4_K_M.gguf (23.0 GB) | 231 tok/s | 525 tok/s | 8,692 tok/s | 0.11 s |
| nemotron-3.5-bolt | NVIDIA-Nemotron-3.5-Bolt-74B-A9B-24GB.gguf (23.8 GB) | 153 tok/s | 236 tok/s | 5,038 tok/s | 0.21 s |
| qwen3.8-27b | Qwen3.8-27B-UD-Q6_K.gguf (22.9 GB) | 112 tok/s | 274 tok/s | 3,135 tok/s | 0.31 s |
| holo4-27b | q4_k_m.gguf (17.8 GB) | 78 tok/s | 228 tok/s | 3,636 tok/s | 0.25 s |
| gemma-4-31b | gemma-4-31B_q4_0-it.gguf (18.9 GB) | 70 tok/s | 221 tok/s | 2,958 tok/s | 0.33 s |
| muse-glimmer-30b | Muse-Glimmer-30B-KQuant-Dynamic-Q4_K_XL.gguf (21.1 GB) | 69 tok/s | 230 tok/s | 3,566 tok/s | 0.12 s |

Which of them make good agents is in [agent-evals](agent-evals/README.md#results).

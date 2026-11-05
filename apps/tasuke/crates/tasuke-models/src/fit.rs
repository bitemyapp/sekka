//! Whether, and how, a catalog model fits this machine.
//!
//! A model needs its weights, the KV cache for its context, llama.cpp's
//! compute buffer and, for a vision model, the mmproj. The placements, in
//! order of preference:
//!
//! 1. **On a GPU** with dedicated memory, when all of it is within 92% of
//!    the GPU's memory, less 1.5 GiB on a GPU that drives a display.
//! 2. **On a GPU with experts in RAM** (`-ncmoe N`): a mixture of experts
//!    whose dense part, KV and some layers' experts fit on the GPU, with the
//!    other layers' experts in system RAM. The fewest layers that fit are
//!    moved.
//! 3. **On unified memory** (an APU, a GB10): the budget is RAM less 8 GiB,
//!    capped by what the driver lets the GPU map.
//! 4. **On the CPU**, in RAM less 8 GiB.
//!
//! Each placement first tries the requested context, then shrinks it (to a
//! multiple of 4K) down to the entry's minimum.
//!
//! # Free memory or total memory
//!
//! Recommendations plan against a GPU's *total* memory ([`MemoryBasis::Total`]),
//! not what is free at the moment: free memory changes with whatever else is
//! running (a game, Tasuke's own loaded model, another llama-server), and a
//! recommendation shouldn't. [`MemoryBasis::Free`] answers "would it load right
//! now" instead.
//!
//! # Compute buffer
//!
//! llama.cpp allocates a compute buffer for the graph's intermediate tensors,
//! sized by the micro-batch, the context and the vocabulary. Each catalog entry
//! records its measured size at the default context (llama-server's
//! `CUDA0 compute buffer size` plus the vision encoder's); see
//! catalog/README.md for how. Between entries it varies from about 0.3 to
//! 1.5 GiB; [`COMPUTE_SCALE_PER_CONTEXT`] grows it with longer contexts.

use serde::{Deserialize, Serialize};

use crate::catalog::{Architecture, Catalog, Entry, Reference};
use crate::hardware::{Gpu, GpuIndex, GpuMemory, Hardware};
use crate::units::{Bytes, BytesPerSecond, ContextTokens, ModelId, TokensPerSecond};

/// A model fits on a GPU when it needs at most this share of its memory.
pub const GPU_MARGIN: f64 = 0.92;
/// What a GPU that drives a display keeps back for the desktop.
pub const DISPLAY_RESERVE: Bytes = Bytes::mib(1536);
/// What unified memory and CPU placements leave to the rest of the system.
pub const SYSTEM_RESERVE: Bytes = Bytes::gib(8);
/// System RAM bandwidth assumed for speed estimates when it isn't known
/// (dual-channel DDR5, what llama.cpp achieves rather than the peak).
pub const ASSUMED_RAM_BANDWIDTH: BytesPerSecond = BytesPerSecond(60e9);
/// Share of memory bandwidth decode achieves, for models without a measured
/// speed (dense models on the RTX 5090 reach about 0.7).
pub const DECODE_EFFICIENCY: f64 = 0.6;
/// How much of the compute buffer grows with context beyond the default:
/// llama.cpp's attention scratch is proportional to the context.
pub const COMPUTE_SCALE_PER_CONTEXT: f64 = 0.5;
/// Contexts are chosen in steps of this many tokens.
pub const CONTEXT_STEP: u32 = 4096;
/// The micro-batch: sliding-window caches hold this many tokens beyond the window.
pub const UBATCH: u32 = 512;

/// The KV cache's element type. Tasuke runs q8_0, as the evaluations did.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum KvCacheType {
    F16,
    #[default]
    Q8_0,
}

impl KvCacheType {
    /// Bytes per element relative to f16 (q8_0: 34 bytes per 32 values).
    fn scale(self) -> f64 {
        match self {
            KvCacheType::F16 => 1.0,
            KvCacheType::Q8_0 => 34.0 / 64.0,
        }
    }

    pub fn llama_name(self) -> &'static str {
        match self {
            KvCacheType::F16 => "f16",
            KvCacheType::Q8_0 => "q8_0",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MemoryBasis {
    /// Plan against the GPU's total memory (recommendations).
    #[default]
    Total,
    /// Against what is free now (will it load right now?).
    Free,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct FitOptions {
    /// The context wanted; the entry's default when `None`.
    pub context: Option<ContextTokens>,
    pub kv: KvCacheType,
    pub basis: MemoryBasis,
}

/// How many layers' routed experts stay in system RAM (`-ncmoe`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ExpertLayers(pub u32);

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Fit {
    Gpu {
        gpu: GpuIndex,
        ctx: ContextTokens,
        need: Bytes,
        budget: Bytes,
    },
    GpuWithExpertOffload {
        gpu: GpuIndex,
        ctx: ContextTokens,
        cpu_expert_layers: ExpertLayers,
        gpu_need: Bytes,
        gpu_budget: Bytes,
        ram_need: Bytes,
        ram_budget: Bytes,
    },
    Unified {
        gpu: GpuIndex,
        ctx: ContextTokens,
        need: Bytes,
        budget: Bytes,
    },
    Cpu {
        ctx: ContextTokens,
        need: Bytes,
        budget: Bytes,
    },
    TooLarge {
        /// What it needs at its minimum context.
        needed: Bytes,
        /// The largest budget on this machine.
        available: Bytes,
    },
}

impl Fit {
    pub fn fits(&self) -> bool {
        !matches!(self, Fit::TooLarge { .. })
    }

    pub fn context(&self) -> Option<ContextTokens> {
        match *self {
            Fit::Gpu { ctx, .. } | Fit::GpuWithExpertOffload { ctx, .. } | Fit::Unified { ctx, .. } | Fit::Cpu { ctx, .. } => {
                Some(ctx)
            }
            Fit::TooLarge { .. } => None,
        }
    }

    pub fn gpu(&self) -> Option<GpuIndex> {
        match *self {
            Fit::Gpu { gpu, .. } | Fit::GpuWithExpertOffload { gpu, .. } | Fit::Unified { gpu, .. } => Some(gpu),
            _ => None,
        }
    }

    /// Preference order of placements: lower is better.
    pub fn rank(&self) -> u8 {
        match self {
            Fit::Gpu { .. } => 0,
            Fit::Unified { .. } => 1,
            Fit::GpuWithExpertOffload { .. } => 2,
            Fit::Cpu { .. } => 3,
            Fit::TooLarge { .. } => 4,
        }
    }

    /// In words, for people: "fits on the RTX 5090 with 64K of context".
    pub fn describe(&self, hw: &Hardware) -> String {
        let gpu_name = |g: GpuIndex| hw.gpu(g).map(|g| g.name.clone()).unwrap_or_else(|| "the GPU".to_owned());
        match *self {
            Fit::Gpu { gpu, ctx, need, .. } => format!("on the {} ({need}), {ctx} context", gpu_name(gpu)),
            Fit::GpuWithExpertOffload { gpu, ctx, cpu_expert_layers, gpu_need, ram_need, .. } => format!(
                "on the {} ({gpu_need}) with {} layers' experts in RAM ({ram_need}), {ctx} context",
                gpu_name(gpu),
                cpu_expert_layers.0
            ),
            Fit::Unified { gpu, ctx, need, .. } => format!("in unified memory on the {} ({need}), {ctx} context", gpu_name(gpu)),
            Fit::Cpu { ctx, need, .. } => format!("on the CPU in RAM ({need}), {ctx} context"),
            Fit::TooLarge { needed, available } => format!("too large: needs {needed}, this machine has {available}"),
        }
    }
}

/// One entry's fit, as [`Catalog::fits`] lists them.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ModelFit {
    pub id: ModelId,
    pub fit: Fit,
    /// Rough single-stream decode speed in this placement.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub estimated_decode: Option<TokensPerSecond>,
}

/// What an entry needs, by part, at a context.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Need {
    pub weights: Bytes,
    pub kv: Bytes,
    pub compute: Bytes,
    pub mmproj: Bytes,
}

impl Need {
    pub fn total(&self) -> Bytes {
        self.weights + self.kv + self.compute + self.mmproj
    }
}

/// The memory an entry needs at `ctx`.
pub fn need(entry: &Entry, ctx: ContextTokens, kv: KvCacheType) -> Need {
    let spec = entry.kv;
    let full = (spec.per_token * ctx).scale(kv.scale());
    let swa_tokens = ContextTokens(ctx.0.min(spec.swa_window.0.saturating_add(UBATCH)));
    let swa = (spec.swa_per_token * swa_tokens).scale(kv.scale());
    let default = entry.context.default.0.max(1) as f64;
    let grow = 1.0 + COMPUTE_SCALE_PER_CONTEXT * ((ctx.0 as f64 / default) - 1.0).max(0.0);
    Need {
        weights: entry.weights_size(),
        kv: full + swa + spec.recurrent,
        compute: entry.compute_buffer.scale(grow),
        mmproj: entry.mmproj().map(|f| f.size).unwrap_or_default(),
    }
}

/// The contexts to try, largest first: the wanted one, then down to the minimum.
fn contexts(entry: &Entry, wanted: ContextTokens) -> impl Iterator<Item = ContextTokens> {
    let wanted = wanted.clamp(entry.context.min, entry.context.max);
    let min = entry.context.min;
    let mut next = Some(wanted);
    std::iter::from_fn(move || {
        let c = next?;
        let lower = (c.0.saturating_sub(1) / CONTEXT_STEP) * CONTEXT_STEP;
        // Halve quickly at first, then step: 128K, 64K, 32K, 28K…
        let halved = (c.0 / 2 / CONTEXT_STEP) * CONTEXT_STEP;
        let step = if halved >= min.0 && halved > 32 * 1024 { halved } else { lower };
        next = (step >= min.0 && step < c.0).then_some(ContextTokens(step));
        Some(c)
    })
}

/// A GPU's budget under `basis`.
pub fn gpu_budget(gpu: &Gpu, basis: MemoryBasis) -> Option<Bytes> {
    match gpu.memory {
        GpuMemory::Dedicated { total, free } => Some(match basis {
            MemoryBasis::Total => {
                total.scale(GPU_MARGIN) - if gpu.drives_display { DISPLAY_RESERVE } else { Bytes::ZERO }
            }
            // Free memory already excludes what the desktop uses now.
            MemoryBasis::Free => free.scale(GPU_MARGIN),
        }),
        _ => None,
    }
}

/// Unified memory's budget: RAM less the system reserve, capped by the GPU's
/// mapping limit.
pub fn unified_budget(gpu: &Gpu, hw: &Hardware) -> Option<Bytes> {
    match gpu.memory {
        GpuMemory::Unified { limit } => {
            let ram = hw.ram.total - SYSTEM_RESERVE;
            Some(limit.map_or(ram, |l| l.min(ram)))
        }
        _ => None,
    }
}

pub fn ram_budget(hw: &Hardware) -> Bytes {
    hw.ram.total - SYSTEM_RESERVE
}

/// How `entry` fits `hw`.
pub fn fit(entry: &Entry, hw: &Hardware, opts: FitOptions) -> Fit {
    let wanted = opts.context.unwrap_or(entry.context.default);
    // Dedicated GPUs, largest budget first.
    let mut dedicated: Vec<(GpuIndex, Bytes)> = hw
        .gpus
        .iter()
        .enumerate()
        .filter_map(|(i, g)| gpu_budget(g, opts.basis).map(|b| (GpuIndex(i), b)))
        .collect();
    dedicated.sort_by_key(|&(_, b)| std::cmp::Reverse(b));
    let unified: Vec<(GpuIndex, Bytes)> =
        hw.gpus.iter().enumerate().filter_map(|(i, g)| unified_budget(g, hw).map(|b| (GpuIndex(i), b))).collect();
    let ram = ram_budget(hw);

    for ctx in contexts(entry, wanted) {
        let n = need(entry, ctx, opts.kv).total();
        if let Some(&(gpu, budget)) = dedicated.iter().find(|&&(_, b)| n <= b) {
            return Fit::Gpu { gpu, ctx, need: n, budget };
        }
        if let Some(&(gpu, budget)) = unified.iter().find(|&&(_, b)| n <= b) {
            return Fit::Unified { gpu, ctx, need: n, budget };
        }
    }
    if let Architecture::Moe { expert_layers, expert_bytes, .. } = entry.architecture
        && expert_layers > 0
    {
        let per_layer = Bytes(expert_bytes.0 / u64::from(expert_layers));
        for ctx in contexts(entry, wanted) {
            let n = need(entry, ctx, opts.kv).total();
            for &(gpu, gpu_budget) in &dedicated {
                // The fewest layers that bring the GPU part within budget.
                let over = n.saturating_sub(gpu_budget);
                let layers = over.0.div_ceil(per_layer.0.max(1)) as u32;
                if layers > expert_layers {
                    continue;
                }
                let moved = Bytes(per_layer.0 * u64::from(layers));
                // llama.cpp keeps a host copy of the compute buffer for the CPU part.
                let ram_need = moved + entry.compute_buffer.scale(0.5);
                if ram_need <= ram {
                    return Fit::GpuWithExpertOffload {
                        gpu,
                        ctx,
                        cpu_expert_layers: ExpertLayers(layers),
                        gpu_need: n - moved,
                        gpu_budget,
                        ram_need,
                        ram_budget: ram,
                    };
                }
            }
        }
    }
    for ctx in contexts(entry, wanted) {
        // No mmproj offload and no GPU compute buffer, but the CPU needs one too.
        let n = need(entry, ctx, opts.kv).total();
        if n <= ram {
            return Fit::Cpu { ctx, need: n, budget: ram };
        }
    }
    let available = dedicated.iter().map(|&(_, b)| b).chain(unified.iter().map(|&(_, b)| b)).chain([ram]).max();
    Fit::TooLarge { needed: need(entry, entry.context.min, opts.kv).total(), available: available.unwrap_or_default() }
}

/// Rough single-stream decode speed of `entry` placed as `fit`. Measured
/// speeds scale with memory bandwidth from the reference GPU; models without a
/// measurement get bandwidth × [`DECODE_EFFICIENCY`] over the bytes read per
/// token. `None` when a bandwidth on the way isn't known.
pub fn estimate_decode(entry: &Entry, fit: &Fit, hw: &Hardware, reference: &Reference) -> Option<TokensPerSecond> {
    let reference_bw = BytesPerSecond::gb_per_s(reference.bandwidth_gb_s);
    let active = entry.weights_size().scale(entry.architecture.active_fraction());
    let from_bandwidth = |bw: BytesPerSecond| -> TokensPerSecond {
        match &entry.speed {
            Some(s) => TokensPerSecond(s.decode.0 * (bw.0 / reference_bw.0).min(1.0) as f32),
            None => TokensPerSecond((bw.0 * DECODE_EFFICIENCY / active.0.max(1) as f64) as f32),
        }
    };
    let gpu_bw = |g: GpuIndex| hw.gpu(g).and_then(|g| g.bandwidth);
    match *fit {
        Fit::Gpu { gpu, .. } | Fit::Unified { gpu, .. } => gpu_bw(gpu).map(from_bandwidth),
        Fit::GpuWithExpertOffload { gpu, cpu_expert_layers, .. } => {
            let Architecture::Moe { expert_layers, expert_bytes, .. } = entry.architecture else { return None };
            let gpu_bw = gpu_bw(gpu)?;
            // Per token, the active share of the moved experts is read from RAM.
            let moved = expert_bytes.0 as f64 * f64::from(cpu_expert_layers.0) / f64::from(expert_layers.max(1));
            let ram_bytes = moved * entry.architecture.expert_read_fraction();
            let gpu_bytes = (active.0 as f64 - ram_bytes).max(0.0);
            let secs = gpu_bytes / (gpu_bw.0 * DECODE_EFFICIENCY) + ram_bytes / (ASSUMED_RAM_BANDWIDTH.0 * DECODE_EFFICIENCY);
            Some(TokensPerSecond((1.0 / secs) as f32))
        }
        Fit::Cpu { .. } => {
            Some(TokensPerSecond((ASSUMED_RAM_BANDWIDTH.0 * DECODE_EFFICIENCY / active.0.max(1) as f64) as f32))
        }
        Fit::TooLarge { .. } => None,
    }
}

impl Catalog {
    /// Every entry's fit on `hw`, in catalog order.
    pub fn fits(&self, hw: &Hardware) -> Vec<ModelFit> {
        self.fits_with(hw, FitOptions::default())
    }

    pub fn fits_with(&self, hw: &Hardware, opts: FitOptions) -> Vec<ModelFit> {
        self.models
            .iter()
            .map(|e| {
                let fit = fit(e, hw, opts);
                ModelFit { id: e.id.clone(), estimated_decode: estimate_decode(e, &fit, hw, &self.reference), fit }
            })
            .collect()
    }
}

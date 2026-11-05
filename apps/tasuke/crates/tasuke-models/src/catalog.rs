//! The curated model catalog (`apps/tasuke/catalog/models.toml`): what each
//! model is, who made it, its pinned files, what it costs in memory, how to
//! serve it, and how it did in Sekka's agent evaluations.

use std::collections::BTreeMap;
use std::fmt;

use serde::{Deserialize, Serialize};

use crate::units::{BillionParams, Bytes, BytesPerToken, ContextTokens, ModelId, RepoId, Revision, Sha256, TokensPerSecond};

/// The catalog shipped with Tasuke.
pub const DEFAULT_CATALOG: &str = include_str!("../../../catalog/models.toml");

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Catalog {
    /// Where the speeds were measured, e.g. "RTX 5090".
    pub reference: Reference,
    /// The curated default per role and memory tier, with the reason.
    #[serde(default, rename = "default")]
    pub defaults: Vec<CuratedDefault>,
    /// Entries in catalog order, keyed by id in the file.
    #[serde(with = "entries_map")]
    pub models: Vec<Entry>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Reference {
    pub gpu: String,
    /// Its memory bandwidth, GB/s: speeds elsewhere scale from it.
    pub bandwidth_gb_s: f64,
    /// When the evaluations the scores come from ran.
    pub evaluated: String,
}

/// "On a machine whose best budget is at least `min_budget`, the default for
/// `role` is `model`, because `why`."
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CuratedDefault {
    pub role: Role,
    #[serde(rename = "min_budget_gib")]
    #[serde(with = "gib")]
    pub min_budget: Bytes,
    pub model: ModelId,
    pub why: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    /// Diagnostics, fixes and conversation.
    Assistant,
    /// Computer use from screenshots (vision models).
    Desktop,
    /// Quick answers: the fastest model worth using.
    Fast,
}

impl Role {
    pub const ALL: [Role; 3] = [Role::Assistant, Role::Desktop, Role::Fast];

    pub fn as_str(self) -> &'static str {
        match self {
            Role::Assistant => "assistant",
            Role::Desktop => "desktop",
            Role::Fast => "fast",
        }
    }
}

impl fmt::Display for Role {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl std::str::FromStr for Role {
    type Err = String;
    fn from_str(s: &str) -> Result<Role, String> {
        match s {
            "assistant" => Ok(Role::Assistant),
            "desktop" => Ok(Role::Desktop),
            "fast" => Ok(Role::Fast),
            _ => Err(format!("unknown role {s:?} (assistant, desktop or fast)")),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Entry {
    #[serde(skip_deserializing, default = "placeholder_id")]
    pub id: ModelId,
    pub name: String,
    /// Who trained this model.
    pub maker: String,
    /// The families it descends from, the maker's own first: `["H Company",
    /// "Qwen (Alibaba)"]`. What `[recommend] exclude_lineage` matches.
    pub lineage: Vec<String>,
    /// Where those makers are, `["France", "China"]`; also matched.
    pub origin: Vec<String>,
    pub license: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub license_note: Option<String>,
    /// The quantization of the weights file, as its maker names it.
    pub quant: String,
    pub roles: Vec<Role>,
    /// Whether Sekka's agent evaluations have run it (scores are measured).
    pub evaluated: bool,
    pub architecture: Architecture,
    pub context: ContextSpec,
    pub kv: KvSpec,
    /// llama.cpp's compute buffer at the default context and batch, measured.
    #[serde(rename = "compute_buffer_mib", with = "mib")]
    pub compute_buffer: Bytes,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub vision: Option<Vision>,
    /// Extra llama-server arguments, always applied.
    #[serde(default)]
    pub args: Vec<String>,
    /// Extra arguments when some experts are in RAM (`-ncmoe`): larger
    /// batches, so prefill moves the experts across PCIe less often.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub offload_args: Vec<String>,
    /// The client should send the model's earlier reasoning back with the
    /// conversation (gpt-oss's harmony format needs it between tool calls).
    #[serde(default)]
    pub keep_reasoning: bool,
    /// Sampling defaults (llama-server's --temp, --top-p, …) by request field name.
    #[serde(default)]
    pub sampling: BTreeMap<String, toml::Value>,
    /// Extra `chat_template_kwargs` llama-server applies.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub template_kwargs: BTreeMap<String, toml::Value>,
    pub files: Vec<ModelFile>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scores: Option<Scores>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub speed: Option<MeasuredSpeed>,
    /// Destructive things this model did in the evaluations, in words.
    #[serde(default)]
    pub incidents: Vec<String>,
    /// Other honest notes: strengths, weaknesses, licence questions.
    #[serde(default)]
    pub notes: Vec<String>,
}

fn placeholder_id() -> ModelId {
    ModelId(String::new())
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum Architecture {
    Dense {
        params: BillionParams,
    },
    /// A mixture of experts. `expert_layers` layers hold routed experts,
    /// `expert_bytes` in all: `-ncmoe N` keeps N layers' experts in RAM.
    Moe {
        total: BillionParams,
        active: BillionParams,
        expert_layers: u32,
        #[serde(rename = "expert_mib", with = "mib")]
        expert_bytes: Bytes,
    },
}

impl Architecture {
    pub fn is_moe(&self) -> bool {
        matches!(self, Architecture::Moe { .. })
    }

    /// The share of the weights read for each token.
    pub fn active_fraction(&self) -> f64 {
        match *self {
            Architecture::Dense { .. } => 1.0,
            Architecture::Moe { total, active, .. } => (active.0 / total.0).clamp(0.0, 1.0) as f64,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContextSpec {
    /// What Tasuke runs it with unless the user says otherwise.
    pub default: ContextTokens,
    /// What it was trained for.
    pub max: ContextTokens,
    /// The least that's still useful for agent work; fit shrinks the context
    /// down to this before giving up on a placement.
    pub min: ContextTokens,
}

/// The KV cache's cost at f16. Full-attention layers grow with the context;
/// sliding-window layers stop at their window; recurrent layers (Mamba,
/// linear attention) have a fixed state per sequence.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct KvSpec {
    /// f16 K+V bytes per token over the full-attention layers.
    pub per_token: BytesPerToken,
    /// f16 K+V bytes per token over the sliding-window layers.
    #[serde(default)]
    pub swa_per_token: BytesPerToken,
    #[serde(default)]
    pub swa_window: ContextTokens,
    /// Recurrent state per sequence, MiB (f32, not quantized).
    #[serde(default, rename = "recurrent_mib", with = "mib")]
    pub recurrent: Bytes,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Vision {
    pub coords: Coords,
}

/// How a vision model gives screen points.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Coords {
    /// Pixels of the screenshot.
    Pixel,
    /// 0–1000 on each axis (Qwen3.x-VL, Holo).
    Norm1000,
    /// 0–1 on each axis (Nemotron Bolt).
    Norm1,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum FileKind {
    Weights,
    Mmproj,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModelFile {
    pub kind: FileKind,
    pub repo: RepoId,
    pub revision: Revision,
    /// The path in the repository, and the file name on disk.
    pub path: String,
    pub size: Bytes,
    pub sha256: Sha256,
}

impl ModelFile {
    /// The name the file has in the store: the last path component.
    pub fn file_name(&self) -> &str {
        self.path.rsplit('/').next().unwrap_or(&self.path)
    }
}

/// Passed out of total, from one evaluation track.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Tally {
    pub passed: u32,
    pub total: u32,
}

impl Tally {
    pub fn share(self) -> f64 {
        if self.total == 0 { 0.0 } else { f64::from(self.passed) / f64::from(self.total) }
    }

    pub fn plus(self, other: Tally) -> Tally {
        Tally { passed: self.passed + other.passed, total: self.total + other.total }
    }
}

impl fmt::Display for Tally {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}/{} ({:.0}%)", self.passed, self.total, self.share() * 100.0)
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Scores {
    pub date: String,
    /// Diagnostics in Tasuke itself (`run.py --agent tasuke`): the product's
    /// own agent, prompt, skills and policy. What the ranking prefers.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tasuke: Option<Tally>,
    /// Desktop tasks in Tasuke, with set-of-marks.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub computer_use_tasuke: Option<Tally>,
    /// Diagnostics in the evaluations' bare tool loop.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub diagnostics: Option<Tally>,
    /// Diagnostics in oh-my-pi.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub diagnostics_omp: Option<Tally>,
    /// Desktop tasks from screenshots.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub computer_use: Option<Tally>,
    /// Desktop tasks from element lists (text-only models).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub computer_use_text: Option<Tally>,
}

impl Scores {
    /// What the assistant role ranks by: Tasuke's own result when measured,
    /// else both evaluation harnesses pooled.
    pub fn assistant(&self) -> Option<Tally> {
        self.tasuke.or_else(|| self.diagnostics_pooled())
    }

    /// What the desktop role ranks by: Tasuke's own result when measured.
    pub fn desktop(&self) -> Option<Tally> {
        self.computer_use_tasuke.or(self.computer_use).or(self.computer_use_text)
    }

    /// Both diagnostics harnesses pooled.
    pub fn diagnostics_pooled(&self) -> Option<Tally> {
        match (self.diagnostics, self.diagnostics_omp) {
            (Some(a), Some(b)) => Some(a.plus(b)),
            (a, b) => a.or(b),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct MeasuredSpeed {
    /// Single-stream decode on the reference GPU.
    pub decode: TokensPerSecond,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prefill: Option<TokensPerSecond>,
}

#[derive(Debug, thiserror::Error)]
pub enum CatalogError {
    #[error("the catalog isn't valid TOML: {0}")]
    Toml(#[from] toml::de::Error),
    #[error("catalog entry {id}: {problem}")]
    Entry { id: ModelId, problem: String },
}

impl Catalog {
    /// The catalog Tasuke ships with.
    pub fn load_default() -> Catalog {
        Catalog::from_toml(DEFAULT_CATALOG).expect("the built-in catalog is checked by its tests")
    }

    pub fn from_toml(text: &str) -> Result<Catalog, CatalogError> {
        let catalog: Catalog = toml::from_str(text)?;
        catalog.validate()?;
        Ok(catalog)
    }

    pub fn get(&self, id: &ModelId) -> Option<&Entry> {
        self.models.iter().find(|e| &e.id == id)
    }

    /// Structural checks: ids, files, defaults that name real entries.
    fn validate(&self) -> Result<(), CatalogError> {
        let bad = |id: &ModelId, problem: String| Err(CatalogError::Entry { id: id.clone(), problem });
        for e in &self.models {
            if !e.id.is_valid() {
                return bad(&e.id, "the id must be letters, digits, '-', '.' or '_'".into());
            }
            if e.files.iter().filter(|f| f.kind == FileKind::Weights).count() == 0 {
                return bad(&e.id, "no weights file".into());
            }
            if e.vision.is_some() != e.files.iter().any(|f| f.kind == FileKind::Mmproj) {
                return bad(&e.id, "vision needs exactly when an mmproj file is listed".into());
            }
            if e.roles.contains(&Role::Desktop) && e.vision.is_none() {
                return bad(&e.id, "the desktop role needs vision".into());
            }
            for f in &e.files {
                if f.path.is_empty() || f.path.contains("..") || f.size == Bytes::ZERO || f.revision.0.len() != 40 {
                    return bad(&e.id, format!("file {:?} needs a path, a size and a 40-digit revision", f.path));
                }
            }
            if e.evaluated != e.scores.is_some() {
                return bad(&e.id, "evaluated entries have scores, unevaluated ones none".into());
            }
            let c = e.context;
            if !(c.min <= c.default && c.default <= c.max) {
                return bad(&e.id, "context needs min ≤ default ≤ max".into());
            }
        }
        for d in &self.defaults {
            match self.get(&d.model) {
                None => return bad(&d.model, "a default names a model not in the catalog".into()),
                Some(e) if !e.roles.contains(&d.role) => {
                    return bad(&d.model, format!("the default for {} doesn't have that role", d.role));
                }
                Some(_) => {}
            }
        }
        Ok(())
    }
}

impl Entry {
    pub fn weights_size(&self) -> Bytes {
        self.files.iter().filter(|f| f.kind == FileKind::Weights).map(|f| f.size).sum()
    }

    pub fn mmproj(&self) -> Option<&ModelFile> {
        self.files.iter().find(|f| f.kind == FileKind::Mmproj)
    }

    pub fn download_size(&self) -> Bytes {
        self.files.iter().map(|f| f.size).sum()
    }

    /// Whether `tag` (case-insensitive) names this entry's maker, one of its
    /// lineage families, or one of their origins.
    pub fn matches_lineage(&self, tag: &str) -> bool {
        let tag = tag.trim().to_lowercase();
        if tag.is_empty() {
            return false;
        }
        let hit = |s: &String| {
            let s = s.to_lowercase();
            s == tag || s.split(|c: char| !c.is_alphanumeric()).any(|w| w == tag)
        };
        hit(&self.maker) || self.lineage.iter().any(hit) || self.origin.iter().any(hit)
    }

    /// No destructive incidents in the evaluations.
    pub fn trusted(&self) -> bool {
        self.incidents.is_empty()
    }
}

/// `[models."id"]` tables, kept in file order.
mod entries_map {
    use super::*;
    use serde::de::{Deserializer, MapAccess, Visitor};
    use serde::ser::{SerializeMap, Serializer};

    pub fn serialize<S: Serializer>(entries: &[Entry], s: S) -> Result<S::Ok, S::Error> {
        let mut map = s.serialize_map(Some(entries.len()))?;
        for e in entries {
            map.serialize_entry(&e.id, e)?;
        }
        map.end()
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Vec<Entry>, D::Error> {
        struct V;
        impl<'de> Visitor<'de> for V {
            type Value = Vec<Entry>;
            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("a table of models")
            }
            fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Vec<Entry>, A::Error> {
                let mut out = Vec::new();
                while let Some((id, mut entry)) = map.next_entry::<ModelId, Entry>()? {
                    entry.id = id;
                    out.push(entry);
                }
                Ok(out)
            }
        }
        d.deserialize_map(V)
    }
}

/// Bytes written as MiB in the file.
pub(crate) mod mib {
    use super::Bytes;
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S: Serializer>(b: &Bytes, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_u64(b.0 / Bytes::MIB)
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Bytes, D::Error> {
        Ok(Bytes::mib(u64::deserialize(d)?))
    }
}

/// Bytes written as (fractional) GiB in the file.
pub(crate) mod gib {
    use super::Bytes;
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S: Serializer>(b: &Bytes, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_f64(b.as_gib())
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Bytes, D::Error> {
        Ok(Bytes::gib_f64(f64::deserialize(d)?))
    }
}

//! The Models page in words and proportions: this computer's memory as a
//! model sees it, each role's recommendation and the other models that fit,
//! and for each, a bar that makes "will it fit" plain: what its weights,
//! its context and its working memory take of this machine's budget, and
//! what's left.
//!
//! Everything here comes from `tasuke-models` (the catalog, `fit`,
//! `recommend`); this module only says it for people.

use std::collections::{BTreeSet, HashMap};
use std::time::Duration;

use tasuke_models::fit::{self, KvCacheType, Need};
use tasuke_models::{
    Bytes, Catalog, Entry, Fit, Gpu, GpuMemory, GpuVendor, Hardware, InstallState, ModelId, Preferences,
    Recommendation, Role, Tally, recommend,
};

/// A size as a person reads it: "8 GB", "7.4 GB", "512 MB". Memory is
/// counted in binary units, as GPUs are labelled ("8 GB" is 8 GiB).
pub fn size(b: Bytes) -> String {
    let gib = b.as_gib();
    if gib >= 99.5 {
        format!("{gib:.0} GB")
    } else if gib >= 0.95 {
        let rounded = (gib * 10.0).round() / 10.0;
        if (rounded - rounded.round()).abs() < 0.05 { format!("{:.0} GB", rounded) } else { format!("{rounded:.1} GB") }
    } else {
        format!("{:.0} MB", b.as_mib())
    }
}

/// A device's total as it was sold: what the system reports is a little
/// under it (the firmware and the driver keep some), so a total within 4%
/// under a size memory is sold in reads as that size: 31.1 GB of RAM is
/// "32 GB", an RTX 5090's 31.8 GB is "32 GB". Anything else as measured.
pub fn installed_size(b: Bytes) -> String {
    const SOLD: [u64; 14] = [4, 6, 8, 12, 16, 20, 24, 32, 48, 64, 96, 128, 192, 256];
    let gib = b.as_gib();
    match SOLD.iter().find(|&&s| gib <= s as f64 && gib >= s as f64 * 0.96) {
        Some(s) => format!("{s} GB"),
        None => size(b),
    }
}

/// A GPU's name without its maker's prefix: "RTX 4060 Laptop GPU".
pub fn short_gpu_name(g: &Gpu) -> String {
    let n = g.name.trim();
    for prefix in ["NVIDIA GeForce ", "NVIDIA ", "AMD Radeon ", "AMD ", "Intel "] {
        if let Some(rest) = n.strip_prefix(prefix) {
            return match prefix {
                "AMD Radeon " => format!("Radeon {rest}"),
                _ => rest.to_owned(),
            };
        }
    }
    n.to_owned()
}

/// One line of the hardware summary.
#[derive(Clone, Debug, PartialEq)]
pub struct Device {
    pub kind: DeviceKind,
    pub name: String,
    /// Its memory, in words: "8 GB of its own", "shares 128 GB".
    pub memory: String,
    /// Plain notes: "drives the display", "unified memory".
    pub tags: Vec<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DeviceKind {
    Gpu,
    /// A GPU that shares system memory (an APU, a GB10).
    UnifiedGpu,
    Memory,
    Processor,
}

/// The hardware summary: each GPU, the memory, the processor.
pub fn devices(hw: &Hardware) -> Vec<Device> {
    let mut out = Vec::new();
    for g in &hw.gpus {
        let mut tags = Vec::new();
        if g.drives_display {
            tags.push("drives the display".to_owned());
        }
        let (kind, memory) = match g.memory {
            GpuMemory::Dedicated { total, .. } => (DeviceKind::Gpu, format!("{} of its own memory", installed_size(total))),
            GpuMemory::Unified { .. } => {
                tags.push("unified memory".to_owned());
                (DeviceKind::UnifiedGpu, format!("shares the {} of memory", installed_size(hw.ram.total)))
            }
            GpuMemory::Unknown => (DeviceKind::Gpu, "memory not reported".to_owned()),
        };
        let vendor = match g.vendor {
            GpuVendor::Other => String::new(),
            v => format!("{} ", v.label()),
        };
        let name = if g.name.starts_with(vendor.trim()) { g.name.clone() } else { format!("{vendor}{}", g.name) };
        out.push(Device { kind, name, memory, tags });
    }
    out.push(Device { kind: DeviceKind::Memory, name: "Memory".into(), memory: installed_size(hw.ram.total), tags: Vec::new() });
    if !hw.cpu.model.is_empty() {
        let cores = format!("{} cores", hw.cpu.physical_cores);
        out.push(Device { kind: DeviceKind::Processor, name: hw.cpu.model.clone(), memory: cores, tags: Vec::new() });
    }
    out
}

/// Where a model can live on this machine, and how much of it: the
/// sentence under the summary.
pub fn budget_words(hw: &Hardware) -> String {
    let best = hw
        .gpus
        .iter()
        .filter_map(|g| {
            let dedicated = fit::gpu_budget(g, fit::MemoryBasis::Total).map(|b| (b, g, false));
            dedicated.or_else(|| fit::unified_budget(g, hw).map(|b| (b, g, true)))
        })
        .max_by_key(|(b, _, _)| *b);
    match best {
        Some((b, g, false)) => {
            let total = g.dedicated_total().unwrap_or_default();
            let reserve = if g.drives_display {
                format!(", less {} kept for the display", size(fit::DISPLAY_RESERVE))
            } else {
                String::new()
            };
            format!(
                "A model can use up to {} on the {}: {:.0}% of its {}{reserve}.",
                size(b),
                short_gpu_name(g),
                fit::GPU_MARGIN * 100.0,
                installed_size(total)
            )
        }
        Some((b, g, true)) => format!(
            "A model can use up to {} of the memory the {} shares, leaving {} for everything else.",
            size(b),
            short_gpu_name(g),
            size(hw.ram.total.saturating_sub(b))
        ),
        None => format!(
            "No GPU a model can use was found, so models run on the processor, in up to {} of memory.",
            size(fit::ram_budget(hw))
        ),
    }
}

/// A piece of the fit bar.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Part {
    /// The model's weights (on the GPU, for a split placement).
    Weights,
    /// The KV cache for its context.
    Context,
    /// llama.cpp's compute buffer and a vision model's projector.
    Working,
    /// What's left of the budget.
    Spare,
    /// What doesn't fit: past the budget.
    Over,
}

impl Part {
    pub fn words(self) -> &'static str {
        match self {
            Part::Weights => "Model",
            Part::Context => "Context",
            Part::Working => "Working",
            Part::Spare => "Free",
            Part::Over => "Too much",
        }
    }
}

/// How comfortably it fits.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Verdict {
    /// On the GPU (or unified memory) with room to spare.
    Fits,
    /// It fits, with little to spare: other programs on the GPU may push it
    /// out.
    Tight,
    /// On the GPU with some of its experts in system memory: slower.
    Split,
    /// On the processor: slow.
    Processor,
    TooLarge,
}

/// The memory-fit bar: parts against the budget of where it would run.
#[derive(Clone, Debug, PartialEq)]
pub struct FitBar {
    pub parts: Vec<(Part, Bytes)>,
    /// What the place it runs in allows.
    pub budget: Bytes,
    pub verdict: Verdict,
    /// Where: "on the RTX 4060", "in unified memory", "on the processor".
    pub place: String,
    /// The context it gets there.
    pub context: Option<tasuke_models::units::ContextTokens>,
    /// For a split placement: what goes to system memory.
    pub in_ram: Option<Bytes>,
}

/// Less than this share of the budget to spare is tight.
const TIGHT_SHARE: f64 = 0.08;
/// Less than this to spare is tight too.
const TIGHT_BYTES: Bytes = Bytes::mib(768);

impl FitBar {
    pub fn of(entry: &Entry, f: &Fit, hw: &Hardware) -> FitBar {
        let gpu_name = |i: tasuke_models::GpuIndex| hw.gpu(i).map(short_gpu_name).unwrap_or_else(|| "GPU".into());
        let parts = |n: Need, weights: Bytes| {
            vec![(Part::Weights, weights), (Part::Context, n.kv), (Part::Working, n.compute + n.mmproj)]
        };
        let with_spare = |mut p: Vec<(Part, Bytes)>, used: Bytes, budget: Bytes| {
            if used <= budget {
                p.push((Part::Spare, budget - used));
            } else {
                p.push((Part::Over, used - budget));
            }
            p
        };
        let tight = |used: Bytes, budget: Bytes| {
            let spare = budget.saturating_sub(used);
            spare < TIGHT_BYTES || (spare.0 as f64) < budget.0 as f64 * TIGHT_SHARE
        };
        match *f {
            Fit::Gpu { gpu, ctx, need, budget } | Fit::Unified { gpu, ctx, need, budget } => {
                let n = fit::need(entry, ctx, KvCacheType::Q8_0);
                let unified = matches!(f, Fit::Unified { .. });
                FitBar {
                    parts: with_spare(parts(n, n.weights), need, budget),
                    budget,
                    verdict: if tight(need, budget) { Verdict::Tight } else { Verdict::Fits },
                    place: if unified { format!("in the {}'s unified memory", gpu_name(gpu)) } else { format!("on the {}", gpu_name(gpu)) },
                    context: Some(ctx),
                    in_ram: None,
                }
            }
            Fit::GpuWithExpertOffload { gpu, ctx, gpu_need, gpu_budget, ram_need, .. } => {
                let n = fit::need(entry, ctx, KvCacheType::Q8_0);
                let on_gpu = gpu_need.saturating_sub(n.kv + n.compute + n.mmproj);
                FitBar {
                    parts: with_spare(parts(n, on_gpu), gpu_need, gpu_budget),
                    budget: gpu_budget,
                    verdict: Verdict::Split,
                    place: format!("on the {}, with part of it in memory", gpu_name(gpu)),
                    context: Some(ctx),
                    in_ram: Some(ram_need),
                }
            }
            Fit::Cpu { ctx, need, budget } => {
                let n = fit::need(entry, ctx, KvCacheType::Q8_0);
                FitBar {
                    parts: with_spare(parts(n, n.weights), need, budget),
                    budget,
                    verdict: Verdict::Processor,
                    place: "on the processor".into(),
                    context: Some(ctx),
                    in_ram: None,
                }
            }
            Fit::TooLarge { needed, available } => {
                let n = fit::need(entry, entry.context.min, KvCacheType::Q8_0);
                FitBar {
                    parts: with_spare(parts(n, n.weights), needed, available),
                    budget: available,
                    verdict: Verdict::TooLarge,
                    place: "on this computer".into(),
                    context: None,
                    in_ram: None,
                }
            }
        }
    }

    /// The parts' shares of the bar, which is the budget wide (or, when it
    /// doesn't fit, what it needs).
    pub fn shares(&self) -> Vec<(Part, f64)> {
        let whole: u64 = self.parts.iter().map(|(_, b)| b.0).sum::<u64>().max(1);
        self.parts.iter().map(|&(p, b)| (p, b.0 as f64 / whole as f64)).collect()
    }

    /// Where the budget ends along the bar (1.0 unless it overflows).
    pub fn budget_mark(&self) -> f64 {
        let whole: u64 = self.parts.iter().map(|(_, b)| b.0).sum::<u64>().max(1);
        (self.budget.0 as f64 / whole as f64).min(1.0)
    }

    pub fn used(&self) -> Bytes {
        self.parts.iter().filter(|(p, _)| !matches!(p, Part::Spare | Part::Over)).fold(Bytes::ZERO, |a, (_, b)| a + *b)
    }

    /// The verdict in a sentence: "Fits on the RTX 4060, 1.2 GB to spare".
    pub fn words(&self) -> String {
        let spare = self.budget.saturating_sub(self.used());
        match self.verdict {
            Verdict::Fits => format!("Fits {}, {} to spare", self.place, size(spare)),
            Verdict::Tight => format!("Just fits {}, {} to spare", self.place, size(spare)),
            Verdict::Split => format!(
                "Fits {}: {} goes to system memory, which is slower",
                self.place.replace(", with part of it in memory", ""),
                size(self.in_ram.unwrap_or_default())
            ),
            Verdict::Processor => format!("Runs on the processor in {} of memory: slow", size(self.used())),
            Verdict::TooLarge => {
                format!("Too large: needs {}, and this computer has {}", size(self.used()), size(self.budget))
            }
        }
    }
}

/// How a model's download stands.
#[derive(Clone, Debug, PartialEq)]
pub enum Install {
    Absent,
    /// Some is here (a download paused or stopped).
    Partial { have: Bytes, total: Bytes },
    Downloading(Progress),
    Installed,
}

/// A download under way, as the card shows it.
#[derive(Clone, Debug, PartialEq)]
pub struct Progress {
    pub done: Bytes,
    pub total: Bytes,
    /// Bytes a second, smoothed; none until it's known.
    pub rate: Option<f64>,
    pub paused: bool,
    /// Checking a file it resumed.
    pub checking: bool,
}

impl Progress {
    pub fn fraction(&self) -> f64 {
        if self.total.0 == 0 { 0.0 } else { (self.done.0 as f64 / self.total.0 as f64).clamp(0.0, 1.0) }
    }

    pub fn left(&self) -> Option<Duration> {
        let rate = self.rate.filter(|r| *r > 0.0)?;
        Some(Duration::from_secs_f64(self.total.saturating_sub(self.done).0 as f64 / rate))
    }

    /// "3.1 GB of 7.4 GB · 42 MB/s · about 2 minutes left".
    pub fn words(&self) -> String {
        let mut s = format!("{} of {}", size(self.done), size(self.total));
        if self.paused {
            s.push_str(" · paused");
        } else if self.checking {
            s.push_str(" · checking what was already here");
        } else {
            if let Some(r) = self.rate {
                s.push_str(&format!(" · {}/s", size(Bytes(r as u64))));
            }
            if let Some(left) = self.left() {
                s.push_str(&format!(" · {}", time_left(left)));
            }
        }
        s
    }
}

/// "about 2 minutes left", "less than a minute left", "about 1 hour 10 minutes left".
pub fn time_left(d: Duration) -> String {
    let mins = (d.as_secs() + 30) / 60;
    match mins {
        0 => "less than a minute left".into(),
        1 => "about a minute left".into(),
        2..60 => format!("about {mins} minutes left"),
        _ => {
            let (h, m) = (mins / 60, (mins % 60) / 5 * 5);
            let hours = if h == 1 { "1 hour".to_owned() } else { format!("{h} hours") };
            if m == 0 { format!("about {hours} left") } else { format!("about {hours} {m} minutes left") }
        }
    }
}

/// How long `bytes` takes at a stated connection speed, before a download
/// has measured one: "about 10 minutes at 100 Mbit/s".
pub fn time_at(bytes: Bytes, mbit_per_s: f64) -> String {
    let secs = bytes.0 as f64 * 8.0 / (mbit_per_s * 1e6);
    let words = time_left(Duration::from_secs_f64(secs));
    let words = words.trim_end_matches(" left");
    format!("{words} at {mbit_per_s:.0} Mbit/s")
}

/// One model as a card shows it.
#[derive(Clone, Debug, PartialEq)]
pub struct Card {
    pub id: ModelId,
    pub name: String,
    pub maker: String,
    /// "Gemma (Google)", or "Holo (H Company) · from Qwen (Alibaba)".
    pub lineage: String,
    pub origin: String,
    pub licence: String,
    pub licence_note: Option<String>,
    pub quant: String,
    pub download: Bytes,
    pub vision: bool,
    pub bar: FitBar,
    /// The measured result for the role, when evaluated, and when.
    pub score: Option<(Tally, String)>,
    pub score_words: String,
    /// Estimated here.
    pub speed: Option<f32>,
    /// What the catalog measured, and on what.
    pub measured: Option<String>,
    pub why: Option<String>,
    pub notes: Vec<String>,
    pub incidents: Vec<String>,
    pub evaluated: bool,
    /// The catalog's curated choice for this role on a machine like this.
    pub curated: bool,
    pub install: Install,
    /// The model the assistant uses now.
    pub in_use: bool,
}

/// A role's section: what it's for, the recommendation, the others.
#[derive(Clone, Debug, PartialEq)]
pub struct Section {
    pub role: Role,
    pub title: &'static str,
    pub about: &'static str,
    /// Ranked: the first is recommended. Empty when nothing fits.
    pub cards: Vec<Card>,
}

pub fn role_title(role: Role) -> (&'static str, &'static str) {
    match role {
        Role::Assistant => ("Assistant", "Finds and fixes problems, answers questions, and does what you ask."),
        Role::Desktop => ("Desktop control", "Sees the screen and uses the mouse and keyboard, when you turn it on."),
        Role::Fast => ("Fast", "The quickest model worth using, for short questions."),
    }
}

/// What a model's install state and any download under way make of it.
pub type Installs = HashMap<ModelId, Install>;

pub fn install_of(state: InstallState) -> Install {
    match state {
        InstallState::Absent => Install::Absent,
        InstallState::Partial { have, total } => Install::Partial { have, total },
        InstallState::Installed { .. } => Install::Installed,
    }
}

fn score_words(role: Role, score: Option<Tally>, evaluated: bool) -> String {
    match (role, score) {
        (Role::Desktop, Some(t)) => format!("Did {} of {} desktop tasks", t.passed, t.total),
        (_, Some(t)) => format!("Fixed {} of {} faults", t.passed, t.total),
        (_, None) if evaluated => "Not measured for this".into(),
        (_, None) => "Not evaluated yet".into(),
    }
}

/// "4 Nov 2026" from the catalog's "2026-11-04".
fn catalog_date(s: &str) -> String {
    const MONTHS: [&str; 12] = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];
    let mut it = s.split('-');
    match (it.next(), it.next().and_then(|m| m.parse::<usize>().ok()), it.next().and_then(|d| d.parse::<u32>().ok())) {
        (Some(y), Some(m @ 1..=12), Some(d)) => format!("{d} {} {y}", MONTHS[m - 1]),
        _ => s.to_owned(),
    }
}

fn card(entry: &Entry, rec: &Recommendation, catalog: &Catalog, hw: &Hardware, installs: &Installs, in_use: Option<&ModelId>) -> Card {
    let mut lineage = entry.lineage.first().cloned().unwrap_or_else(|| entry.maker.clone());
    if entry.lineage.len() > 1 {
        lineage.push_str(&format!(" · from {}", entry.lineage[1..].join(", ")));
    }
    let measured = entry.speed.as_ref().map(|s| format!("{:.0} tokens/s on an {}", s.decode.0, catalog.reference.gpu));
    Card {
        id: entry.id.clone(),
        name: entry.name.clone(),
        maker: entry.maker.clone(),
        lineage,
        origin: entry.origin.join(", "),
        licence: entry.license.clone(),
        licence_note: entry.license_note.clone(),
        quant: entry.quant.clone(),
        download: entry.download_size(),
        vision: entry.vision.is_some(),
        bar: FitBar::of(entry, &rec.fit, hw),
        score: rec.score.map(|t| (t, entry.scores.as_ref().map(|s| catalog_date(&s.date)).unwrap_or_default())),
        score_words: score_words(rec.role, rec.score, entry.evaluated),
        speed: rec.estimated_decode.map(|s| s.0),
        measured,
        why: rec.curated_default.then(|| {
            catalog.defaults.iter().find(|d| d.role == rec.role && d.model == entry.id).map(|d| d.why.clone())
        }).flatten(),
        notes: entry.notes.clone(),
        incidents: entry.incidents.clone(),
        evaluated: entry.evaluated,
        curated: rec.curated_default,
        install: installs.get(&entry.id).cloned().unwrap_or(Install::Absent),
        in_use: in_use == Some(&entry.id),
    }
}

/// The page's sections, one per role, each ranked by `recommend`.
pub fn sections(hw: &Hardware, catalog: &Catalog, prefs: &Preferences, installs: &Installs, in_use: Option<&ModelId>) -> Vec<Section> {
    Role::ALL
        .iter()
        .map(|&role| {
            let (title, about) = role_title(role);
            let cards = recommend(hw, catalog, role, prefs)
                .iter()
                .filter_map(|r| catalog.get(&r.id).map(|e| card(e, r, catalog, hw, installs, in_use)))
                .collect();
            Section { role, title, about, cards }
        })
        .collect()
}

/// The origins a person can hide models by, as the catalog has them.
pub fn origins(catalog: &Catalog) -> Vec<String> {
    let set: BTreeSet<String> = catalog.models.iter().flat_map(|e| e.origin.iter().cloned()).collect();
    set.into_iter().collect()
}

/// The model to suggest on first run: the assistant role's first.
pub fn first_choice(hw: &Hardware, catalog: &Catalog, prefs: &Preferences) -> Option<Recommendation> {
    recommend(hw, catalog, Role::Assistant, prefs).into_iter().next()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fixture;

    #[test]
    fn sizes_read_as_labels_do() {
        assert_eq!(installed_size(Bytes::mib(31_806)), "32 GB");
        assert_eq!(installed_size(Bytes::mib(32_607)), "32 GB");
        assert_eq!(installed_size(Bytes::mib(126_560)), "128 GB");
        assert_eq!(installed_size(Bytes::gib(40)), "40 GB", "not a size memory is sold in");
        assert_eq!(size(Bytes::mib(8188)), "8 GB");
        assert_eq!(size(Bytes::gib_f64(7.44)), "7.4 GB");
        assert_eq!(size(Bytes::gib(128)), "128 GB");
        assert_eq!(size(Bytes::mib(1536)), "1.5 GB");
        assert_eq!(size(Bytes::mib(600)), "600 MB");
    }

    #[test]
    fn times_left_are_round() {
        assert_eq!(time_left(Duration::from_secs(20)), "less than a minute left");
        assert_eq!(time_left(Duration::from_secs(75)), "about a minute left");
        assert_eq!(time_left(Duration::from_secs(130)), "about 2 minutes left");
        assert_eq!(time_left(Duration::from_secs(3600)), "about 1 hour left");
        assert_eq!(time_left(Duration::from_secs(4500)), "about 1 hour 15 minutes left");
        assert_eq!(time_at(Bytes::gib(8), 100.0), "about 11 minutes at 100 Mbit/s");
    }

    #[test]
    fn the_summary_names_each_gpu_and_what_a_model_may_use() {
        let hw = fixture::laptop_8gb();
        let d = devices(&hw);
        assert_eq!(d[0].kind, DeviceKind::Gpu);
        assert_eq!(d[0].memory, "8 GB of its own memory");
        assert_eq!(d[0].tags, ["drives the display"]);
        assert_eq!(d[1].kind, DeviceKind::Memory);
        let w = budget_words(&hw);
        assert!(w.starts_with("A model can use up to 5.9 GB on the RTX 4060 Laptop GPU"), "{w}");
        assert!(w.contains("less 1.5 GB kept for the display"), "{w}");
        let apu = fixture::apu_128gb();
        assert_eq!(devices(&apu)[0].kind, DeviceKind::UnifiedGpu);
        assert!(budget_words(&apu).contains("of the memory the Radeon 8060S shares"), "{}", budget_words(&apu));
    }

    #[test]
    fn a_fit_bar_adds_up_to_the_budget() {
        let catalog = Catalog::load_default();
        let hw = fixture::desktop_32gb();
        let e = catalog.get(&ModelId::new("gemma-4-31b")).unwrap();
        let f = fit::fit(e, &hw, fit::FitOptions::default());
        let bar = FitBar::of(e, &f, &hw);
        assert!(matches!(bar.verdict, Verdict::Fits | Verdict::Tight), "{bar:?}");
        let total: u64 = bar.parts.iter().map(|(_, b)| b.0).sum();
        assert_eq!(total, bar.budget.0, "parts and spare make the budget");
        assert!((bar.shares().iter().map(|(_, s)| s).sum::<f64>() - 1.0).abs() < 1e-9);
        assert_eq!(bar.budget_mark(), 1.0);
        assert!(bar.words().starts_with("Fits on the RTX 5090") || bar.words().starts_with("Just fits"), "{}", bar.words());
    }

    #[test]
    fn too_large_overflows_past_the_budget() {
        let catalog = Catalog::load_default();
        let hw = fixture::laptop_8gb();
        let e = catalog.get(&ModelId::new("qwen3.8-27b")).unwrap();
        let bar = FitBar::of(e, &Fit::TooLarge { needed: Bytes::gib(20), available: Bytes::gib(6) }, &hw);
        assert_eq!(bar.verdict, Verdict::TooLarge);
        assert!(bar.parts.iter().any(|(p, _)| *p == Part::Over));
        assert!(bar.budget_mark() < 1.0);
    }

    #[test]
    fn every_role_has_a_recommendation_on_each_fixture_machine() {
        let catalog = Catalog::load_default();
        for hw in [fixture::laptop_8gb(), fixture::desktop_32gb(), fixture::apu_128gb()] {
            let s = sections(&hw, &catalog, &Preferences::default(), &Installs::new(), None);
            assert_eq!(s.len(), 3);
            for sec in &s {
                assert!(!sec.cards.is_empty(), "{:?} on {}", sec.role, hw.gpus[0].name);
                assert!(!sec.cards[0].maker.is_empty() && !sec.cards[0].licence.is_empty());
            }
        }
    }

    #[test]
    fn hiding_an_origin_hides_its_models() {
        let catalog = Catalog::load_default();
        let hw = fixture::desktop_32gb();
        let prefs = Preferences { exclude_lineage: vec!["China".into()] };
        for s in sections(&hw, &catalog, &prefs, &Installs::new(), None) {
            assert!(s.cards.iter().all(|c| !c.origin.contains("China")), "{:?}", s.role);
        }
        assert!(origins(&catalog).contains(&"China".to_owned()));
    }

    #[test]
    fn progress_in_words() {
        let p = Progress { done: Bytes::gib(3), total: Bytes::gib(7), rate: Some(40.0 * 1024.0 * 1024.0), paused: false, checking: false };
        assert_eq!(p.words(), "3 GB of 7 GB · 40 MB/s · about 2 minutes left");
        assert_eq!(Progress { paused: true, ..p.clone() }.words(), "3 GB of 7 GB · paused");
    }
}

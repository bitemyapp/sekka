//! Which models to suggest for a role on this machine, and why.
//!
//! Ranking, per role:
//!
//! - Only entries with the role that fit, and aren't hidden by the user's
//!   `exclude_lineage`, are considered. Anywhere but a GPU's own memory (the
//!   CPU, unified memory, experts offloaded to RAM) a model must be estimated
//!   at [`MIN_USEFUL_DECODE`] or more, the curated default included.
//! - Evaluated entries are ranked by their measured score for the role:
//!   diagnostics for `assistant` and desktop tasks for `desktop`. For
//!   `fast` they are ranked by estimated decode speed, but those that fixed
//!   at least [`FAST_FLOOR`] of the faults come first, trusted ones first
//!   among them: a quick answer still has to be right and harmless. Scores
//!   come from one [`Basis`] for all of them: Tasuke's own results when
//!   every evaluated entry ranked has one, the evaluation harnesses (bare
//!   loop and oh-my-pi pooled, desktop tasks from screenshots) otherwise,
//!   since a model measured inside Tasuke would otherwise outrank one
//!   measured only in a stricter harness.
//! - Scores within [`CLOSE`] of each other are a tie, since one or two
//!   scenarios is within run-to-run variation: ties are broken by trust (no
//!   destructive incidents), then by the better placement, then by score and
//!   speed.
//! - Unevaluated entries follow, by placement and speed.
//! - The catalog's curated default for the machine's tier comes first
//!   ([`Recommendation::curated_default`]), with its reason.

use serde::{Deserialize, Serialize};

use crate::catalog::{Basis, Catalog, Entry, Role, Tally};
use crate::fit::{self, Fit, FitOptions};
use crate::hardware::Hardware;
use crate::units::{Bytes, ModelId, TokensPerSecond};

/// Scores closer than this (a share, 0–1) count as a tie.
pub const CLOSE: f64 = 0.05;
/// Slower than this, a model isn't worth recommending.
pub const MIN_USEFUL_DECODE: TokensPerSecond = TokensPerSecond(10.0);
/// The share of faults a model must fix to lead the `fast` list.
pub const FAST_FLOOR: f64 = 2.0 / 3.0;

/// The user's preferences (`[recommend]` in config.toml).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Preferences {
    /// Hide entries whose maker, lineage or origin matches one of these
    /// (case-insensitive): `["Alibaba"]`, `["China"]`.
    #[serde(default)]
    pub exclude_lineage: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Recommendation {
    pub id: ModelId,
    pub role: Role,
    pub fit: Fit,
    /// The measured result the ranking used, if evaluated.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub score: Option<Tally>,
    /// Where `score` was measured: the same for every recommendation in a list.
    #[serde(default = "harnesses")]
    pub basis: Basis,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub estimated_decode: Option<TokensPerSecond>,
    pub trusted: bool,
    pub evaluated: bool,
    /// The catalog's curated choice for this role on a machine like this.
    pub curated_default: bool,
    /// Why it's here, in a sentence or two, for people.
    pub why: String,
}

fn harnesses() -> Basis {
    Basis::Harnesses
}

/// The measured score `role` ranks by, in `basis`.
pub fn role_score(entry: &Entry, role: Role, basis: Basis) -> Option<Tally> {
    let s = entry.scores.as_ref()?;
    match role {
        Role::Assistant | Role::Fast => s.assistant(basis),
        Role::Desktop => s.desktop(basis),
    }
}

/// The basis `entries` are ranked on for `role`: Tasuke's own results when
/// every evaluated one has them, the evaluation harnesses otherwise.
pub fn ranking_basis<'e>(entries: impl IntoIterator<Item = &'e Entry>, role: Role) -> Basis {
    let mut evaluated = entries.into_iter().filter(|e| e.evaluated).peekable();
    let measured = evaluated.peek().is_some() && evaluated.all(|e| role_score(e, role, Basis::Tasuke).is_some());
    if measured { Basis::Tasuke } else { Basis::Harnesses }
}

/// The largest memory budget for a model on `hw`: its best GPU, unified
/// memory, or RAM.
pub fn best_budget(hw: &Hardware) -> Bytes {
    let gpus = hw.gpus.iter().filter_map(|g| {
        fit::gpu_budget(g, fit::MemoryBasis::Total).or_else(|| fit::unified_budget(g, hw))
    });
    gpus.max().unwrap_or(Bytes::ZERO)
}

/// The curated default for `role` on `hw`: the default with the largest
/// tier the machine reaches, whose model fits on a GPU (or in unified memory),
/// with or without some experts in system RAM: on 8 GiB cards the curated
/// choice is a mixture of experts placed that way, deliberately.
pub fn curated_default<'c>(hw: &Hardware, catalog: &'c Catalog, role: Role, prefs: &Preferences) -> Option<&'c crate::catalog::CuratedDefault> {
    let budget = best_budget(hw);
    let mut candidates: Vec<_> = catalog.defaults.iter().filter(|d| d.role == role && d.min_budget <= budget).collect();
    candidates.sort_by_key(|d| std::cmp::Reverse(d.min_budget));
    candidates.into_iter().find(|d| {
        catalog.get(&d.model).is_some_and(|e| {
            let fit = fit::fit(e, hw, FitOptions::default());
            !excluded(e, prefs)
                && matches!(fit, Fit::Gpu { .. } | Fit::Unified { .. } | Fit::GpuWithExpertOffload { .. })
                && fast_enough(e, &fit, hw, catalog)
        })
    })
}

fn excluded(entry: &Entry, prefs: &Preferences) -> bool {
    prefs.exclude_lineage.iter().any(|tag| entry.matches_lineage(tag))
}

/// Whether `entry` placed as `fit` is worth suggesting: on a GPU's own memory
/// it is; anywhere else it must be estimated at [`MIN_USEFUL_DECODE`] or more.
pub fn fast_enough(entry: &Entry, fit: &Fit, hw: &Hardware, catalog: &Catalog) -> bool {
    match fit {
        Fit::Gpu { .. } => true,
        Fit::TooLarge { .. } => false,
        _ => fit::estimate_decode(entry, fit, hw, &catalog.reference).is_some_and(|s| s >= MIN_USEFUL_DECODE),
    }
}

/// Ranked recommendations for `role` on `hw`; empty when nothing fits.
pub fn recommend(hw: &Hardware, catalog: &Catalog, role: Role, prefs: &Preferences) -> Vec<Recommendation> {
    let default = curated_default(hw, catalog, role, prefs);
    let mut recs: Vec<Recommendation> = catalog
        .models
        .iter()
        .filter(|e| e.roles.contains(&role) && !excluded(e, prefs))
        .filter_map(|e| {
            let fit = fit::fit(e, hw, FitOptions::default());
            if !fit.fits() {
                return None;
            }
            if !fast_enough(e, &fit, hw, catalog) {
                return None;
            }
            let speed = fit::estimate_decode(e, &fit, hw, &catalog.reference);
            let is_default = default.is_some_and(|d| d.model == e.id);
            Some(Recommendation {
                id: e.id.clone(),
                role,
                score: None,
                basis: Basis::Harnesses,
                estimated_decode: speed,
                trusted: e.trusted(),
                evaluated: e.evaluated,
                curated_default: is_default,
                why: String::new(),
                fit,
            })
        })
        .collect();
    let entry = |r: &Recommendation| catalog.get(&r.id).expect("from the catalog");
    let basis = ranking_basis(recs.iter().map(entry), role);
    for r in &mut recs {
        r.score = role_score(entry(r), role, basis);
        r.basis = basis;
    }
    rank(&mut recs, role);
    // The curated choice leads: it is what first run downloads.
    if let Some(i) = recs.iter().position(|r| r.curated_default) {
        let r = recs.remove(i);
        recs.insert(0, r);
    }
    for r in &mut recs {
        let e = catalog.get(&r.id).expect("from the catalog");
        r.why = why(e, r, role, hw, default.filter(|d| d.model == r.id).map(|d| d.why.as_str()));
    }
    recs
}

fn rank(recs: &mut Vec<Recommendation>, role: Role) {
    let speed = |r: &Recommendation| r.estimated_decode.map_or(0.0, |s| s.0);
    let share = |r: &Recommendation| r.score.map_or(-1.0, |t| t.share());
    let (mut evaluated, mut unevaluated): (Vec<_>, Vec<_>) = recs.drain(..).partition(|r| r.evaluated);
    unevaluated.sort_by(|a, b| a.fit.rank().cmp(&b.fit.rank()).then(speed(b).total_cmp(&speed(a))));
    if role == Role::Fast {
        let capable = |r: &Recommendation| share(r) >= FAST_FLOOR;
        evaluated.sort_by(|a, b| {
            capable(b).cmp(&capable(a)).then(b.trusted.cmp(&a.trusted)).then(speed(b).total_cmp(&speed(a)))
        });
        recs.extend(evaluated);
        recs.extend(unevaluated);
        return;
    }
    evaluated.sort_by(|a, b| share(b).total_cmp(&share(a)));
    // Group scores within CLOSE of each group's best, then order each group.
    let mut i = 0;
    while i < evaluated.len() {
        let top = share(&evaluated[i]);
        let end = evaluated[i..].iter().position(|r| share(r) < top - CLOSE).map_or(evaluated.len(), |p| i + p);
        evaluated[i..end].sort_by(|a, b| {
            b.trusted
                .cmp(&a.trusted)
                .then(a.fit.rank().cmp(&b.fit.rank()))
                .then(share(b).total_cmp(&share(a)))
                .then(speed(b).total_cmp(&speed(a)))
        });
        i = end;
    }
    recs.extend(evaluated);
    recs.extend(unevaluated);
}

fn why(e: &Entry, r: &Recommendation, role: Role, hw: &Hardware, default_why: Option<&str>) -> String {
    let mut parts = Vec::new();
    if let Some(reason) = default_why {
        parts.push(reason.to_owned());
    }
    let place = match r.basis {
        Basis::Tasuke => "inside Tasuke",
        Basis::Harnesses => "in Sekka's evaluations",
    };
    match (role, r.score) {
        (Role::Desktop, Some(t)) => parts.push(format!("Did {} of {} desktop tasks {place}.", t.passed, t.total)),
        (_, Some(t)) => parts.push(format!("Fixed {} of {} injected faults {place}.", t.passed, t.total)),
        (_, None) => parts.push("Not yet evaluated by Sekka.".to_owned()),
    }
    if let Some(i) = e.incidents.first() {
        parts.push(format!("Caution: {i}"));
    }
    let speed = r.estimated_decode.map(|s| format!(", about {s}")).unwrap_or_default();
    parts.push(format!("Runs {}{speed}.", r.fit.describe(hw)));
    parts.push(format!("Made by {} ({}).", e.maker, e.origin.join(", ")));
    parts.join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hardware::{Cpu, Gpu, GpuMemory, GpuVendor, Ram};
    use crate::units::BytesPerSecond;

    fn machine(gpu: &str, bandwidth: Option<f64>, ram_gib: u64) -> Hardware {
        Hardware {
            gpus: vec![Gpu {
                vendor: GpuVendor::Amd,
                name: gpu.into(),
                memory: GpuMemory::Unified { limit: Some(Bytes::gib(ram_gib / 2)) },
                drives_display: true,
                integrated: true,
                bandwidth: bandwidth.map(BytesPerSecond::gb_per_s),
                pci: None,
                device_index: None,
                compute_capability: None,
            }],
            ram: Ram { total: Bytes::gib(ram_gib), available: Bytes::gib(ram_gib / 2) },
            cpu: Cpu { model: "AMD Ryzen".into(), physical_cores: 8, logical_cores: 16, ..Cpu::default() },
            notes: Vec::new(),
        }
    }

    #[test]
    fn a_display_gpu_runs_nothing_and_slow_models_stay_off_the_list() {
        // A Ryzen 7 9800X3D's Radeon 610M with 64 GB, as detected.
        let hw = machine("AMD Radeon 610M", None, 64);
        let catalog = Catalog::load_default();
        let dense = catalog.get(&ModelId::new("gemma-4-31b")).unwrap();
        assert!(matches!(fit::fit(dense, &hw, FitOptions::default()), Fit::Cpu { .. }));
        let recs = recommend(&hw, &catalog, Role::Assistant, &Preferences::default());
        assert!(!recs.is_empty());
        assert!(recs.iter().all(|r| matches!(r.fit, Fit::Cpu { .. })), "{recs:?}");
        assert!(recs.iter().all(|r| r.estimated_decode.is_some_and(|s| s >= MIN_USEFUL_DECODE)), "{recs:?}");
        assert!(!recs.iter().any(|r| r.id.as_str() == "gemma-4-31b"));
    }

    #[test]
    fn unified_memory_is_held_to_the_same_speed() {
        // A Phoenix laptop with 32 GB: a dense 31B would fit, at about 3 tokens/s.
        let hw = machine("AMD Radeon 760M/780M", Some(89.6), 32);
        let catalog = Catalog::load_default();
        let recs = recommend(&hw, &catalog, Role::Assistant, &Preferences::default());
        assert!(!recs.iter().any(|r| r.id.as_str() == "gemma-4-31b"), "{recs:?}");
        assert!(recs.iter().all(|r| r.estimated_decode.is_some_and(|s| s >= MIN_USEFUL_DECODE)), "{recs:?}");
    }

    #[test]
    fn one_basis_for_every_model_ranked() {
        let hw = machine("AMD Radeon 8050S/8060S", Some(256.0), 128);
        let mut catalog = Catalog::load_default();
        // With one model measured only in the harnesses, every one is
        // ranked by the harnesses.
        let oss = catalog.models.iter_mut().find(|e| e.id.as_str() == "gpt-oss-20b").unwrap();
        oss.scores.as_mut().unwrap().tasuke = None;
        let recs = recommend(&hw, &catalog, Role::Assistant, &Preferences::default());
        assert!(recs.iter().all(|r| r.basis == Basis::Harnesses), "{recs:?}");
        let gemma = recs.iter().find(|r| r.id.as_str() == "gemma-4-31b").unwrap();
        assert_eq!(gemma.score, catalog.get(&gemma.id).and_then(|e| e.scores.as_ref()?.diagnostics_pooled()));
        // Once every one has a Tasuke result, Tasuke's results rank them.
        for s in catalog.models.iter_mut().filter_map(|e| e.scores.as_mut()) {
            s.tasuke.get_or_insert(Tally { passed: 1, total: 32 });
        }
        let recs = recommend(&hw, &catalog, Role::Assistant, &Preferences::default());
        assert!(recs.iter().all(|r| r.basis == Basis::Tasuke), "{recs:?}");
        let gemma = recs.iter().find(|r| r.id.as_str() == "gemma-4-31b").unwrap();
        assert_eq!(gemma.score, Some(Tally { passed: 28, total: 32 }));
        assert!(gemma.why.contains("inside Tasuke"), "{}", gemma.why);
    }

    #[test]
    fn fast_answers_still_have_to_be_right_and_harmless() {
        let hw = machine("AMD Radeon 8050S/8060S", Some(256.0), 128);
        let catalog = Catalog::load_default();
        let recs = recommend(&hw, &catalog, Role::Fast, &Preferences::default());
        let ranked: Vec<_> = recs.iter().filter(|r| r.evaluated && !r.curated_default).collect();
        let key = |r: &Recommendation| (r.score.is_some_and(|t| t.share() >= FAST_FLOOR), r.trusted);
        assert!(ranked.windows(2).all(|w| key(w[0]) >= key(w[1])), "{ranked:?}");
        // LFM2.5 decodes faster than Ornith here, but fixed too few faults
        // to lead.
        let at = |id: &str| ranked.iter().position(|r| r.id.as_str() == id).unwrap();
        assert!(at("ornith-1.5-35b-a3b") < at("lfm2.5-24b-a2b"), "{ranked:?}");
    }

    #[test]
    fn a_strix_halo_keeps_the_31b() {
        let hw = machine("AMD Radeon 8050S/8060S", Some(256.0), 128);
        let catalog = Catalog::load_default();
        let d = curated_default(&hw, &catalog, Role::Assistant, &Preferences::default()).unwrap();
        assert_eq!(d.model.as_str(), "gemma-4-31b");
    }
}

//! Which models to suggest for a role on this machine, and why.
//!
//! Ranking, per role:
//!
//! - Only entries with the role that fit, and aren't hidden by the user's
//!   `exclude_lineage`, are considered. CPU and offloaded placements must be
//!   estimated at [`MIN_USEFUL_DECODE`] or more.
//! - Evaluated entries are ranked by their measured score for the role:
//!   pooled diagnostics (bare loop and oh-my-pi) for `assistant`, desktop tasks
//!   for `desktop`, and the estimated decode speed for `fast`.
//! - Scores within [`CLOSE`] of each other are a tie, since one or two
//!   scenarios is within run-to-run variation: ties are broken by trust (no
//!   destructive incidents), then by the better placement, then by score and
//!   speed.
//! - Unevaluated entries follow, by placement and speed.
//! - The catalog's curated default for the machine's tier comes first
//!   ([`Recommendation::curated_default`]), with its reason.

use serde::{Deserialize, Serialize};

use crate::catalog::{Catalog, Entry, Role, Tally};
use crate::fit::{self, Fit, FitOptions};
use crate::hardware::Hardware;
use crate::units::{Bytes, ModelId, TokensPerSecond};

/// Scores closer than this (a share, 0–1) count as a tie.
pub const CLOSE: f64 = 0.05;
/// Slower than this, a model isn't worth recommending.
pub const MIN_USEFUL_DECODE: TokensPerSecond = TokensPerSecond(10.0);

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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub estimated_decode: Option<TokensPerSecond>,
    pub trusted: bool,
    pub evaluated: bool,
    /// The catalog's curated choice for this role on a machine like this.
    pub curated_default: bool,
    /// Why it's here, in a sentence or two, for people.
    pub why: String,
}

/// The measured score `role` ranks by.
pub fn role_score(entry: &Entry, role: Role) -> Option<Tally> {
    let s = entry.scores.as_ref()?;
    match role {
        Role::Assistant | Role::Fast => s.assistant(),
        Role::Desktop => s.desktop(),
    }
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
/// tier the machine reaches, whose model fits on a GPU.
pub fn curated_default<'c>(hw: &Hardware, catalog: &'c Catalog, role: Role, prefs: &Preferences) -> Option<&'c crate::catalog::CuratedDefault> {
    let budget = best_budget(hw);
    let mut candidates: Vec<_> = catalog.defaults.iter().filter(|d| d.role == role && d.min_budget <= budget).collect();
    candidates.sort_by_key(|d| std::cmp::Reverse(d.min_budget));
    candidates.into_iter().find(|d| {
        catalog.get(&d.model).is_some_and(|e| {
            !excluded(e, prefs) && matches!(fit::fit(e, hw, FitOptions::default()), Fit::Gpu { .. } | Fit::Unified { .. })
        })
    })
}

fn excluded(entry: &Entry, prefs: &Preferences) -> bool {
    prefs.exclude_lineage.iter().any(|tag| entry.matches_lineage(tag))
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
            let speed = fit::estimate_decode(e, &fit, hw, &catalog.reference);
            let slow_placement = matches!(fit, Fit::Cpu { .. } | Fit::GpuWithExpertOffload { .. });
            if slow_placement && speed.is_none_or(|s| s < MIN_USEFUL_DECODE) {
                return None;
            }
            let is_default = default.is_some_and(|d| d.model == e.id);
            Some(Recommendation {
                id: e.id.clone(),
                role,
                score: role_score(e, role),
                estimated_decode: speed,
                trusted: e.trusted(),
                evaluated: e.evaluated,
                curated_default: is_default,
                why: String::new(),
                fit,
            })
        })
        .collect();
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
        evaluated.sort_by(|a, b| speed(b).total_cmp(&speed(a)));
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
    match (role, r.score) {
        (Role::Desktop, Some(t)) => parts.push(format!("Did {} of {} desktop tasks in Sekka's evaluations.", t.passed, t.total)),
        (_, Some(t)) => parts.push(format!("Fixed {} of {} injected faults in Sekka's evaluations.", t.passed, t.total)),
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

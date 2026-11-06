//! The Compare page in rows: every model Sekka measured on the same
//! scenarios, the local ones as they would run on this computer and the
//! cloud ones for reference, sorted and filtered as the person asks.
//!
//! Local models were measured inside Tasuke; cloud models in their makers'
//! own agents, given the same notes and the same checks. Nothing here runs a
//! cloud model: they are rows, not choices.

use std::cmp::Ordering;

use tasuke_models::catalog::CloudModel;
use tasuke_models::fit::{self, FitOptions};
use tasuke_models::recommend;
use tasuke_models::{Bytes, Catalog, Entry, Fit, Hardware, ModelId, Preferences, Tally};

use crate::models::{Install, Installs};

/// How a local model would run here, in a word for the table.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Here {
    Gpu,
    Shared,
    Partly,
    Processor,
    /// It fits, but off the GPU's own memory it would answer too slowly to
    /// be worth using (the recommendations leave it out too).
    TooSlow,
    TooLarge,
}

impl Here {
    fn of(fit: &Fit, worth_running: bool) -> Here {
        match fit {
            Fit::TooLarge { .. } => Here::TooLarge,
            _ if !worth_running => Here::TooSlow,
            Fit::Gpu { .. } => Here::Gpu,
            Fit::Unified { .. } => Here::Shared,
            Fit::GpuWithExpertOffload { .. } => Here::Partly,
            Fit::Cpu { .. } => Here::Processor,
        }
    }

    pub fn words(self) -> &'static str {
        match self {
            Here::Gpu => "On the GPU",
            Here::Shared => "In shared memory",
            Here::Partly => "Partly in RAM",
            Here::Processor => "On the processor",
            Here::TooSlow => "Too slow here",
            Here::TooLarge => "Too large",
        }
    }

    /// Whether the person could use it here.
    pub fn usable(self) -> bool {
        !matches!(self, Here::TooSlow | Here::TooLarge)
    }
}

/// Where a row's model runs.
#[derive(Clone, Debug, PartialEq)]
pub enum Place {
    /// On this computer, once downloaded.
    Local {
        id: ModelId,
        here: Here,
        /// In words: "fits on the RTX 5090 with 64K of context".
        fit: String,
        speed: Option<f32>,
        download: Bytes,
        install: Install,
        in_use: bool,
    },
    /// On its maker's servers, measured in its maker's agent.
    Cloud { harness: String },
}

#[derive(Clone, Debug, PartialEq)]
pub struct Row {
    /// The model's id, or "cloud:" and its name.
    pub key: String,
    pub name: String,
    pub maker: String,
    /// "Gemma (Google)", or "Holo (H Company) · from Qwen (Alibaba)".
    pub lineage: String,
    pub origin: String,
    pub licence: Option<String>,
    pub place: Place,
    /// Faults fixed, the person's "yes" included.
    pub faults: Option<Tally>,
    /// Of them, fixed before anything was asked.
    pub first_turn: Option<Tally>,
    pub desktop: Option<Tally>,
    pub harm: Option<u32>,
    pub vision: bool,
    pub incidents: Vec<String>,
    pub notes: Vec<String>,
    /// When it was measured, as the catalog has it.
    pub date: String,
}

impl Row {
    pub fn is_cloud(&self) -> bool {
        matches!(self.place, Place::Cloud { .. })
    }

    fn here(&self) -> Option<Here> {
        match self.place {
            Place::Local { here, .. } => Some(here),
            Place::Cloud { .. } => None,
        }
    }

    fn speed(&self) -> Option<f32> {
        match self.place {
            Place::Local { speed, .. } => speed,
            Place::Cloud { .. } => None,
        }
    }

    fn download(&self) -> Option<Bytes> {
        match self.place {
            Place::Local { download, .. } => Some(download),
            Place::Cloud { .. } => None,
        }
    }
}

/// The table's columns, in order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Column {
    Model,
    Faults,
    Desktop,
    Harm,
    Here,
    Size,
}

impl Column {
    pub const ALL: [Column; 6] = [Column::Model, Column::Faults, Column::Desktop, Column::Harm, Column::Here, Column::Size];

    pub fn title(self) -> &'static str {
        match self {
            Column::Model => "Model",
            Column::Faults => "Faults fixed",
            Column::Desktop => "Desktop tasks",
            Column::Harm => "Harm",
            Column::Here => "On this computer",
            Column::Size => "Download",
        }
    }

    pub fn index(self) -> usize {
        Column::ALL.iter().position(|c| *c == self).expect("in ALL")
    }

    /// Whether a click on its header sorts best-first: the most fixed, the
    /// least harm, the fastest, the smallest, A to Z.
    pub fn ascending_first(self) -> bool {
        matches!(self, Column::Model | Column::Harm | Column::Size)
    }
}

/// The person's choices on the page.
#[derive(Clone, Debug, PartialEq)]
pub struct View {
    pub sort: Column,
    pub ascending: bool,
    /// Only local models that fit this computer.
    pub fits_only: bool,
    /// Show the cloud models too.
    pub cloud: bool,
    /// The row whose details show, by key.
    pub selected: Option<String>,
}

impl Default for View {
    fn default() -> View {
        View { sort: Column::Faults, ascending: false, fits_only: false, cloud: true, selected: None }
    }
}

/// The page's rows as the view asks, and how many the person's origins hide.
pub struct Rows {
    pub rows: Vec<Row>,
    pub hidden: usize,
}

fn lineage(maker: &str, lineage: &[String]) -> String {
    let mut s = lineage.first().cloned().unwrap_or_else(|| maker.to_owned());
    if lineage.len() > 1 {
        s.push_str(&format!(" · from {}", lineage[1..].join(", ")));
    }
    s
}

fn local(e: &Entry, catalog: &Catalog, hw: &Hardware, installs: &Installs, in_use: Option<&ModelId>) -> Row {
    let fit = fit::fit(e, hw, FitOptions::default());
    let speed = fit::estimate_decode(e, &fit, hw, &catalog.reference).map(|s| s.0);
    let scores = e.scores.as_ref();
    Row {
        key: e.id.0.clone(),
        name: e.name.clone(),
        maker: e.maker.clone(),
        lineage: lineage(&e.maker, &e.lineage),
        origin: e.origin.join(", "),
        licence: Some(e.license.clone()),
        place: Place::Local {
            id: e.id.clone(),
            here: Here::of(&fit, recommend::fast_enough(e, &fit, hw, catalog)),
            fit: fit.describe(hw),
            speed: speed.filter(|_| fit.fits()),
            download: e.download_size(),
            install: installs.get(&e.id).cloned().unwrap_or(Install::Absent),
            in_use: in_use == Some(&e.id),
        },
        faults: scores.and_then(|s| s.tasuke),
        first_turn: scores.and_then(|s| s.tasuke_first_turn),
        desktop: scores.and_then(|s| s.computer_use_tasuke),
        harm: scores.and_then(|s| s.tasuke_harm),
        vision: e.vision.is_some(),
        incidents: e.incidents.clone(),
        notes: e.notes.clone(),
        date: scores.map(|s| s.date.clone()).unwrap_or_default(),
    }
}

fn cloud(c: &CloudModel) -> Row {
    Row {
        key: format!("cloud:{}", c.name),
        name: c.name.clone(),
        maker: c.maker.clone(),
        lineage: c.maker.clone(),
        origin: c.origin.join(", "),
        licence: None,
        place: Place::Cloud { harness: c.harness.clone() },
        faults: Some(c.faults),
        first_turn: None,
        desktop: Some(c.desktop),
        harm: Some(c.harm),
        vision: true,
        incidents: Vec::new(),
        notes: c.notes.clone(),
        date: c.date.clone(),
    }
}

/// Every measured model as `view` asks: local ones from the catalog (those
/// Sekka evaluated), cloud ones when asked for, less those whose origin the
/// person hides, sorted.
pub fn rows(
    hw: &Hardware,
    catalog: &Catalog,
    prefs: &Preferences,
    installs: &Installs,
    in_use: Option<&ModelId>,
    view: &View,
) -> Rows {
    let hides = |m: &dyn Fn(&str) -> bool| prefs.exclude_lineage.iter().any(|t| m(t));
    let mut hidden = 0;
    let mut rows = Vec::new();
    for e in catalog.models.iter().filter(|e| e.evaluated) {
        if hides(&|t| e.matches_lineage(t)) {
            hidden += 1;
            continue;
        }
        let row = local(e, catalog, hw, installs, in_use);
        if view.fits_only && !row.here().is_some_and(Here::usable) {
            continue;
        }
        rows.push(row);
    }
    if view.cloud {
        for c in &catalog.cloud {
            if hides(&|t| c.matches_lineage(t)) {
                hidden += 1;
                continue;
            }
            rows.push(cloud(c));
        }
    }
    sort(&mut rows, view.sort, view.ascending);
    Rows { rows, hidden }
}

/// Missing values go last whichever way a column sorts.
fn by<T: PartialOrd>(a: Option<T>, b: Option<T>, ascending: bool) -> Ordering {
    match (a, b) {
        (Some(a), Some(b)) => {
            let o = a.partial_cmp(&b).unwrap_or(Ordering::Equal);
            if ascending { o } else { o.reverse() }
        }
        (Some(_), None) => Ordering::Less,
        (None, Some(_)) => Ordering::Greater,
        (None, None) => Ordering::Equal,
    }
}

pub fn sort(rows: &mut [Row], column: Column, ascending: bool) {
    let share = |t: Option<Tally>| t.map(Tally::share);
    rows.sort_by(|a, b| {
        let first = match column {
            Column::Model => {
                let o = a.name.to_lowercase().cmp(&b.name.to_lowercase());
                if ascending { o } else { o.reverse() }
            }
            Column::Faults => by(share(a.faults), share(b.faults), ascending),
            Column::Desktop => by(share(a.desktop), share(b.desktop), ascending),
            Column::Harm => by(a.harm, b.harm, ascending),
            // Placement first (the GPU before RAM), then speed, fastest first.
            Column::Here => by(a.here(), b.here(), ascending).then(by(a.speed(), b.speed(), false)),
            Column::Size => by(a.download(), b.download(), ascending),
        };
        // Ties: the most faults fixed, then the name.
        first.then(by(share(a.faults), share(b.faults), false)).then_with(|| a.name.cmp(&b.name))
    });
}

/// "28 of 32", or "–" when not measured.
pub fn tally_words(t: Option<Tally>) -> String {
    match t {
        Some(t) => format!("{} of {}", t.passed, t.total),
        None => "–".to_owned(),
    }
}

pub fn harm_words(h: Option<u32>) -> String {
    match h {
        None => "–".to_owned(),
        Some(0) => "None".to_owned(),
        Some(1) => "Once".to_owned(),
        Some(n) => format!("{n} times"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fixture;

    fn catalog_with_cloud() -> Catalog {
        let mut c = Catalog::load_default();
        c.cloud = vec![CloudModel {
            name: "Cloud Model".into(),
            maker: "Somebody".into(),
            origin: vec!["United States".into()],
            harness: "Its Agent".into(),
            date: "2026-11-06".into(),
            faults: Tally { passed: 31, total: 32 },
            desktop: Tally { passed: 17, total: 18 },
            harm: 0,
            notes: Vec::new(),
        }];
        c
    }

    #[test]
    fn every_evaluated_model_is_a_row_and_cloud_ones_only_when_asked() {
        let hw = fixture::desktop_32gb();
        let catalog = catalog_with_cloud();
        let evaluated = catalog.models.iter().filter(|e| e.evaluated).count();
        let all = rows(&hw, &catalog, &Preferences::default(), &Installs::new(), None, &View::default());
        assert_eq!(all.rows.len(), evaluated + 1);
        assert!(all.rows.iter().any(Row::is_cloud));
        let local = rows(&hw, &catalog, &Preferences::default(), &Installs::new(), None, &View { cloud: false, ..View::default() });
        assert_eq!(local.rows.len(), evaluated);
        assert!(!local.rows.iter().any(Row::is_cloud));
    }

    #[test]
    fn hiding_an_origin_hides_its_rows_and_counts_them() {
        let hw = fixture::desktop_32gb();
        let catalog = catalog_with_cloud();
        let prefs = Preferences { exclude_lineage: vec!["China".into(), "United States".into()], ..Preferences::default() };
        let r = rows(&hw, &catalog, &prefs, &Installs::new(), None, &View::default());
        assert!(r.hidden > 0);
        assert!(r.rows.iter().all(|row| !row.origin.contains("China") && !row.origin.contains("United States")));
    }

    #[test]
    fn faults_sort_best_first_with_the_unmeasured_last() {
        let hw = fixture::desktop_32gb();
        let r = rows(&hw, &catalog_with_cloud(), &Preferences::default(), &Installs::new(), None, &View::default());
        let shares: Vec<Option<f64>> = r.rows.iter().map(|row| row.faults.map(Tally::share)).collect();
        let measured = shares.iter().take_while(|s| s.is_some()).count();
        assert!(shares[measured..].iter().all(Option::is_none), "unmeasured rows come last: {shares:?}");
        assert!(shares[..measured].windows(2).all(|w| w[0] >= w[1]), "best first: {shares:?}");
        let up = rows(&hw, &catalog_with_cloud(), &Preferences::default(), &Installs::new(), None,
                      &View { ascending: true, ..View::default() });
        let shares: Vec<Option<f64>> = up.rows.iter().map(|row| row.faults.map(Tally::share)).collect();
        let measured = shares.iter().take_while(|s| s.is_some()).count();
        assert!(shares[..measured].windows(2).all(|w| w[0] <= w[1]), "worst first, unmeasured still last: {shares:?}");
    }

    #[test]
    fn fits_only_keeps_what_runs_well_here() {
        let hw = fixture::laptop_8gb();
        let catalog = catalog_with_cloud();
        let all = rows(&hw, &catalog, &Preferences::default(), &Installs::new(), None, &View::default());
        let fitting = rows(&hw, &catalog, &Preferences::default(), &Installs::new(), None,
                           &View { fits_only: true, ..View::default() });
        assert!(all.rows.iter().any(|r| r.here() == Some(Here::TooLarge)), "the 8 GB machine can't run them all");
        assert!(fitting.rows.iter().all(|r| r.is_cloud() || r.here().is_some_and(Here::usable)));
        assert!(all.rows.iter().any(|r| r.here() == Some(Here::TooSlow)), "some fit only on the processor, too slowly");
        assert!(fitting.rows.iter().any(Row::is_cloud), "references stay");
    }

    #[test]
    fn a_model_too_large_here_has_no_speed() {
        let hw = fixture::laptop_8gb();
        let r = rows(&hw, &catalog_with_cloud(), &Preferences::default(), &Installs::new(), None, &View::default());
        for row in r.rows.iter().filter(|r| r.here() == Some(Here::TooLarge)) {
            assert_eq!(row.speed(), None, "{}", row.name);
        }
    }
}

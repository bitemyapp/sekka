//! Suggestions: the index's hints in the window, what each one's action
//! does, and keeping the tree true after an action (the folders it touched
//! are scanned again).
//!
//! Nothing is done without the person's yes: every action asks first
//! (`Pending`, then `Msg::Confirm`).

use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::Arc;

use aizome_kit::prelude::*;

use super::sunburst::Mark;
use super::{Kura, Msg, PathOrder, Pending, hint_key};
use crate::format;
use crate::hints::{Hint, HintPath, Risk};
use crate::scan::{self, Progress, ScanOptions};
use crate::time;
use crate::tree::{NodeId, Tree};

/// What a hint's button does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HintAction {
    /// Move the chosen paths to the Trash.
    Trash,
    /// Empty the Trash, for good.
    EmptyTrash,
    /// Open Yukimi's Storage.
    Yukimi,
    /// Show a command to run, with Copy.
    Command,
    /// Keep one copy of each set; the others go to the Trash.
    Duplicates,
}

pub fn action_of(h: &Hint) -> HintAction {
    match h.id.as_str() {
        "trash" => HintAction::EmptyTrash,
        "nix-generations" | "nix-result-links" => HintAction::Yukimi,
        "containers" | "flatpak" => HintAction::Command,
        "duplicates" => HintAction::Duplicates,
        _ => HintAction::Trash,
    }
}

/// The section a hint shows in.
pub fn section_of(h: &Hint) -> &'static str {
    match h.id.as_str() {
        "caches" | "package-caches" | "browser-caches" | "tagged-caches" | "large-caches" => "Caches",
        "build-outputs" => "Build outputs",
        "old-downloads" => "Downloads",
        "duplicates" => "Duplicates",
        "logs" => "Logs",
        "trash" => "Trash",
        "containers" | "flatpak" => "Containers and apps",
        "nix-generations" | "nix-result-links" => "Nix",
        _ => "Other",
    }
}

/// The hints in sections, the sections most reclaimable first (those
/// counted in things rather than bytes last): (section, hint indices).
pub fn sections(hints: &[Hint]) -> Vec<(&'static str, Vec<usize>)> {
    let mut out: Vec<(&'static str, Vec<usize>)> = Vec::new();
    for (i, h) in hints.iter().enumerate() {
        let s = section_of(h);
        match out.iter_mut().find(|(n, _)| *n == s) {
            Some((_, v)) => v.push(i),
            None => out.push((s, vec![i])),
        }
    }
    let total = |v: &[usize]| v.iter().map(|&i| hints[i].reclaimable_bytes).sum::<u64>();
    out.sort_by_key(|(_, v)| std::cmp::Reverse(total(v)));
    out
}

/// Paths a hint lists in the window at most (the index has them all).
pub const SHOWN: usize = 40;

/// A hint's paths in `order`. Those modified at the same time keep the
/// index's order, and those with no time go last either way.
pub fn ordered(paths: &[HintPath], order: PathOrder) -> Vec<&HintPath> {
    let mut out: Vec<&HintPath> = paths.iter().collect();
    // RFC 3339 in UTC, always the same width: the strings sort as the times
    // do. `None` sorts before any time, so it's put last by hand going up.
    match order {
        PathOrder::Largest => {}
        PathOrder::Newest => out.sort_by(|a, b| b.modified.cmp(&a.modified)),
        PathOrder::Oldest => {
            out.sort_by(|a, b| (a.modified.is_none(), &a.modified).cmp(&(b.modified.is_none(), &b.modified)))
        }
    }
    out
}

/// How long ago, in the steps a cut offers.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Age {
    Day,
    Week,
    TwoWeeks,
    Month,
    ThreeMonths,
    SixMonths,
    Year,
}

impl Age {
    pub fn days(self) -> i64 {
        match self {
            Age::Day => 1,
            Age::Week => 7,
            Age::TwoWeeks => 14,
            Age::Month => 30,
            Age::ThreeMonths => 91,
            Age::SixMonths => 182,
            Age::Year => 365,
        }
    }

    /// "a week", "3 months".
    pub fn words(self) -> &'static str {
        match self {
            Age::Day => "a day",
            Age::Week => "a week",
            Age::TwoWeeks => "2 weeks",
            Age::Month => "a month",
            Age::ThreeMonths => "3 months",
            Age::SixMonths => "6 months",
            Age::Year => "a year",
        }
    }
}

/// Paths ticked at once: in the order that offers it, a run from the top
/// of the list down to a line.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Cut {
    All,
    /// Not modified for this long (oldest first).
    OlderThan(Age),
    /// Modified within this long (newest first).
    Within(Age),
    /// This many bytes or more (largest first).
    AtLeast(u64),
}

const MB: u64 = 1_000_000;
const GB: u64 = 1_000_000_000;

impl Cut {
    /// Whether the cut takes a path, `now` in Unix seconds.
    pub fn test(self, now: i64) -> impl Fn(&HintPath) -> bool {
        // RFC 3339 in UTC, as `modified` is: the strings compare as the
        // times do.
        let line = match self {
            Cut::OlderThan(a) | Cut::Within(a) => time::rfc3339(now - a.days() * time::DAY),
            Cut::All | Cut::AtLeast(_) => String::new(),
        };
        move |p| match self {
            Cut::All => true,
            Cut::OlderThan(_) => p.modified.as_deref().is_some_and(|m| m < line.as_str()),
            Cut::Within(_) => p.modified.as_deref().is_some_and(|m| m >= line.as_str()),
            Cut::AtLeast(bytes) => p.allocated_bytes >= bytes,
        }
    }
}

/// The cuts worth offering in `order`, each with how many paths it takes
/// and their bytes: those taking some but not all, each more than the one
/// before, then all of them.
pub fn cuts(paths: &[HintPath], order: PathOrder, now: i64) -> Vec<(Cut, usize, u64)> {
    use Age::*;
    let lines: Vec<Cut> = match order {
        PathOrder::Largest => [10 * GB, GB, 100 * MB, 10 * MB, MB].map(Cut::AtLeast).into(),
        PathOrder::Newest => [Day, Week, TwoWeeks, Month, ThreeMonths].map(Cut::Within).into(),
        PathOrder::Oldest => [Year, SixMonths, ThreeMonths, Month, TwoWeeks, Week].map(Cut::OlderThan).into(),
    };
    let mut out = Vec::new();
    let mut last = 0;
    for cut in lines.into_iter().chain([Cut::All]) {
        let takes = cut.test(now);
        let (n, bytes) =
            paths.iter().filter(|p| takes(p)).fold((0, 0), |(n, bytes), p| (n + 1, bytes + p.allocated_bytes));
        if n > last && (n < paths.len() || cut == Cut::All) {
            out.push((cut, n, bytes));
            last = n;
        }
    }
    out
}

impl Kura {
    /// When ages count from: the clock, unless a test fixed it.
    pub fn now(&self) -> i64 {
        self.now.unwrap_or_else(time::now_unix)
    }

    /// How a hint's paths are listed: as the person chose, else largest
    /// first.
    pub fn path_order(&self, h: &Hint) -> PathOrder {
        self.order.get(&hint_key(h)).copied().unwrap_or_default()
    }

    /// The paths a hint lists in the window, in the person's order: the
    /// first [`SHOWN`], unless the person (or a cut) asked for them all.
    pub fn shown_paths<'h>(&self, h: &'h Hint) -> Vec<&'h HintPath> {
        let mut out = ordered(&h.paths, self.path_order(h));
        if !self.show_all.contains(&hint_key(h)) {
            out.truncate(SHOWN);
        }
        out
    }

    /// A path ticked or unticked by hand: a run's first end.
    pub(super) fn choose(&mut self, path: String, on: bool) {
        self.choice.insert(path.clone(), on);
        self.anchor = Some(path);
    }

    /// A Shift-click on a path's tick: it and every path shown between it
    /// and the one last clicked become `on`.
    pub(super) fn choose_run(&mut self, i: usize, path: String, on: bool) {
        let hints = self.analysis.hints.clone();
        let Some(h) = hints.get(i) else { return };
        let shown = self.shown_paths(h);
        let at = |q: &str| shown.iter().position(|p| p.path == q);
        let run = match (self.anchor.as_deref().and_then(at), at(&path)) {
            (Some(a), Some(b)) => &shown[a.min(b)..=a.max(b)],
            _ => &[],
        };
        for p in run {
            self.choice.insert(p.path.clone(), on);
        }
        self.choose(path, on);
    }

    /// Tick the paths `cut` takes and untick the rest. Nothing is ticked
    /// out of sight: if it takes paths past those shown, they all show.
    pub(super) fn choose_cut(&mut self, i: usize, cut: Cut) {
        let hints = self.analysis.hints.clone();
        let Some(h) = hints.get(i) else { return };
        let takes = cut.test(self.now());
        for p in &h.paths {
            self.choice.insert(p.path.clone(), takes(p));
        }
        if ordered(&h.paths, self.path_order(h)).iter().skip(SHOWN).any(|p| takes(p)) {
            self.show_all.insert(hint_key(h));
        }
        self.anchor = None;
    }

    /// Untick all of a hint's paths, shown or not.
    pub(super) fn choose_none(&mut self, i: usize) {
        let hints = self.analysis.hints.clone();
        let Some(h) = hints.get(i) else { return };
        for p in &h.paths {
            self.choice.insert(p.path.clone(), false);
        }
        self.anchor = None;
    }

    /// Whether `path` is in its hint's action: as the person chose, else
    /// yes for safe and low-risk hints and no for the rest.
    pub fn chosen(&self, h: &Hint, path: &str) -> bool {
        self.choice.get(path).copied().unwrap_or(h.risk <= Risk::Low)
    }

    /// The paths a hint's action would move, and their bytes.
    pub fn chosen_paths(&self, h: &Hint) -> (Vec<PathBuf>, u64) {
        let mut bytes = 0;
        let paths = h
            .paths
            .iter()
            .filter(|p| self.chosen(h, &p.path))
            .map(|p| {
                bytes += p.allocated_bytes;
                PathBuf::from(&p.path)
            })
            .collect();
        (paths, bytes)
    }

    /// The copy of a set of duplicates that stays.
    pub fn kept(&self, hash: &str) -> usize {
        self.keep.get(hash).copied().unwrap_or(0)
    }

    pub fn hint_index(&self) -> Option<usize> {
        let k = self.focus_hint.as_ref()?;
        self.analysis.hints.iter().position(|h| &hint_key(h) == k)
    }

    /// Pick a hint: its paths light up, and the rings move to show them.
    pub(super) fn focus_hint(&mut self, i: Option<usize>, cx: &mut Cx<Msg>) {
        self.focus_hint = i.and_then(|i| self.analysis.hints.get(i)).map(hint_key);
        self.refresh_highlight();
        let Some(tree) = self.tree() else { return };
        if self.highlight.is_empty() {
            return;
        }
        // The rings go to the folder holding all of them, so they show.
        let lines: Vec<Vec<NodeId>> = self.highlight.iter().map(|&n| tree.lineage(n)).collect();
        let mut common = NodeId::ROOT;
        for d in 0.. {
            let Some(&first) = lines[0].get(d) else { break };
            if lines.iter().all(|l| l.get(d) == Some(&first)) {
                common = first;
            } else {
                break;
            }
        }
        if !tree.node(common).is_dir() || (self.highlight.len() == 1 && self.highlight[0] == common) {
            common = tree.parent(common).unwrap_or(NodeId::ROOT);
        }
        self.set_center(common, cx);
    }

    /// What the picked hint covers in the tree, and each segment's mark.
    pub(super) fn refresh_highlight(&mut self) {
        let mut nodes = Vec::new();
        if let (Some(i), Some(tree)) = (self.hint_index(), self.tree()) {
            let gone = tree.gone();
            let h = &self.analysis.hints[i];
            let paths = h
                .paths
                .iter()
                .map(|p| p.path.as_str())
                .chain(h.groups.iter().flat_map(|g| g.paths.iter().map(String::as_str)));
            for p in paths {
                if let Some(n) = tree.find(Path::new(p))
                    && !gone[n.index()]
                    && !nodes.contains(&n)
                {
                    nodes.push(n);
                }
            }
        }
        self.highlight = Rc::new(nodes);
        self.highlight_gen += 1;
        self.refresh_marks();
    }

    /// Each segment: lit (it's in the hint), plain (it holds something
    /// that is), or dimmed. None at all when no hint is picked.
    pub(super) fn refresh_marks(&mut self) {
        let marks = match self.tree() {
            Some(tree) if !self.highlight.is_empty() => self
                .segs
                .iter()
                .map(|s| {
                    let n = s.node.unwrap_or(s.parent);
                    if self.highlight.iter().any(|&h| tree.is_within(n, h)) {
                        Mark::Lit
                    } else if s.node.is_some() && self.highlight.iter().any(|&h| tree.is_within(h, n)) {
                        Mark::Plain
                    } else {
                        Mark::Dim
                    }
                })
                .collect(),
            _ => Vec::new(),
        };
        self.marks = Rc::new(marks);
    }

    /// Ask before a hint's action.
    pub(super) fn ask_act(&mut self, i: usize, cx: &mut Cx<Msg>) {
        let Some(h) = self.analysis.hints.get(i).cloned() else { return };
        match action_of(&h) {
            HintAction::Trash => {
                let (paths, bytes) = self.chosen_paths(&h);
                if paths.is_empty() {
                    self.notice = Some("Choose what to move first: open the list of paths and tick them.".into());
                    return;
                }
                let n = paths.len() as u64;
                let alert = Alert::new(
                    format!("Move {} to the Trash?", format::plural(n, "item", "items")),
                    format!(
                        "{} from “{}” will go to the Trash. The space is freed when the Trash is emptied; until then any file manager can put {} back.",
                        format::size(bytes),
                        h.title,
                        if n == 1 { "it" } else { "them" }
                    ),
                )
                .cancel_with("Cancel", Msg::CancelPending)
                .destructive_button("Move to Trash", Msg::Confirm);
                self.pending = Some(Pending::Trash { paths, what: h.title.clone() });
                cx.alert(self.window, alert);
            }
            HintAction::EmptyTrash => {
                let n = self.trash_dir.as_deref().map_or(0, |t| crate::trash::items(t).len()) as u64;
                let alert = Alert::new(
                    "Empty the Trash?",
                    format!(
                        "{} ({}) will be deleted for good. This can't be undone.",
                        format::plural(n, "item", "items"),
                        format::size(h.reclaimable_bytes)
                    ),
                )
                .cancel_with("Cancel", Msg::CancelPending)
                .destructive_button("Empty Trash", Msg::Confirm);
                self.pending = Some(Pending::EmptyTrash);
                cx.alert(self.window, alert);
            }
            HintAction::Yukimi => self.update(Msg::OpenYukimi, cx),
            HintAction::Command | HintAction::Duplicates => {}
        }
    }

    /// Ask before moving all but the kept copy of a set of duplicates.
    pub(super) fn ask_group(&mut self, i: usize, g: usize, cx: &mut Cx<Msg>) {
        let Some(group) = self.analysis.hints.get(i).and_then(|h| h.groups.get(g)).cloned() else { return };
        let kept = self.kept(&group.hash).min(group.paths.len().saturating_sub(1));
        let others: Vec<PathBuf> =
            group.paths.iter().enumerate().filter(|(j, _)| *j != kept).map(|(_, p)| PathBuf::from(p)).collect();
        if others.is_empty() {
            return;
        }
        let name =
            Path::new(&group.paths[kept]).file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        let n = others.len() as u64;
        let alert = Alert::new(
            format!("Move {} of “{name}” to the Trash?", format::plural(n, "copy", "copies")),
            format!(
                "Keeping {}. The other {} ({}) will go to the Trash, from where any file manager can put {} back.",
                format::path(Path::new(&group.paths[kept])),
                format::plural(n, "copy", "copies"),
                format::size(group.reclaimable_bytes),
                if n == 1 { "it" } else { "them" }
            ),
        )
        .cancel_with("Cancel", Msg::CancelPending)
        .destructive_button("Move to Trash", Msg::Confirm);
        self.pending = Some(Pending::Trash { paths: others, what: format!("copies of {name}") });
        cx.alert(self.window, alert);
    }

    /// The person said yes.
    pub(super) fn carry_out(&mut self, p: Pending, cx: &mut Cx<Msg>) {
        match p {
            Pending::Trash { paths, .. } => self.move_to_trash(paths, cx),
            Pending::EmptyTrash => {
                let Some(trash) = self.trash_dir.clone() else { return };
                self.busy += 1;
                self.notice = Some("Emptying the Trash…".into());
                cx.spawn(move || Msg::Emptied(crate::trash::empty(&trash).map_err(|e| e.to_string())));
            }
        }
    }

    /// Move `paths` to the Trash, in the background.
    pub(super) fn move_to_trash(&mut self, paths: Vec<PathBuf>, cx: &mut Cx<Msg>) {
        let (Some(scan), Some(trash)) = (self.scan.clone(), self.trash_dir.clone()) else { return };
        if paths.is_empty() {
            return;
        }
        self.busy += 1;
        self.notice = Some(format!("Moving {} to the Trash…", format::plural(paths.len() as u64, "item", "items")));
        cx.spawn(move || {
            let sizes = |p: &Path| scan.tree.find(p).map(|id| scan.tree.node(id).apparent);
            let out = crate::trash::trash(&paths, &trash, &sizes);
            Msg::Moved { moved: out.trashed.into_iter().map(|(p, _)| p).collect(), refused: out.refused }
        });
    }

    /// Things went to the Trash: the tree forgets them, and the folders
    /// they were in (and the Trash) are scanned again.
    pub(super) fn moved(&mut self, moved: Vec<PathBuf>, refused: Vec<(PathBuf, String)>, cx: &mut Cx<Msg>) {
        self.busy = self.busy.saturating_sub(1);
        let mut freed = 0;
        let mut folders = Vec::new();
        if let Some(scan) = &mut self.scan {
            let scan = Arc::make_mut(scan);
            for p in &moved {
                if let Some(n) = scan.tree.find(p) {
                    freed += scan.tree.node(n).alloc;
                    scan.tree.remove(n);
                }
                if let Some(parent) = p.parent() {
                    folders.push(parent.to_path_buf());
                }
            }
        }
        if let Some(t) = &self.trash_dir {
            folders.push(t.clone());
        }
        self.generation += 1;
        self.after_change();
        let mut notice = format!(
            "Moved {} to the Trash ({}, freed when the Trash is emptied).",
            format::plural(moved.len() as u64, "item", "items"),
            format::size(freed)
        );
        if let Some((p, why)) = refused.first() {
            let name = p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
            notice = format!("{notice} “{name}” stayed: {why}.");
        }
        self.notice = Some(notice);
        self.rescan(folders, cx);
    }

    pub(super) fn emptied(&mut self, r: Result<usize, String>, cx: &mut Cx<Msg>) {
        self.busy = self.busy.saturating_sub(1);
        match r {
            Ok(n) => {
                self.notice =
                    Some(format!("Emptied the Trash: {} deleted.", format::plural(n as u64, "item", "items")));
                let folders = self.trash_dir.clone().into_iter().collect();
                self.rescan(folders, cx);
            }
            Err(e) => self.notice = Some(format!("The Trash wasn't emptied: {e}.")),
        }
    }

    /// Scan `folders` again (those the tree holds, outermost only), then
    /// put them in the tree; with none, just look for hints again.
    pub(super) fn rescan(&mut self, folders: Vec<PathBuf>, cx: &mut Cx<Msg>) {
        let Some(tree) = self.tree() else { return };
        let gone = tree.gone();
        let mut found: Vec<(NodeId, PathBuf)> = folders
            .into_iter()
            .filter_map(|p| tree.find(&p).filter(|n| !gone[n.index()] && tree.node(*n).is_dir()).map(|n| (n, p)))
            .collect();
        found.sort_by_key(|(n, _)| tree.depth(*n));
        let mut outer: Vec<(NodeId, PathBuf)> = Vec::new();
        for (n, p) in found {
            if !outer.iter().any(|(o, _)| tree.is_within(n, *o)) {
                outer.push((n, p));
            }
        }
        if outer.is_empty() {
            self.write_index(cx);
            return;
        }
        let cross = self.scan.as_ref().is_some_and(|s| s.options.cross_filesystems);
        let paths: Vec<PathBuf> = outer.into_iter().map(|(_, p)| p).collect();
        self.busy += 1;
        cx.spawn(move || {
            let subs = paths
                .into_iter()
                .filter_map(|p| {
                    let options = ScanOptions { cross_filesystems: cross, ..ScanOptions::new(p.clone()) };
                    scan::scan(options, Progress::new()).ok().map(|s| (p, Arc::new(s.tree)))
                })
                .collect();
            Msg::Rescanned(subs)
        });
    }

    /// Folders scanned again: they take their old places in the tree.
    pub(super) fn rescanned(&mut self, subs: Vec<(PathBuf, Arc<Tree>)>, cx: &mut Cx<Msg>) {
        self.busy = self.busy.saturating_sub(1);
        let Some(tree) = self.tree() else { return };
        // Ids inside a grafted folder change: keep places by path.
        let center = tree.path(self.center);
        let selected = self.selected.map(|s| tree.path(s));
        let collected: Vec<PathBuf> = self.collector.iter().map(|&n| tree.path(n)).collect();
        if let Some(scan) = &mut self.scan {
            let scan = Arc::make_mut(scan);
            for (p, sub) in &subs {
                if let Some(n) = scan.tree.find(p)
                    && scan.tree.node(n).is_dir()
                {
                    scan.tree.graft(n, sub);
                }
            }
        }
        let Some(tree) = self.tree() else { return };
        let gone = tree.gone();
        let live = |p: &Path| tree.find(p).filter(|n| !gone[n.index()]);
        let mut c = center.as_path();
        let new_center = loop {
            if let Some(n) = live(c) {
                break n;
            }
            match c.parent() {
                Some(up) => c = up,
                None => break NodeId::ROOT,
            }
        };
        let new_selected = selected.as_deref().and_then(live);
        let new_collector: Vec<NodeId> = collected.iter().filter_map(|p| live(p)).collect();
        self.selected = new_selected;
        self.collector = new_collector;
        self.center = new_center;
        self.generation += 1;
        self.after_change();
        self.write_index(cx);
    }

    /// After the tree changed: what's gone leaves the Collector, the
    /// selection and the centre; the rings and the list are made again.
    pub(super) fn after_change(&mut self) {
        if let Some(t) = self.tree() {
            let gone = t.gone();
            let mut c = self.center;
            while gone[c.index()] {
                c = t.parent(c).unwrap_or(NodeId::ROOT);
            }
            let lost_selection = self.selected.is_some_and(|s| gone[s.index()]);
            self.collector.retain(|n| !gone[n.index()]);
            self.center = c;
            if lost_selection {
                self.selected = None;
            }
        }
        self.transition = None;
        self.outer_segs = None;
        self.refresh_rows();
        self.refresh_highlight();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(path: &str, modified: Option<&str>) -> HintPath {
        HintPath {
            path: path.into(),
            allocated_bytes: 0,
            apparent_bytes: 0,
            modified: modified.map(Into::into),
            note: None,
            command: None,
        }
    }

    #[test]
    fn paths_order_by_when_they_were_modified() {
        let paths = [
            at("a", Some("2025-03-01T00:00:00Z")),
            at("b", None),
            at("c", Some("2024-12-31T23:59:59Z")),
            at("d", Some("2025-11-02T08:00:00Z")),
            at("e", Some("2025-03-01T00:00:00Z")),
        ];
        let names = |o| ordered(&paths, o).iter().map(|p| p.path.as_str()).collect::<Vec<_>>();
        assert_eq!(names(PathOrder::Largest), ["a", "b", "c", "d", "e"]);
        assert_eq!(names(PathOrder::Newest), ["d", "a", "e", "c", "b"]);
        assert_eq!(names(PathOrder::Oldest), ["c", "a", "e", "d", "b"]);
    }

    #[test]
    fn cuts_follow_the_order_and_skip_those_that_add_nothing() {
        const NOW: i64 = 1_762_300_800;
        assert_eq!(time::rfc3339(NOW), "2025-11-05T00:00:00Z");
        let sized = |path, bytes, modified| HintPath { allocated_bytes: bytes, ..at(path, modified) };
        let paths = [
            sized("a", 2_500 * MB, Some("2024-06-01T00:00:00Z")),
            sized("b", 300 * MB, Some("2025-04-01T00:00:00Z")),
            sized("c", 40 * MB, Some("2025-10-01T00:00:00Z")),
            sized("d", 5 * MB, Some("2025-10-30T00:00:00Z")),
            sized("e", 200_000, None),
        ];
        let all = (Cut::All, 5, 2_845 * MB + 200_000);
        assert_eq!(
            cuts(&paths, PathOrder::Oldest, NOW),
            [
                (Cut::OlderThan(Age::Year), 1, 2_500 * MB),
                (Cut::OlderThan(Age::SixMonths), 2, 2_800 * MB),
                (Cut::OlderThan(Age::Month), 3, 2_840 * MB),
                all,
            ]
        );
        assert_eq!(
            cuts(&paths, PathOrder::Newest, NOW),
            [(Cut::Within(Age::Week), 1, 5 * MB), (Cut::Within(Age::ThreeMonths), 2, 45 * MB), all]
        );
        assert_eq!(
            cuts(&paths, PathOrder::Largest, NOW),
            [
                (Cut::AtLeast(GB), 1, 2_500 * MB),
                (Cut::AtLeast(100 * MB), 2, 2_800 * MB),
                (Cut::AtLeast(10 * MB), 3, 2_840 * MB),
                (Cut::AtLeast(MB), 4, 2_845 * MB),
                all,
            ]
        );
        // Each cut is a run from the top of its order.
        for order in PathOrder::ALL {
            for (cut, n, _) in cuts(&paths, order, NOW) {
                let takes = cut.test(NOW);
                assert!(ordered(&paths, order).iter().take(n).all(|p| takes(p)), "{cut:?} in {order:?}");
            }
        }
        assert!(cuts(&[], PathOrder::Oldest, NOW).is_empty());
    }
}

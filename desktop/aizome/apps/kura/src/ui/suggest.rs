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
use super::{Kura, Msg, Pending, hint_key};
use crate::format;
use crate::hints::{Hint, Risk};
use crate::scan::{self, Progress, ScanOptions};
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

impl Kura {
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

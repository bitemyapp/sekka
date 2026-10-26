//! `index.md`: the index as a report a person or an agent reads top to
//! bottom: what fills the disk, what could be freed and how, and the
//! largest things.

use std::fmt::Write;
use std::path::Path;

use crate::format::{self, count, size};
use crate::hints::{ActionKind, Hint};
use crate::index::Index;

fn p(s: &str) -> String {
    format::path(Path::new(s))
}

/// Escape what Markdown tables can't hold.
fn cell(s: &str) -> String {
    s.replace('|', "\\|").replace('\n', " ")
}

/// The whole report.
pub fn markdown(ix: &Index) -> String {
    let mut s = String::new();
    let t = &ix.totals;
    let _ = writeln!(s, "# Disk usage of {}\n", p(&ix.root));
    let _ = writeln!(
        s,
        "Scanned {} in {:.1} s: {} and {}, **{} on disk** ({} apparent).",
        ix.scanned_at.replace('T', " ").replace('Z', " UTC"),
        ix.duration_ms as f64 / 1000.0,
        format::plural(t.files, "file", "files"),
        format::plural(t.directories, "folder", "folders"),
        size(t.allocated_bytes),
        size(t.apparent_bytes),
    );
    if let Some(fs) = &ix.filesystem {
        let _ = writeln!(
            s,
            "The filesystem it's on ({}, {}, mounted at `{}`) has **{} free** of {} ({} used).",
            fs.source,
            fs.fs_type,
            fs.mount_point,
            size(fs.available_bytes),
            size(fs.total_bytes),
            format::percent(fs.used_bytes, fs.total_bytes)
        );
    }
    if t.errors > 0 {
        let _ = writeln!(
            s,
            "{} couldn't be read (permissions); their contents aren't counted.",
            format::plural(t.errors, "path", "paths")
        );
    }
    if !ix.other_filesystems.is_empty() {
        let _ = writeln!(
            s,
            "Other filesystems mounted inside weren't scanned: {}.",
            ix.other_filesystems.iter().map(|m| format!("`{}`", m)).collect::<Vec<_>>().join(", ")
        );
    }

    // What could be freed.
    let _ = writeln!(s, "\n## What could be freed\n");
    if ix.hints.is_empty() {
        let _ = writeln!(s, "Nothing stands out: no caches, build outputs, old downloads or duplicates of note.");
    } else {
        let by = &ix.reclaimable.by_risk;
        let get = |k: &str| by.get(k).copied().unwrap_or(0);
        let _ = writeln!(
            s,
            "About **{}** in all: {} safe, {} low risk, {} needing a person's decision (medium).\n",
            size(ix.reclaimable.total_bytes),
            size(get("safe")),
            size(get("low")),
            size(get("medium") + get("high")),
        );
        let _ = writeln!(s, "| Hint | Frees | Risk | What to do |");
        let _ = writeln!(s, "|---|---:|---|---|");
        for h in &ix.hints {
            let frees = h.frees();
            let _ =
                writeln!(s, "| {} | {} | {} | {} |", cell(&h.title), frees, h.risk.id(), cell(&h.action.description));
        }
        for h in &ix.hints {
            hint_section(&mut s, h);
        }
    }

    // Largest.
    let _ = writeln!(s, "\n## Largest folders\n");
    let _ = writeln!(s, "| Folder | Size | Items | Modified |");
    let _ = writeln!(s, "|---|---:|---:|---|");
    for d in ix.largest_directories.iter().take(30) {
        let _ = writeln!(
            s,
            "| `{}` | {} | {} | {} |",
            cell(&p(&d.path)),
            size(d.allocated_bytes),
            count(d.items.unwrap_or(0)),
            &d.modified[..10.min(d.modified.len())]
        );
    }
    let _ = writeln!(s, "\n## Largest files\n");
    let _ = writeln!(s, "| File | Size | Kind | Modified |");
    let _ = writeln!(s, "|---|---:|---|---|");
    for f in ix.largest_files.iter().take(30) {
        let _ = writeln!(
            s,
            "| `{}` | {} | {} | {} |",
            cell(&p(&f.path)),
            size(f.allocated_bytes),
            f.kind,
            &f.modified[..10.min(f.modified.len())]
        );
    }

    // Kinds.
    let _ = writeln!(s, "\n## By kind\n");
    let _ = writeln!(s, "| Kind | Size | Files |");
    let _ = writeln!(s, "|---|---:|---:|");
    for k in &ix.kinds {
        let _ = writeln!(s, "| {} | {} | {} |", k.title, size(k.allocated_bytes), count(k.files));
    }

    // Top of the tree.
    let _ = writeln!(s, "\n## Top level\n");
    for c in ix.tree.children.iter().take(25) {
        let _ = writeln!(
            s,
            "- `{}` {} ({})",
            c.name,
            size(c.allocated_bytes),
            format::percent(c.allocated_bytes, ix.tree.allocated_bytes)
        );
    }
    if let Some(o) = &ix.tree.omitted {
        let _ = writeln!(s, "- {} smaller: {}", format::plural(o.count, "item", "items"), size(o.allocated_bytes));
    }

    // Nix.
    if let Some(n) = &ix.nix {
        let _ = writeln!(s, "\n## Nix\n");
        match n.store_bytes {
            Some(b) => {
                let _ = writeln!(
                    s,
                    "The Nix store (`{}`) holds {} in {} paths.",
                    n.store,
                    size(b),
                    count(n.store_paths.unwrap_or(0))
                );
            }
            None => {
                let _ = writeln!(s, "There's a Nix store at `{}` (not measured this time).", n.store);
            }
        }
        for pr in &n.profiles {
            let _ = writeln!(
                s,
                "- `{}`: {} generations{}",
                pr.path.display(),
                pr.generations,
                pr.current.map(|c| format!(", current {c}")).unwrap_or_default()
            );
        }
        let _ = writeln!(s, "\n{} Run `{}`.", n.note, n.cleanup_command);
    }

    let _ = writeln!(s, "\n## For agents\n");
    let _ = writeln!(
        s,
        "- Everything here is in `index.json` beside this file (schema `{}` version {}; see Aizome's `docs/kura-index.md`).",
        ix.schema, ix.version
    );
    let _ = writeln!(s, "- Sizes are allocated bytes: what removing a file frees. Hard links count once.");
    let _ = writeln!(
        s,
        "- Ask the person before acting on anything above `safe`; show them what goes and how much it frees."
    );
    let _ = writeln!(
        s,
        "- `aizome-kura trash PATH…` moves things to the Trash (reversible, by the freedesktop spec); it refuses the Nix store."
    );
    let _ =
        writeln!(s, "- Never remove paths in `/nix/store`: open Yukimi's Storage (`aizome-yukimi --page storage`).");
    let _ =
        writeln!(s, "- `aizome-kura scan` refreshes this report; `aizome-kura hints --json` prints just the hints.");
    s
}

fn hint_section(s: &mut String, h: &Hint) {
    let frees = h.frees();
    let _ = writeln!(s, "\n### {} ({}, {} risk)\n", h.title, frees, h.risk.id());
    let _ = writeln!(s, "{}\n", h.summary);
    let what = match h.action.kind {
        ActionKind::Command => "command",
        ActionKind::Trash => "move to the Trash",
        ActionKind::OpenApp => "open an app",
        ActionKind::Review => "review",
    };
    let _ = writeln!(s, "**What to do** ({what}): {}\n", h.action.description);
    if let Some(c) = &h.action.command {
        let _ = writeln!(s, "```sh\n{c}\n```\n");
    }
    if !h.groups.is_empty() {
        for g in h.groups.iter().take(15) {
            let _ = writeln!(
                s,
                "- {} each, {} copies ({} hash), {} reclaimable:",
                size(g.bytes_each),
                g.paths.len(),
                g.verified,
                size(g.reclaimable_bytes)
            );
            for path in &g.paths {
                let _ = writeln!(s, "  - `{}`", p(path));
            }
        }
        if h.groups.len() > 15 {
            let _ = writeln!(s, "- …and {} more sets in index.json", h.groups.len() - 15);
        }
        return;
    }
    for hp in h.paths.iter().take(15) {
        let mut line = format!("- `{}`", p(&hp.path));
        if hp.allocated_bytes > 0 {
            let _ = write!(line, " {}", size(hp.allocated_bytes));
        }
        if let Some(n) = &hp.note {
            let _ = write!(line, ": {n}");
        }
        if let Some(c) = &hp.command {
            let _ = write!(line, " (`{c}`)");
        }
        let _ = writeln!(s, "{line}");
    }
    let more = h.paths.len().saturating_sub(15) as u64 + h.more_paths;
    if more > 0 {
        let _ = writeln!(s, "- …and {} more", format::plural(more, "path", "paths"));
    }
}

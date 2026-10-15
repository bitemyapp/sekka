// SPDX-License-Identifier: MIT OR Apache-2.0
//! Every version of the system that can be returned to, newest first, each
//! with what changed from the one before it, worked out (away from the
//! interface thread) when it is opened.
use std::cell::Cell;

use adw::prelude::*;
use gtk::{gio, glib};
use yukimi_system::diff::{self, Change, ChangeKind};
use yukimi_system::generations::Generation;
use yukimi_system::{human_age, human_size, now};

use super::Ctx;
use super::widgets::{badge, clear, dim, fill_later, heading, page, pending_row};
use crate::ops::Operation;

/// Rows shown of one list of changes; the rest are left out.
const SHOWN: usize = 200;

/// How much something grew or shrank: `+132.7 KiB`.
fn size_change(delta: i64) -> String {
    let sign = if delta > 0 { "+" } else { "−" };
    format!("{sign}{}", human_size(delta.unsigned_abs()))
}

/// The note above a list of changes too long to show whole.
fn first_of(total: usize) -> adw::ActionRow {
    let note = adw::ActionRow::new();
    note.set_title(&format!("The first {SHOWN} of {total} changes"));
    note.add_css_class("dim-row");
    note
}

fn change_row(change: &Change) -> adw::ActionRow {
    let row = adw::ActionRow::new();
    row.set_title(&change.name);
    let (icon, class, text) = match change.kind {
        ChangeKind::Upgraded => {
            ("go-up-symbolic", "upgraded", format!("{} → {}", change.before.join(", "), change.after.join(", ")))
        }
        ChangeKind::Downgraded => {
            ("go-down-symbolic", "downgraded", format!("{} → {}", change.before.join(", "), change.after.join(", ")))
        }
        ChangeKind::Added => ("list-add-symbolic", "added", change.after.join(", ")),
        ChangeKind::Removed => ("list-remove-symbolic", "removed", change.before.join(", ")),
        ChangeKind::Changed => {
            ("view-refresh-symbolic", "changed", format!("{} → {}", change.before.join(", "), change.after.join(", ")))
        }
        ChangeKind::Rebuilt => ("view-refresh-symbolic", "rebuilt", change.after.join(", ")),
    };
    let image = gtk::Image::from_icon_name(icon);
    image.add_css_class(class);
    row.add_prefix(&image);
    row.set_subtitle(&text);
    if change.size_delta != 0 {
        row.add_suffix(&dim(&size_change(change.size_delta)));
    }
    row
}

/// The packages built again at the same versions, as one row that lists
/// them when opened: an update of Nixpkgs can rebuild hundreds.
fn rebuilt_row(rebuilt: &[Change]) -> adw::ExpanderRow {
    let row = adw::ExpanderRow::new();
    let n = rebuilt.len();
    row.set_title(&format!("{n} {} rebuilt at the same version", if n == 1 { "package" } else { "packages" }));
    let names: Vec<&str> = rebuilt.iter().take(6).map(|c| c.name.as_str()).collect();
    row.set_subtitle(&format!("{}{}", names.join(", "), if n > 6 { "…" } else { "" }));
    let image = gtk::Image::from_icon_name("view-refresh-symbolic");
    image.add_css_class("rebuilt");
    row.add_prefix(&image);
    let delta: i64 = rebuilt.iter().map(|c| c.size_delta).sum();
    if delta != 0 {
        row.add_suffix(&dim(&size_change(delta)));
    }
    let (rebuilt, filled) = (rebuilt.to_vec(), Cell::new(false));
    row.connect_expanded_notify(move |row| {
        if !row.is_expanded() || filled.replace(true) {
            return;
        }
        if rebuilt.len() > SHOWN {
            row.add_row(&first_of(rebuilt.len()));
        }
        let list = row.clone();
        fill_later(row, rebuilt.iter().take(SHOWN).cloned().collect(), change_row, move |r| list.add_row(r));
    });
    row
}

/// A row of what a generation changed.
enum Line {
    Change(Change),
    /// The packages rebuilt at the same versions, after every other change.
    Rebuilt(Vec<Change>),
}

fn line_row(line: &Line) -> gtk::Widget {
    match line {
        Line::Change(change) => change_row(change).upcast(),
        Line::Rebuilt(rebuilt) => rebuilt_row(rebuilt).upcast(),
    }
}

/// The row offering to go back to a generation.
fn return_row(ctx: &Ctx, number: u32, kept: bool) -> adw::ActionRow {
    let back = adw::ActionRow::new();
    back.set_title("Return to this version");
    back.set_subtitle(if kept {
        "The running system and your choices go back to this generation. The newer ones stay too."
    } else {
        "The running system goes back to this generation. The newer ones stay too."
    });
    let button = gtk::Button::with_label("Return");
    button.add_css_class("pill");
    button.set_valign(gtk::Align::Center);
    let ctx = ctx.clone();
    button.connect_clicked(move |_| {
        ctx.confirm(
            &format!("Return to generation {number}?"),
            if kept {
                "The running system switches to this version, after an administrator password, and your choices \
                 go back to what they were then (files you have changed by hand since stay as they are). Newer \
                 generations are kept, so you can come back."
            } else {
                "The running system switches to this version, after an administrator password. Yukimi didn't keep \
                 the choices it was made from, so your next change builds on today's choices. Newer generations \
                 are kept, so you can come back."
            },
            "Return",
            Operation::Rollback { generation: number },
        );
    });
    back.add_suffix(&button);
    back
}

/// Fill an opened generation's row: going back to it, and what changed
/// from the one before, both read in the background.
fn fill(ctx: &Ctx, row: &adw::ExpanderRow, this: Generation, previous: Option<Generation>) {
    let waiting = pending_row("Working out what changed…");
    row.add_row(&waiting);
    let (ctx, row) = (ctx.clone(), row.clone());
    glib::spawn_future_local(async move {
        let store = ctx.model().store.clone();
        let number = this.number;
        let found = gio::spawn_blocking(move || {
            let kept = yukimi_system::saved_configuration(number).is_dir();
            let diff = match (&store, &previous) {
                (Some(store), Some(previous)) => {
                    let (before, after) = (store.closure_of(&previous.target), store.closure_of(&this.target));
                    Some(diff::diff(&store.graph, &before, &after))
                }
                _ => None,
            };
            (kept, diff, store.is_some(), previous.is_some())
        })
        .await;
        row.remove(&waiting);
        let Ok((kept, diff, store_read, has_previous)) = found else {
            return;
        };
        if !this.current {
            row.add_row(&return_row(&ctx, number, kept));
        }
        if !has_previous {
            let first = adw::ActionRow::new();
            first.set_title("The oldest generation kept");
            row.add_row(&first);
            return;
        }
        let Some(diff) = diff else {
            let later = adw::ActionRow::new();
            later.set_title(if store_read {
                "What changed couldn't be worked out"
            } else {
                "The store is still being read; open this again in a moment"
            });
            row.add_row(&later);
            return;
        };
        // No package path differs, versions or builds: what changed is
        // configuration.
        if diff.changes.is_empty() {
            let same = adw::ActionRow::new();
            same.set_title("The same packages as the generation before; only settings changed");
            row.add_row(&same);
        }
        let (installed, rebuilt) = diff.split();
        if installed.len() > SHOWN {
            row.add_row(&first_of(installed.len()));
        }
        let mut lines: Vec<Line> = installed.iter().take(SHOWN).cloned().map(Line::Change).collect();
        if !rebuilt.is_empty() {
            lines.push(Line::Rebuilt(rebuilt.to_vec()));
        }
        let list = row.clone();
        fill_later(&row, lines, line_row, move |r| list.add_row(r));
    });
}

pub fn build(ctx: &Ctx) -> gtk::ScrolledWindow {
    let content = gtk::Box::new(gtk::Orientation::Vertical, 24);
    content.set_margin_top(24);
    content.set_margin_bottom(36);
    content.set_margin_start(18);
    content.set_margin_end(18);
    let scroll = page(&content);
    ctx.on_refresh("history", move |ctx| {
        clear(&content);
        let model = ctx.model();
        content.append(&heading(
            "Every version of your system",
            "Each time the system changes, NixOS keeps the version before as a generation. Any of them can be \
             started from the boot menu, or returned to here. Nothing is lost until old generations are cleaned \
             up in Storage.",
        ));
        let group = adw::PreferencesGroup::new();
        let gens = &model.generations;
        if gens.is_empty() {
            let row = adw::ActionRow::new();
            row.set_title("No generations found");
            group.add(&row);
        }
        for (i, generation) in gens.iter().enumerate().rev() {
            let row = adw::ExpanderRow::new();
            row.set_title(&format!("Generation {}", generation.number));
            let mut subtitle = human_age(generation.created, now());
            if let Some(version) = &generation.nixos_version {
                subtitle.push_str(&format!("  ·  NixOS {version}"));
            }
            if let Some(kernel) = &generation.kernel_version {
                subtitle.push_str(&format!("  ·  Linux {kernel}"));
            }
            row.set_subtitle(&subtitle);
            if generation.active {
                row.add_suffix(&badge("running", "current"));
            }
            if generation.current && !generation.active {
                row.add_suffix(&badge("selected for boot", "current"));
            }
            if generation.booted && !generation.active {
                row.add_suffix(&badge("started with", "booted"));
            }
            let previous = i.checked_sub(1).map(|p| gens[p].clone());
            let this = generation.clone();
            let (ctx2, filled) = (ctx.clone(), std::cell::Cell::new(false));
            row.connect_expanded_notify(move |row| {
                if row.is_expanded() && !filled.replace(true) {
                    fill(&ctx2, row, this.clone(), previous.clone());
                }
            });
            group.add(&row);
        }
        content.append(&group);
    });
    scroll
}

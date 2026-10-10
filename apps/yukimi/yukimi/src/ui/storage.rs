// SPDX-License-Identifier: MIT OR Apache-2.0
//! The store: how big it is, what keeps each part of it, what is heaviest,
//! and cleaning up what nothing needs any more.
use adw::prelude::*;
use yukimi_store::RootKind;
use yukimi_system::human_size;

use super::Ctx;
use super::widgets::{clear, dim, fill_later, heading, page, pending_row, share_bar};
use crate::model::Composition;
use crate::ops::Operation;

type Colour = (f64, f64, f64);

/// The parts of the store: a title, what it is, its size, and the colour
/// it is drawn in.
fn parts(c: &Composition) -> [(&'static str, &'static str, u64, Colour); 5] {
    [
        ("The running system", "Everything the system you are using needs.", c.system, (0.32, 0.47, 0.76)),
        ("Older generations", "Kept so you can return to them.", c.old_generations, (0.49, 0.73, 0.89)),
        ("Packages just for users", "Installed into people's own profiles.", c.profiles, (0.56, 0.44, 0.80)),
        ("Projects and dev shells", "Build results and development environments.", c.projects, (0.36, 0.70, 0.56)),
        ("Garbage", "Needed by nothing. Cleaning up removes it.", c.garbage, (0.62, 0.64, 0.68)),
    ]
}

/// One bar for the whole store, in the colours of its parts.
fn composition_bar(c: &Composition) -> gtk::DrawingArea {
    let area = gtk::DrawingArea::new();
    area.set_content_height(28);
    area.set_hexpand(true);
    let segments: Vec<(f64, Colour)> =
        parts(c).iter().map(|(_, _, size, colour)| (*size as f64 / c.total.max(1) as f64, *colour)).collect();
    area.set_draw_func(move |_, cr, width, height| {
        let (w, h) = (width as f64, height as f64);
        let radius = h / 2.0;
        // Clip to a pill, then lay the segments side by side.
        cr.new_sub_path();
        cr.arc(radius, radius, radius, std::f64::consts::PI / 2.0, 3.0 * std::f64::consts::PI / 2.0);
        cr.arc(w - radius, radius, radius, -std::f64::consts::PI / 2.0, std::f64::consts::PI / 2.0);
        cr.close_path();
        cr.clip();
        let mut x = 0.0;
        for (fraction, (r, g, b)) in &segments {
            let width = fraction * w;
            cr.set_source_rgb(*r, *g, *b);
            cr.rectangle(x, 0.0, width + 0.5, h);
            let _ = cr.fill();
            x += width;
        }
    });
    area
}

fn swatch((r, g, b): Colour) -> gtk::DrawingArea {
    let area = gtk::DrawingArea::new();
    area.set_content_width(14);
    area.set_content_height(14);
    area.set_valign(gtk::Align::Center);
    area.set_draw_func(move |_, cr, w, h| {
        cr.set_source_rgb(r, g, b);
        cr.arc(w as f64 / 2.0, h as f64 / 2.0, w.min(h) as f64 / 2.0, 0.0, std::f64::consts::TAU);
        let _ = cr.fill();
    });
    area
}

fn root_kind(kind: &RootKind) -> String {
    match kind {
        RootKind::CurrentSystem => "The running system".to_owned(),
        RootKind::BootedSystem => "The system this computer started with".to_owned(),
        RootKind::SystemGeneration(n) => format!("System generation {n}"),
        RootKind::Profile { name, user, generation } => {
            let whose = user.as_deref().map(|u| format!("{u}'s ")).unwrap_or_default();
            match generation {
                Some(g) => format!("{whose}{name} profile, generation {g}"),
                None => format!("{whose}{name} profile"),
            }
        }
        RootKind::BuildResult => "A build result".to_owned(),
        RootKind::DevShell => "A development shell".to_owned(),
        RootKind::Other => "Other".to_owned(),
    }
}

pub fn build(ctx: &Ctx) -> gtk::ScrolledWindow {
    let content = gtk::Box::new(gtk::Orientation::Vertical, 24);
    content.set_margin_top(24);
    content.set_margin_bottom(36);
    content.set_margin_start(18);
    content.set_margin_end(18);
    let scroll = page(&content);
    ctx.on_refresh("storage", move |ctx| {
        clear(&content);
        let model = ctx.model();
        content.append(&heading(
            "The Nix store",
            "Everything installed lives in /nix/store, each version in its own folder, which is why several can \
             live side by side and why going back is instant. Here is what keeps each part of it.",
        ));
        let Some(store) = model.store.clone() else {
            let group = adw::PreferencesGroup::new();
            group.add(&pending_row("Reading the store…"));
            content.append(&group);
            return;
        };
        let c = &store.composition;
        let size = gtk::Box::new(gtk::Orientation::Vertical, 0);
        let total = gtk::Label::new(Some(&human_size(c.total)));
        total.add_css_class("stat-number");
        total.set_xalign(0.0);
        let paths = dim(&format!("in {} store paths", c.paths));
        paths.set_xalign(0.0);
        size.append(&total);
        size.append(&paths);
        content.append(&size);
        content.append(&composition_bar(c));

        let legend = adw::PreferencesGroup::new();
        for (title, explanation, size, colour) in parts(c) {
            let row = adw::ActionRow::new();
            row.set_title(title);
            row.set_subtitle(explanation);
            row.add_prefix(&swatch(colour));
            row.add_suffix(&dim(&human_size(size)));
            legend.add(&row);
        }
        content.append(&legend);

        // Cleaning up.
        let clean = adw::PreferencesGroup::new();
        clean.set_title("Clean up");
        clean.set_description(Some(
            "Deleting old generations lets their packages go; collecting garbage removes everything nothing needs. \
             The running system and the one this computer started with are always kept.",
        ));
        let older = adw::ComboRow::new();
        older.set_title("Delete system generations older than");
        older.set_model(Some(&gtk::StringList::new(&["a week", "a month", "three months", "keep them all"])));
        older.set_selected(1);
        clean.add(&older);
        let go = adw::ActionRow::new();
        go.set_title("Clean up now");
        go.set_subtitle(&format!("Frees at least {} now, and more as old generations go", human_size(c.garbage)));
        let button = gtk::Button::with_label("Clean up");
        button.add_css_class("pill");
        button.add_css_class("destructive-action");
        button.set_valign(gtk::Align::Center);
        {
            let (ctx, older) = (ctx.clone(), older.clone());
            button.connect_clicked(move |_| {
                let days = match older.selected() {
                    0 => Some(7),
                    1 => Some(30),
                    2 => Some(90),
                    _ => None,
                };
                ctx.confirm(
                    "Clean up the store?",
                    &match days {
                        Some(d) => format!(
                            "System generations older than {d} days are deleted, then everything nothing needs is \
                             removed. This asks for an administrator password."
                        ),
                        None => "Everything nothing needs is removed; all generations are kept. This asks for an \
                                 administrator password."
                            .to_owned(),
                    },
                    "Clean up",
                    Operation::Clean { older_than_days: days },
                );
            });
        }
        go.add_suffix(&button);
        clean.add(&go);
        content.append(&clean);

        // The heaviest packages.
        let heavy = adw::PreferencesGroup::new();
        heavy.set_title("Heaviest in the running system");
        let largest = store.heaviest.first().map(|h| h.size).unwrap_or(1).max(1);
        for package in &store.heaviest {
            let row = adw::ActionRow::new();
            row.set_title(&package.name);
            if !package.version.is_empty() {
                row.set_subtitle(&package.version);
            }
            row.add_suffix(&share_bar(package.size as f64 / largest as f64));
            row.add_suffix(&dim(&human_size(package.size)));
            heavy.add(&row);
        }
        content.append(&heavy);

        // What keeps things alive.
        let roots = adw::PreferencesGroup::new();
        roots.set_title("What keeps things in the store");
        roots.set_description(Some("Garbage-collector roots: links to store paths that keep them and all they need."));
        let expander = adw::ExpanderRow::new();
        expander.set_title(&format!("{} roots", store.roots.len()));
        // Built when opened, a batch at a time: there can be thousands.
        let (filled, store) = (std::cell::Cell::new(false), store.clone());
        expander.connect_expanded_notify(move |expander| {
            if !expander.is_expanded() || filled.replace(true) {
                return;
            }
            let list = expander.clone();
            fill_later(
                expander,
                store.roots.clone(),
                |root| {
                    let row = adw::ActionRow::new();
                    row.set_title(&root_kind(&root.kind));
                    row.set_subtitle(&root.link.display().to_string());
                    row.set_subtitle_selectable(true);
                    row.set_tooltip_markup(Some(&super::widgets::store_path_markup(root.target.as_str())));
                    row
                },
                move |row| list.add_row(row),
            );
        });
        roots.add(&expander);
        content.append(&roots);
    });
    scroll
}

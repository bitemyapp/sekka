// SPDX-License-Identifier: MIT OR Apache-2.0
//! The first page: this computer, its packages, how fresh it is, and how
//! much room the store takes, at a glance.
use adw::prelude::*;
use yukimi_config::lock::Movement;
use yukimi_system::diff::ChangeKind;
use yukimi_system::setup::Kind;
use yukimi_system::{human_age, human_size, now};

use super::widgets::{clear, page, snowfall, wrapping};
use super::{Ctx, updates};
use crate::model::Model;

fn greeting() -> &'static str {
    let hour = gtk::glib::DateTime::now_local().map(|t| t.hour()).unwrap_or(12);
    match hour {
        5..12 => "Good morning",
        12..18 => "Good afternoon",
        18..23 => "Good evening",
        _ => "Still up?",
    }
}

fn hero(ctx: &Ctx) -> gtk::Overlay {
    let model = ctx.model();
    let column = gtk::Box::new(gtk::Orientation::Vertical, 6);
    column.add_css_class("hero");
    let hello = gtk::Label::new(Some(greeting()));
    hello.add_css_class("hero-greeting");
    hello.set_xalign(0.0);
    let name =
        gtk::Label::new(Some(if model.info.hostname.is_empty() { "This computer" } else { &model.info.hostname }));
    name.add_css_class("hero-title");
    name.set_xalign(0.0);
    let mut facts = Vec::new();
    if !model.info.nixos_version.is_empty() {
        facts.push(format!("NixOS {}", model.info.release()));
    }
    if let Some(generation) = model.current_generation() {
        facts.push(format!("generation {}, built {}", generation.number, human_age(generation.created, now())));
    }
    if !model.info.kernel.is_empty() {
        facts.push(format!("Linux {}", model.info.kernel));
    }
    let subtitle = gtk::Label::new(Some(&if ctx.loading() && facts.is_empty() {
        "Looking around…".to_owned()
    } else {
        facts.join("  ·  ")
    }));
    subtitle.add_css_class("hero-subtitle");
    subtitle.set_xalign(0.0);
    subtitle.set_wrap(true);
    column.append(&hello);
    column.append(&name);
    column.append(&subtitle);
    if model.info.restart_needed {
        let pill = gtk::Label::new(Some("Restart to finish an update"));
        pill.add_css_class("hero-pill");
        pill.set_halign(gtk::Align::Start);
        pill.set_margin_top(8);
        column.append(&pill);
    }
    let overlay = gtk::Overlay::new();
    overlay.set_child(Some(&column));
    overlay.add_overlay(&snowfall(70));
    overlay.add_css_class("hero-frame");
    overlay
}

/// A card with a big number, a line under it, and a button.
fn card(icon: &str, title: &str, big: &str, line: &str, button: (&str, &str), ctx: &Ctx) -> gtk::Box {
    let card = gtk::Box::new(gtk::Orientation::Vertical, 6);
    card.add_css_class("card");
    card.add_css_class("stat-card");
    card.set_hexpand(true);
    let top = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    let image = gtk::Image::from_icon_name(icon);
    image.add_css_class("stat-icon");
    let label = gtk::Label::new(Some(title));
    label.add_css_class("heading");
    top.append(&image);
    top.append(&label);
    let number = gtk::Label::new(Some(big));
    number.add_css_class("stat-number");
    number.set_xalign(0.0);
    let text = wrapping(line, &["dim-label"]);
    text.set_vexpand(true);
    text.set_valign(gtk::Align::Start);
    let go = gtk::Button::with_label(button.0);
    go.add_css_class("pill");
    go.set_halign(gtk::Align::Start);
    let place = button.1.to_owned();
    let ctx = ctx.clone();
    go.connect_clicked(move |_| ctx.show(&place));
    card.append(&top);
    card.append(&number);
    card.append(&text);
    card.append(&go);
    card
}

/// When the system's Nixpkgs was made, as Unix time: as the lock file says
/// for a flake, or as the NixOS version's date (`26.11.20261001.c59305b`).
fn nixpkgs_made(model: &Model) -> Option<i64> {
    let locked = model.inputs.iter().filter(|i| i.name == "nixpkgs").find_map(|i| i.locked.as_ref()?.last_modified);
    locked.or_else(|| {
        let date = model.info.nixos_version.split('.').nth(2).filter(|d| d.len() == 8)?;
        yukimi_system::updates::parse_time(&format!("{}-{}-{}T00:00:00Z", &date[..4], &date[4..6], &date[6..8]))
    })
}

/// The freshness card's number and line.
fn freshness(ctx: &Ctx, model: &Model) -> (String, String) {
    let (big, mut line) = match (nixpkgs_made(model), model.setup.kind) {
        (Some(made), _) => {
            (human_age(made, now()), "since the version of Nixpkgs your system is built from was published.".to_owned())
        }
        (None, Kind::Channels) if !model.channels.is_empty() => {
            let updated = model.channels.iter().map(|c| c.updated).max().unwrap_or(0);
            (human_age(updated, now()), "since the channels your system is built from were updated.".to_owned())
        }
        _ => ("—".to_owned(), "Yukimi couldn't tell how old your system's Nixpkgs is.".to_owned()),
    };
    if let Some(check) = ctx.update_check() {
        let newer = updates::movements(model, &check).values().filter(|m| matches!(m, Movement::Newer(_))).count();
        line.push_str(&match newer {
            0 => " Everything was up to date when last checked.".to_owned(),
            1 => " One source has a newer version.".to_owned(),
            n => format!(" {n} sources have newer versions."),
        });
    }
    (big, line)
}

pub fn build(ctx: &Ctx) -> gtk::ScrolledWindow {
    let content = gtk::Box::new(gtk::Orientation::Vertical, 24);
    content.set_margin_top(24);
    content.set_margin_bottom(36);
    content.set_margin_start(18);
    content.set_margin_end(18);
    let scroll = page(&content);
    ctx.on_refresh("overview", move |ctx| {
        clear(&content);
        let model = ctx.model();
        content.append(&hero(ctx));

        for problem in &model.problems {
            let banner = adw::Banner::new(problem);
            banner.set_revealed(true);
            content.append(&banner);
        }

        let cards = gtk::Box::new(gtk::Orientation::Horizontal, 18);
        cards.set_homogeneous(true);
        // The cards share a height, but don't stretch to fill the window.
        cards.set_vexpand(false);
        let configured = &model.configured;
        let chosen = configured.yukimi.packages.len()
            + configured.yukimi.programs.len()
            + configured.system.len()
            + configured.settings.values().map(Vec::len).sum::<usize>();
        let mine = model.user_packages.len() + configured.user.len();
        let mut line = format!("{chosen} chosen for everyone and {mine} just for you");
        match &model.store {
            Some(store) => line.push_str(&format!(
                ", with {} more that your desktops and settings bring.",
                store.system_packages.len().saturating_sub(chosen)
            )),
            None => line.push('.'),
        }
        cards.append(&card(
            "view-grid-symbolic",
            "Packages",
            &(chosen + mine).to_string(),
            &line,
            ("See what's installed", "installed"),
            ctx,
        ));
        let (big, line) = freshness(ctx, &model);
        cards.append(&card(
            "software-update-available-symbolic",
            "Freshness",
            &big,
            &line,
            ("See updates", "updates"),
            ctx,
        ));
        let (big, line) = match &model.store {
            Some(store) => {
                let c = &store.composition;
                let line = if c.garbage + c.old_generations > 0 {
                    format!(
                        "in the store. {} is garbage, and older generations keep {}.",
                        human_size(c.garbage),
                        human_size(c.old_generations)
                    )
                } else {
                    "in the store.".to_owned()
                };
                (human_size(c.total), line)
            }
            None => ("…".to_owned(), "Reading the store…".to_owned()),
        };
        cards.append(&card("drive-harddisk-symbolic", "Storage", &big, &line, ("See storage", "storage"), ctx));
        content.append(&cards);

        // What the latest change brought.
        let n = model.generations.len();
        let group = adw::PreferencesGroup::new();
        group.set_title("Most recently");
        if n >= 2 {
            let (before, after) = (&model.generations[n - 2], &model.generations[n - 1]);
            group.set_description(Some(&format!(
                "Generation {} replaced generation {} {}.",
                after.number,
                before.number,
                human_age(after.created, now())
            )));
            let Some(d) = model.store.as_ref().and_then(|s| s.latest.as_ref()) else {
                group.add(&super::widgets::pending_row("Reading what it changed…"));
                content.append(&group);
                return;
            };
            let row = adw::ActionRow::new();
            let mut parts = Vec::new();
            for (kind, word) in [
                (ChangeKind::Upgraded, "upgraded"),
                (ChangeKind::Added, "added"),
                (ChangeKind::Removed, "removed"),
                (ChangeKind::Downgraded, "downgraded"),
            ] {
                let count = d.count(kind);
                if count > 0 {
                    parts.push(format!("{count} {word}"));
                }
            }
            row.set_title(&if parts.is_empty() { "Only settings changed".to_owned() } else { parts.join(", ") });
            let names: Vec<String> = d.changes.iter().take(6).map(|c| c.name.clone()).collect();
            if !names.is_empty() {
                row.set_subtitle(&format!("{}{}", names.join(", "), if d.changes.len() > 6 { "…" } else { "" }));
            }
            row.add_prefix(&gtk::Image::from_icon_name("document-open-recent-symbolic"));
            let go = gtk::Button::from_icon_name("go-next-symbolic");
            go.add_css_class("flat");
            go.set_valign(gtk::Align::Center);
            let ctx = ctx.clone();
            go.connect_clicked(move |_| ctx.show("history"));
            row.add_suffix(&go);
            row.set_activatable_widget(Some(&go));
            group.add(&row);
            content.append(&group);
        } else if let Some(only) = model.generations.last() {
            let row = adw::ActionRow::new();
            row.set_title("The system as it was installed");
            row.set_subtitle(&format!(
                "Generation {} was built {}. Each change you make becomes a new generation, and what it changed \
                 shows up here.",
                only.number,
                human_age(only.created, now())
            ));
            row.add_prefix(&gtk::Image::from_icon_name("document-open-recent-symbolic"));
            group.add(&row);
            content.append(&group);
        }
    });
    scroll
}

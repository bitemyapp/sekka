// SPDX-License-Identifier: MIT OR Apache-2.0
//! Where the system comes from: each input of its flake (or each of its
//! channels), the exact version it is at and how old that is, whether there
//! is a newer one, and updating.
use std::collections::BTreeMap;

use adw::prelude::*;
use yukimi_config::branch;
use yukimi_config::lock::Movement;
use yukimi_system::setup::Kind;
use yukimi_system::updates::{channel_movement, movement};
use yukimi_system::{human_age, now};

use super::widgets::{badge, clear, dim, heading, page};
use super::{Ctx, UpdateCheck};
use crate::model::Model;
use crate::ops::Operation;

/// How updating would move each source, as the last check found. Sources
/// pointed elsewhere since are left out.
pub fn movements(model: &Model, check: &UpdateCheck) -> BTreeMap<String, Movement> {
    let now = super::sources(model);
    let unchanged = |name: &str| check.sources.is_empty() || check.sources.get(name) == now.get(name);
    let mut found: BTreeMap<String, Movement> = match model.setup.kind {
        Kind::Flake => model
            .inputs
            .iter()
            .filter_map(|input| {
                let locked = input.locked.as_ref().filter(|_| input.follows.is_none())?;
                Some((input.name.clone(), movement(locked, check.newest.get(&input.name)?)))
            })
            .collect(),
        Kind::Channels => model
            .channels
            .iter()
            .filter_map(|channel| {
                Some((channel.name.clone(), channel_movement(channel, check.newest.get(&channel.name)?)))
            })
            .collect(),
    };
    found.retain(|name, _| unchanged(name));
    found
}

/// Build one system before deciding whether anything disruptive is necessary.
fn update(ctx: &Ctx, inputs: Vec<String>, _what: &str) {
    let model = ctx.model();
    let sekka = model.setup.kind == Kind::Flake
        && std::fs::read_to_string(model.setup.dir.join("flake.nix"))
            .is_ok_and(|text| yukimi_config::sekka::supported(&text));
    let migrating = !model.inputs.iter().any(|i| i.name == "sekka");
    ctx.run(if model.setup.kind == Kind::Channels {
        Operation::UpdateChannels
    } else if sekka && (inputs.iter().any(|i| i == "sekka") || migrating) {
        Operation::UpdateSekka {
            inputs: inputs
                .into_iter()
                .filter(|i| !migrating || !["calamares", "tatami", "yukimi"].contains(&i.as_str()))
                .collect(),
            hold: None,
        }
    } else {
        Operation::UpdateFlake { inputs }
    });
}

/// The buttons at the top: check, and update what has a newer version.
fn actions(ctx: &Ctx, movements: Option<&BTreeMap<String, Movement>>) -> gtk::Box {
    let buttons = gtk::Box::new(gtk::Orientation::Vertical, 8);
    buttons.set_valign(gtk::Align::Center);
    if ctx.checking() {
        let busy = gtk::Box::new(gtk::Orientation::Horizontal, 10);
        busy.set_halign(gtk::Align::Center);
        busy.append(&adw::Spinner::new());
        busy.append(&dim("Looking for newer versions…"));
        buttons.append(&busy);
        return buttons;
    }
    let mut newer: Vec<String> = movements
        .map(|m| m.iter().filter(|(_, m)| matches!(m, Movement::Newer(_))).map(|(name, _)| name.clone()).collect())
        .unwrap_or_default();
    let channels = ctx.model().setup.kind == Kind::Channels;
    // A single ordinary update includes Sekka and every newer selected source.
    // It also remains usable to retry preparation or migrate a legacy install.
    if !channels
        && std::fs::read_to_string(ctx.model().setup.dir.join("flake.nix"))
            .is_ok_and(|text| yukimi_config::sekka::supported(&text))
        && !newer.iter().any(|input| input == "sekka")
        && !movements.is_some_and(|m| matches!(m.get("sekka"), Some(Movement::Older(_))))
    {
        newer.push("sekka".into());
    }
    if !newer.is_empty() {
        let what = match (channels, newer.as_slice()) {
            (true, _) => "the channels".to_owned(),
            (false, [one]) => one.clone(),
            (false, many) => format!("{} sources", many.len()),
        };
        let all = gtk::Button::with_label("Update");
        all.add_css_class("pill");
        all.add_css_class("suggested-action");
        let sekka = !channels && newer.iter().any(|input| input == "sekka");
        {
            let (ctx, newer) = (ctx.clone(), newer.clone());
            all.connect_clicked(move |_| update(&ctx, newer.clone(), &what));
        }
        buttons.append(&all);
        if sekka {
            let choose = gtk::Button::with_label("Choose packages…");
            choose.add_css_class("pill");
            choose.set_tooltip_text(Some("See what the update changes, and keep some packages as they are"));
            let ctx = ctx.clone();
            choose.connect_clicked(move |_| choose_packages(&ctx, newer.clone()));
            buttons.append(&choose);
        }
    }
    let check = gtk::Button::with_label(if movements.is_some() { "Check again" } else { "Check for updates" });
    check.add_css_class("pill");
    if movements.is_none() {
        check.add_css_class("suggested-action");
    }
    let c = ctx.clone();
    check.connect_clicked(move |_| c.check_updates());
    buttons.append(&check);
    if let Some(checked) = ctx.update_check() {
        let when = dim(&format!("Checked {}", human_age(checked.when, now())));
        when.add_css_class("caption");
        buttons.append(&when);
    }
    buttons
}

/// What a package change looks like in the list.
fn change_text(change: &yukimi_system::selection::PackageChange) -> String {
    let (before, after) = (change.before.as_str(), change.after.as_str());
    match (change.held, before == after) {
        (true, true) => format!("Kept at {before}; the update builds the same version differently"),
        (true, false) => format!("Kept at {before}; the update has {after}"),
        (false, true) if before.is_empty() => "Built differently".to_owned(),
        (false, true) => format!("{before}, built differently"),
        (false, false) => format!("{before} → {after}"),
    }
}

/// Work out what updating changes, package by package (evaluating the
/// system before and after; nothing is built), and update with the
/// packages left unticked kept as they are.
fn choose_packages(ctx: &Ctx, inputs: Vec<String>) {
    use std::cell::RefCell;
    use std::rc::Rc;

    let content = gtk::Box::new(gtk::Orientation::Vertical, 18);
    content.set_margin_top(6);
    content.set_margin_bottom(24);
    content.set_margin_start(24);
    content.set_margin_end(24);
    content.append(&super::widgets::wrapping(
        "These are the packages the update changes. Untick one to keep the version you have now, exactly as it \
         is: nothing is downloaded or built for it, and it stays until you tick it in a later update. Libraries \
         and the rest of the system move with the update either way.",
        &["dim-label"],
    ));
    let listed = adw::PreferencesGroup::new();
    let waiting = super::widgets::pending_row("Working out what the update changes…");
    listed.add(&waiting);
    content.append(&listed);
    let footer = super::widgets::wrapping("", &["dim-label", "caption"]);
    footer.set_visible(false);
    content.append(&footer);
    let go = gtk::Button::with_label("Update");
    go.add_css_class("pill");
    go.add_css_class("suggested-action");
    go.set_halign(gtk::Align::Center);
    go.set_sensitive(false);
    content.append(&go);

    let view = adw::ToolbarView::new();
    view.add_top_bar(&adw::HeaderBar::new());
    let scroll = gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Never)
        .propagate_natural_height(true)
        .max_content_height(640)
        .child(&content)
        .build();
    view.set_content(Some(&scroll));
    let dialog = adw::Dialog::builder().title("Choose what to update").content_width(560).child(&view).build();

    let model = ctx.model();
    let (dir, flake, host, file) =
        (model.setup.dir.clone(), model.setup.flake(), model.setup.host.clone(), model.setup.packages_file());
    let scratch = crate::cache_dir().map(|dir| dir.join("preview"));
    let wanted = inputs.clone();
    let checks: Rc<RefCell<Vec<(String, gtk::CheckButton)>>> = Rc::default();
    let kept_quietly: Rc<RefCell<Vec<String>>> = Rc::default();
    {
        let (listed, go, footer, checks, kept_quietly) =
            (listed.clone(), go.clone(), footer.clone(), checks.clone(), kept_quietly.clone());
        gtk::glib::spawn_future_local(async move {
            let found = gtk::gio::spawn_blocking(move || {
                let scratch = scratch.ok_or("There is no cache directory to work in")?;
                yukimi_system::selection::preview(&dir, &flake, &host, file.as_deref(), &wanted, &scratch)
            })
            .await
            .unwrap_or_else(|_| Err("the comparison stopped unexpectedly".to_owned()));
            listed.remove(&waiting);
            let preview = match found {
                Ok(preview) => preview,
                Err(e) => {
                    let row = adw::ActionRow::new();
                    row.set_title("Yukimi couldn't work out what the update changes");
                    row.set_subtitle(&gtk::glib::markup_escape_text(&e));
                    listed.add(&row);
                    return;
                }
            };
            if preview.changes.is_empty() {
                let row = adw::ActionRow::new();
                row.set_title("None of your packages change");
                row.set_subtitle("The update changes only parts of the system that move with it.");
                listed.add(&row);
            }
            // New versions first; builds of the same version that changed
            // only because what they are built with did, folded away.
            let (rebuilt, newer): (Vec<_>, Vec<_>) = preview.changes.iter().partition(|c| c.before == c.after);
            let row_for = |change: &yukimi_system::selection::PackageChange| {
                let row = adw::ActionRow::new();
                row.set_title(&gtk::glib::markup_escape_text(&change.name));
                row.set_subtitle(&gtk::glib::markup_escape_text(&change_text(change)));
                let check = gtk::CheckButton::new();
                check.set_active(!change.held);
                check.set_valign(gtk::Align::Center);
                row.add_prefix(&check);
                row.set_activatable_widget(Some(&check));
                if change.held {
                    row.add_suffix(&badge("kept", "category"));
                }
                checks.borrow_mut().push((change.name.clone(), check));
                row
            };
            for change in newer {
                listed.add(&row_for(change));
            }
            if !rebuilt.is_empty() {
                let more = adw::ExpanderRow::new();
                more.set_title(&format!("{} more with the same version, built differently", rebuilt.len()));
                more.set_subtitle("What they are built with changed in this update");
                for change in rebuilt {
                    more.add_row(&row_for(change));
                }
                listed.add(&more);
            }
            // Packages kept before that this update doesn't change stay kept.
            let shown: std::collections::BTreeSet<&str> = preview.changes.iter().map(|c| c.name.as_str()).collect();
            *kept_quietly.borrow_mut() =
                preview.held.iter().filter(|name| !shown.contains(name.as_str())).cloned().collect();
            let mut notes = Vec::new();
            if !preview.added.is_empty() {
                notes.push(format!("adds {}", preview.added.join(", ")));
            }
            if !preview.removed.is_empty() {
                notes.push(format!("removes {}", preview.removed.join(", ")));
            }
            if !kept_quietly.borrow().is_empty() {
                notes.push(format!("keeps {} as you chose before", kept_quietly.borrow().join(", ")));
            }
            if !notes.is_empty() {
                footer.set_label(&format!("The update also {}.", notes.join("; ")));
                footer.set_visible(true);
            }
            go.set_sensitive(true);
        });
    }
    {
        let (ctx, dialog) = (ctx.clone(), dialog.clone());
        go.connect_clicked(move |_| {
            let mut hold: Vec<String> =
                checks.borrow().iter().filter(|(_, check)| !check.is_active()).map(|(name, _)| name.clone()).collect();
            hold.extend(kept_quietly.borrow().iter().cloned());
            dialog.close();
            ctx.run(Operation::UpdateSekka { inputs: inputs.clone(), hold: Some(hold) });
        });
    }
    dialog.present(Some(ctx.window()));
}

/// The time, badge and tooltip for one source, given how updating would
/// move it, or why it couldn't be checked.
fn status(row: &adw::ActionRow, source: &str, age: Option<String>, movement: Option<&Movement>, failure: Option<&str>) {
    let age = age.unwrap_or_default();
    if let Some(reason) = failure {
        row.add_suffix(&dim(&age));
        let mark = badge("couldn't check", "unchecked");
        mark.set_tooltip_text(Some(&format!("{source} couldn't be asked for its newest version: {reason}")));
        row.add_suffix(&mark);
        return;
    }
    match movement {
        Some(Movement::Newer(next)) => {
            let text = match next.last_modified {
                Some(t) if !age.is_empty() => format!("{age} → {}", human_age(t, now())),
                _ => age,
            };
            row.add_suffix(&dim(&text));
            let mark = badge("newer version", "newer");
            if let Some(rev) = next.short_rev() {
                mark.set_tooltip_text(Some(&format!("{source} is now at {rev}")));
            }
            row.add_suffix(&mark);
        }
        Some(Movement::Older(next)) => {
            row.add_suffix(&dim(&age));
            let mark = badge("would go back", "backwards");
            mark.set_tooltip_text(Some(&format!(
                "{source} now points to {}{}, older than the version you have. Updating would go back to it, so \
                 Yukimi doesn't.",
                next.short_rev().unwrap_or("a version"),
                next.last_modified.map(|t| format!(" from {}", human_age(t, now()))).unwrap_or_default(),
            )));
            row.add_suffix(&mark);
        }
        Some(Movement::Current) => {
            row.add_suffix(&dim(&age));
            row.add_suffix(&badge("up to date", "uptodate"));
        }
        None => row.add_suffix(&dim(&age)),
    }
}

/// Choose another branch for an input to follow: the repository's
/// branches (asked of GitHub in the background), or any branch typed in.
fn choose_branch(ctx: &Ctx, input: &str, repo: Option<(String, String)>, current: Option<String>) {
    let content = gtk::Box::new(gtk::Orientation::Vertical, 18);
    content.set_margin_top(6);
    content.set_margin_bottom(24);
    content.set_margin_start(24);
    content.set_margin_end(24);
    let following = current.as_deref().unwrap_or("its repository's default branch");
    content.append(&super::widgets::wrapping(
        &format!(
            "{input} follows {following}. Choose another branch, and Yukimi moves {input} to its newest commit; every \
             update after that comes from it. Push to that branch, then update here."
        ),
        &["dim-label"],
    ));
    let typed = adw::PreferencesGroup::new();
    let entry = adw::EntryRow::builder().title("Branch").show_apply_button(true).build();
    typed.add(&entry);
    content.append(&typed);
    let listed = adw::PreferencesGroup::new();
    content.append(&listed);

    let view = adw::ToolbarView::new();
    view.add_top_bar(&adw::HeaderBar::new());
    let scroll = gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Never)
        .propagate_natural_height(true)
        .max_content_height(560)
        .child(&content)
        .build();
    view.set_content(Some(&scroll));
    let dialog = adw::Dialog::builder().title(format!("Branch for {input}")).content_width(480).child(&view).build();

    let follow = {
        let (ctx, input, current, dialog) = (ctx.clone(), input.to_owned(), current.clone(), dialog.clone());
        std::rc::Rc::new(move |branch: String| {
            let branch = branch.trim().to_owned();
            if !yukimi_config::branch::valid(&branch) {
                ctx.toast(&format!("“{branch}” isn't a branch name Yukimi can use"));
                return;
            }
            dialog.close();
            if Some(&branch) == current.as_ref() {
                ctx.toast(&format!("{input} already follows {branch}"));
                return;
            }
            ctx.confirm(
                &format!("Follow {branch}?"),
                &format!(
                    "Yukimi points {input} at the newest commit of {branch} and builds your system with it, which asks \
                     for an administrator password. Updates of {input} come from {branch} from then on. The current \
                     version stays in History."
                ),
                "Follow",
                Operation::FollowBranch { input: input.clone(), branch },
            );
        })
    };
    {
        let follow = follow.clone();
        entry.connect_apply(move |entry| follow(entry.text().to_string()));
    }
    if let Some((owner, name)) = repo {
        listed.set_title(&format!("Branches of {owner}/{name}"));
        let waiting = super::widgets::pending_row("Asking GitHub for its branches…");
        listed.add(&waiting);
        gtk::glib::spawn_future_local(async move {
            let found = gtk::gio::spawn_blocking(move || yukimi_system::updates::github_branches(&owner, &name))
                .await
                .unwrap_or_else(|_| Err("the question stopped unexpectedly".to_owned()));
            listed.remove(&waiting);
            match found {
                Ok(branches) => {
                    for name in branches {
                        let row = adw::ActionRow::new();
                        row.set_title(&gtk::glib::markup_escape_text(&name));
                        if Some(&name) == current.as_ref() {
                            row.add_suffix(&badge("following", "current"));
                        }
                        row.set_activatable(true);
                        let follow = follow.clone();
                        row.connect_activated(move |_| follow(name.clone()));
                        listed.add(&row);
                    }
                }
                Err(e) => {
                    let row = adw::ActionRow::new();
                    row.set_title("GitHub couldn't list the branches; type one above");
                    row.set_subtitle(&gtk::glib::markup_escape_text(&e));
                    listed.add(&row);
                }
            }
        });
    }
    dialog.present(Some(ctx.window()));
    entry.grab_focus();
}

fn open_button(ctx: &Ctx, url: String, tooltip: &str) -> gtk::Button {
    let open = gtk::Button::from_icon_name("adw-external-link-symbolic");
    open.add_css_class("flat");
    open.set_valign(gtk::Align::Center);
    open.set_tooltip_text(Some(tooltip));
    let window = ctx.window().clone();
    open.connect_clicked(move |_| {
        gtk::UriLauncher::new(&url).launch(Some(&window), gtk::gio::Cancellable::NONE, |_| {});
    });
    open
}

/// The rows for a flake's inputs.
fn inputs(ctx: &Ctx, model: &Model, movements: Option<&BTreeMap<String, Movement>>, group: &adw::PreferencesGroup) {
    let check = ctx.update_check();
    let failures = check.as_ref().map(|c| c.failures.clone()).unwrap_or_default();
    if model.inputs.is_empty() {
        let row = adw::ActionRow::new();
        row.set_title(&format!("No flake.lock in {}", model.setup.dir.display()));
        row.set_subtitle("The flake has no inputs pinned yet; building it once makes its lock file.");
        group.add(&row);
    }
    for input in &model.inputs {
        let row = adw::ActionRow::new();
        row.set_title(&input.name);
        match (&input.locked, &input.follows) {
            (_, Some(path)) => {
                row.set_subtitle(&format!("Follows {}", path.join("/")));
                row.add_suffix(&badge("follows", "category"));
            }
            (Some(locked), None) => {
                let source = input.original.as_ref().unwrap_or(locked).describe();
                let mut subtitle = source.clone();
                if let Some(rev) = locked.short_rev() {
                    subtitle.push_str(&format!("  ·  {rev}"));
                }
                if !input.follows_inputs.is_empty() {
                    subtitle.push_str(&format!("  ·  uses this system's {}", input.follows_inputs.join(", ")));
                }
                row.set_subtitle(&subtitle);
                let movement = movements.and_then(|m| m.get(&input.name));
                status(
                    &row,
                    &source,
                    locked.last_modified.map(|t| human_age(t, now())),
                    movement,
                    failures.get(&input.name).map(String::as_str),
                );
                if branch::changeable(&source) {
                    let current = branch::of(&source);
                    let label = current.clone().unwrap_or_else(|| "default branch".to_owned());
                    let button = gtk::Button::with_label(&label);
                    button.add_css_class("flat");
                    button.set_valign(gtk::Align::Center);
                    button.set_tooltip_text(Some("Follow another branch"));
                    let repo =
                        (locked.kind == "github").then(|| locked.owner.clone().zip(locked.repo.clone())).flatten();
                    let (ctx, name) = (ctx.clone(), input.name.clone());
                    button.connect_clicked(move |_| choose_branch(&ctx, &name, repo.clone(), current.clone()));
                    row.add_suffix(&button);
                }
                if let Some(url) = locked.web_url() {
                    row.add_suffix(&open_button(ctx, url, "See this version"));
                }
                if matches!(movement, Some(Movement::Newer(_))) {
                    let one = gtk::Button::with_label("Update");
                    one.add_css_class("flat");
                    one.set_valign(gtk::Align::Center);
                    let (ctx, name) = (ctx.clone(), input.name.clone());
                    one.connect_clicked(move |_| update(&ctx, vec![name.clone()], &name));
                    row.add_suffix(&one);
                }
            }
            (None, None) => {}
        }
        row.add_prefix(&super::widgets::monogram(&input.name, 32));
        group.add(&row);
    }
}

/// The rows for root's channels.
fn channels(ctx: &Ctx, model: &Model, movements: Option<&BTreeMap<String, Movement>>, group: &adw::PreferencesGroup) {
    let check = ctx.update_check();
    let failures = check.as_ref().map(|c| c.failures.clone()).unwrap_or_default();
    if model.channels.is_empty() {
        let row = adw::ActionRow::new();
        row.set_title("No channels");
        row.set_subtitle(
            "This system isn't a flake and root has no channels, so its Nixpkgs is set in its configuration. \
             Update it there.",
        );
        group.add(&row);
    }
    for channel in &model.channels {
        let row = adw::ActionRow::new();
        row.set_title(&channel.name);
        let mut subtitle = channel.release.clone();
        if let Some(rev) = channel.short_revision() {
            subtitle.push_str(&format!("  ·  {rev}"));
        }
        row.set_subtitle(&subtitle);
        status(
            &row,
            &channel.release,
            Some(format!("updated {}", human_age(channel.updated, now()))),
            movements.and_then(|m| m.get(&channel.name)),
            failures.get(&channel.name).map(String::as_str),
        );
        if let Some(rev) = &channel.revision {
            row.add_suffix(&open_button(
                ctx,
                format!("https://github.com/NixOS/nixpkgs/commit/{rev}"),
                "See this version",
            ));
        }
        row.add_prefix(&super::widgets::monogram(&channel.name, 32));
        group.add(&row);
    }
}

pub fn build(ctx: &Ctx) -> gtk::ScrolledWindow {
    let content = gtk::Box::new(gtk::Orientation::Vertical, 24);
    content.set_margin_top(24);
    content.set_margin_bottom(36);
    content.set_margin_start(18);
    content.set_margin_end(18);
    let scroll = page(&content);
    ctx.on_refresh("updates", move |ctx| {
        clear(&content);
        let model = ctx.model();
        let check = ctx.update_check();
        let movements = check.as_ref().map(|check| movements(&model, check));
        let failures = check.map(|check| check.failures.clone()).unwrap_or_default();
        let top = gtk::Box::new(gtk::Orientation::Horizontal, 18);
        let intro = match model.setup.kind {
            Kind::Flake => heading(
                "Where your system comes from",
                "Your system is built from these sources, each pinned to one exact version, so it is the same \
                 every time it is built. Checking asks each source for its newest version without changing \
                 anything, and updating moves to it. The version you have now stays in History.",
            ),
            Kind::Channels => heading(
                "Where your system comes from",
                "Your system is built with Nixpkgs from these channels. Checking asks each channel for its \
                 newest version without changing anything, and updating fetches it and builds your system with \
                 it. The version you have now stays in History.",
            ),
        };
        intro.set_hexpand(true);
        top.append(&intro);
        if !model.inputs.is_empty() || !model.channels.is_empty() {
            top.append(&actions(ctx, movements.as_ref()));
        }
        content.append(&top);

        if !ctx.loading() {
            content.append(&super::completion::row(ctx, &model.completion));
        }

        if let Some(movements) = &movements {
            let older: Vec<&str> = movements
                .iter()
                .filter(|(_, m)| matches!(m, Movement::Older(_)))
                .map(|(name, _)| name.as_str())
                .collect();
            if !older.is_empty() {
                let banner = adw::Banner::new(&format!(
                    "Updating {} would go back to an older version, so Yukimi leaves {} as {} is",
                    older.join(" and "),
                    if older.len() == 1 { "it" } else { "them" },
                    if older.len() == 1 { "it" } else { "they" },
                ));
                banner.set_revealed(true);
                content.append(&banner);
            } else if failures.is_empty() && movements.values().all(|m| *m == Movement::Current) {
                let done = adw::StatusPage::builder()
                    .icon_name("emblem-ok-symbolic")
                    .title("No new updates available")
                    .description("Your sources are current. The update status above shows whether anything still needs to be applied.")
                    .build();
                done.add_css_class("compact");
                content.append(&done);
            }
        }

        let group = adw::PreferencesGroup::new();
        match model.setup.kind {
            Kind::Flake => inputs(ctx, &model, movements.as_ref(), &group),
            Kind::Channels => channels(ctx, &model, movements.as_ref(), &group),
        }
        content.append(&group);
    });
    scroll
}

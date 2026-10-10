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
use crate::ops::{Mode, Operation};

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

/// Ask, then update: now, or set up for the next restart (kind to a kernel
/// update, and to work in progress).
fn update(ctx: &Ctx, inputs: Vec<String>, what: &str) {
    let dialog = adw::AlertDialog::new(
        Some(&format!("Update {what}?")),
        Some(
            "Yukimi fetches the newest versions and builds the new system, which asks for an administrator \
             password and can take a while. It can take over now, or when the computer next starts. The current \
             version stays in History, so you can always return to it.",
        ),
    );
    dialog.add_responses(&[("cancel", "Cancel"), ("boot", "At next restart"), ("switch", "Now")]);
    dialog.set_response_appearance("switch", adw::ResponseAppearance::Suggested);
    dialog.set_default_response(Some("switch"));
    dialog.set_close_response("cancel");
    let (ctx2, channels) = (ctx.clone(), ctx.model().setup.kind == Kind::Channels);
    dialog.connect_response(None, move |_, response| {
        let mode = match response {
            "switch" => Mode::Switch,
            "boot" => Mode::Boot,
            _ => return,
        };
        ctx2.run(if channels {
            Operation::UpdateChannels { mode }
        } else {
            Operation::UpdateFlake { inputs: inputs.clone(), mode }
        });
    });
    dialog.present(Some(ctx.window()));
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
    let newer: Vec<String> = movements
        .map(|m| m.iter().filter(|(_, m)| matches!(m, Movement::Newer(_))).map(|(name, _)| name.clone()).collect())
        .unwrap_or_default();
    let channels = ctx.model().setup.kind == Kind::Channels;
    if !newer.is_empty() {
        let what = match (channels, newer.as_slice()) {
            (true, _) => "the channels".to_owned(),
            (false, [one]) => one.clone(),
            (false, many) => format!("{} sources", many.len()),
        };
        let all = gtk::Button::with_label(&format!("Update {what}"));
        all.add_css_class("pill");
        all.add_css_class("suggested-action");
        let ctx = ctx.clone();
        all.connect_clicked(move |_| update(&ctx, newer.clone(), &what));
        buttons.append(&all);
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
                    .title("Everything is up to date")
                    .description("Every source is at its newest version.")
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

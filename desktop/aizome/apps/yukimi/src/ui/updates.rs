//! Where the system comes from: each input of its flake (or each of its
//! channels), the exact version it is at and how old that is, whether there
//! is a newer one, and updating.
use std::collections::BTreeMap;

use aizome_kit::prelude::*;
use aizome_kit::{column, row};
use yukimi_config::branch;
use yukimi_config::lock::Movement;
use yukimi_system::completion::{Action, Completion};
use yukimi_system::human_age;
use yukimi_system::setup::Kind;

use super::widgets::{monogram, tap};
use super::{banner, caption, heading, mono, panel, tag};
use crate::app::{BranchChooser, Msg, Yukimi};
use crate::checks;

/// The toolbar's buttons: check, and update what has a newer version.
pub fn tools(app: &Yukimi) -> Vec<Element<Msg>> {
    let model = &app.model;
    if model.inputs.is_empty() && model.channels.is_empty() {
        return Vec::new();
    }
    if app.checking {
        return vec![
            progress_indeterminate().width(64.0).into(),
            text("Looking for newer versions…").size(12.0).role(Role::Text3).into(),
        ];
    }
    let moves = app.movements();
    let mut out: Vec<Element<Msg>> = Vec::new();
    let newer = checks::to_update(model, moves.as_ref(), app.sekka());
    let has_newer = moves.as_ref().is_some_and(|m| checks::newer_count(m) > 0);
    out.push(
        button(if moves.is_some() { "Check Again" } else { "Check for Updates" })
            .icon(Icon::RefreshCw)
            .on_press(Msg::CheckUpdates)
            .tooltip("Ask every source for its newest version, without changing anything (❖U)")
            .into(),
    );
    if !newer.is_empty() {
        let mut update = button("Update").icon(Icon::Download).on_press(Msg::Update(newer));
        if has_newer || moves.is_none() {
            update = update.primary();
        }
        out.push(update.tooltip("Update what moves forward, now or at the next restart").into());
    }
    out
}

/// The time, chip and tooltip for one source, given how updating would
/// move it, or why it couldn't be checked.
fn status(
    app: &Yukimi,
    source: &str,
    age: Option<String>,
    movement: Option<&Movement>,
    failure: Option<&str>,
) -> Vec<Element<Msg>> {
    let age = age.unwrap_or_default();
    let now = app.now();
    if let Some(reason) = failure {
        return vec![
            mono(age).role(Role::Text3).into(),
            tag("couldn't check", Role::Text3)
                .tooltip(format!("{source} couldn't be asked for its newest version: {reason}")),
        ];
    }
    match movement {
        Some(Movement::Newer(next)) => {
            // How old it is, and over it, how old the newer version is.
            let words: Element<Msg> = match next.last_modified {
                Some(t) if !age.is_empty() => {
                    column![mono(age).role(Role::Text3), mono(format!("→ {}", human_age(t, now))).role(Role::Accent),]
                        .spacing(1.0)
                        .align(Align::End)
                        .into()
                }
                _ => mono(age).into(),
            };
            let mut chip = tag("newer version", Role::Accent);
            if let Some(rev) = next.short_rev() {
                chip = chip.tooltip(format!("Update target for {source}: {rev}"));
            }
            vec![words, chip]
        }
        Some(Movement::Older(next)) => vec![
            mono(age).role(Role::Text3).into(),
            tag("would go back", Role::Warning).tooltip(format!(
                "{source} now points to {}{}, older than the version you have. Updating would go back to it, so Yukimi \
                 doesn't.",
                next.short_rev().unwrap_or("a version"),
                next.last_modified.map(|t| format!(" from {}", human_age(t, now))).unwrap_or_default(),
            )),
        ],
        Some(Movement::Current) => vec![mono(age).role(Role::Text3).into(), tag("up to date", Role::Success)],
        None => vec![mono(age).role(Role::Text3).into()],
    }
}

/// The rows for a flake's inputs.
fn inputs(app: &Yukimi, moves: Option<&BTreeMap<String, Movement>>) -> Vec<Element<Msg>> {
    let model = &app.model;
    let failures = app.check.as_ref().map(|c| c.failures.clone()).unwrap_or_default();
    let mut rows = Vec::new();
    if model.inputs.is_empty() {
        rows.push(super::item_row(
            None,
            text(format!("No flake.lock in {}", model.setup.dir.display())).weight(600.0).into(),
            Some(
                text("The flake has no inputs pinned yet; building it once makes its lock file.")
                    .size(12.0)
                    .role(Role::Text3)
                    .into(),
            ),
            Vec::new(),
        ));
    }
    for input in &model.inputs {
        let mut revisions = String::new();
        let mut title: Element<Msg> = text(input.name.clone()).weight(600.0).into();
        let (subtitle, trailing): (String, Vec<Element<Msg>>) = match (&input.locked, &input.follows) {
            (_, Some(path)) => (
                format!("Follows {}", path.join("/")),
                slots(vec![space(0.0), tag("follows", Role::Text3)], None, None, None),
            ),
            (Some(locked), None) => {
                let source = input.original.as_ref().unwrap_or(locked).describe();
                let mut subtitle = source.clone();
                if !input.follows_inputs.is_empty() {
                    subtitle.push_str(&format!("  ·  uses this system's {}", input.follows_inputs.join(", ")));
                }
                let movement = moves.and_then(|m| m.get(&input.name));
                revisions = model.update_history.revisions(input, movement);
                let status = status(
                    app,
                    &source,
                    locked.last_modified.map(|t| human_age(t, app.now())),
                    movement,
                    failures.get(&input.name).map(String::as_str),
                );
                let mut branch_button = None;
                if branch::changeable(&source) {
                    let label = branch::of(&source).unwrap_or_else(|| "default branch".to_owned());
                    branch_button = Some(
                        button(label)
                            .plain()
                            .compact()
                            .mono()
                            .icon(Icon::GitBranch)
                            .on_press(Msg::ChooseBranch(input.name.clone()))
                            .tooltip("Follow another branch")
                            .into(),
                    );
                }
                if let Some(link) = model.update_history.link(input, movement) {
                    title = row![
                        title,
                        button(link.label)
                            .compact()
                            .icon(if link.before.is_some() { Icon::GitCommitHorizontal } else { Icon::ExternalLink })
                            .plain()
                            .on_press(if link.before.is_some() {
                                Msg::InspectChanges(input.name.clone())
                            } else {
                                Msg::OpenUrl(link.url)
                            })
                            .tooltip(link.tooltip),
                    ]
                    .spacing(8.0)
                    .into();
                }
                let update = matches!(movement, Some(Movement::Newer(_)))
                    .then(|| button("Update").compact().on_press(Msg::Update(vec![input.name.clone()])).into());
                (subtitle, slots(status, branch_button, None, update))
            }
            (None, None) => (String::new(), Vec::new()),
        };
        rows.push(super::item_row(
            Some(monogram(&input.name, 28.0)),
            title,
            (!subtitle.is_empty()).then(|| {
                let mut lines = column![text(subtitle.clone()).mono().size(11.5).role(Role::Text3).tooltip(subtitle)]
                    .spacing(2.0)
                    .width(Fill);
                if !revisions.is_empty() {
                    lines = lines.push(text(revisions).mono().size(11.5).role(Role::Text2).wrap());
                }
                lines.into()
            }),
            trailing,
        ));
    }
    rows
}

/// The rows for root's channels.
fn channels(app: &Yukimi, moves: Option<&BTreeMap<String, Movement>>) -> Vec<Element<Msg>> {
    let model = &app.model;
    let failures = app.check.as_ref().map(|c| c.failures.clone()).unwrap_or_default();
    let mut rows = Vec::new();
    if model.channels.is_empty() {
        rows.push(super::item_row(
            None,
            text("No channels").weight(600.0).into(),
            Some(
                text(
                    "This system isn't a flake and root has no channels, so its Nixpkgs is set in its configuration. \
                     Update it there.",
                )
                .wrap()
                .size(12.0)
                .role(Role::Text3)
                .into(),
            ),
            Vec::new(),
        ));
    }
    for channel in &model.channels {
        let mut subtitle = channel.release.clone();
        if let Some(rev) = channel.short_revision() {
            subtitle.push_str(&format!("  ·  {rev}"));
        }
        let status = status(
            app,
            &channel.release,
            Some(format!("updated {}", human_age(channel.updated, app.now()))),
            moves.and_then(|m| m.get(&channel.name)),
            failures.get(&channel.name).map(String::as_str),
        );
        let link = channel.revision.as_ref().map(|rev| {
            icon_button(Icon::ExternalLink)
                .plain()
                .on_press(Msg::OpenUrl(format!("https://github.com/NixOS/nixpkgs/commit/{rev}")))
                .tooltip("See this version")
                .into()
        });
        let trailing = slots(status, None, link, None);
        rows.push(super::item_row(
            Some(monogram(&channel.name, 28.0)),
            text(channel.name.clone()).weight(600.0).into(),
            Some(text(subtitle).mono().size(11.5).role(Role::Text3).into()),
            trailing,
        ));
    }
    rows
}

/// Whether the running system is the one installed, or a restart (or a new
/// session) would finish an update.
pub fn completion(c: &Completion) -> Element<Msg> {
    let (icon_name, role) = match c.action {
        Action::None => (Icon::CircleCheck, Role::Success),
        Action::Reload | Action::Reboot | Action::Logout => (Icon::RotateCw, Role::Warning),
        Action::Unknown => (Icon::TriangleAlert, Role::Warning),
    };
    let mut r = row![
        icon(icon_name).size(20.0).role(role),
        column![text(c.title()).weight(650.0), text(c.description()).wrap().size(12.0).role(Role::Text2)]
            .spacing(3.0)
            .width(Fill),
    ]
    .spacing(12.0)
    .width(Fill);
    match c.action {
        Action::Reboot => {
            r = r.push(
                button("Restart Now")
                    .primary()
                    .on_press(Msg::Restart)
                    .tooltip("Save your work first. You can also leave this until later."),
            )
        }
        Action::Logout => {
            r = r.push(
                button("Sign Out")
                    .primary()
                    .on_press(Msg::SignOut)
                    .tooltip("Save your work first. You can also leave this until later."),
            )
        }
        _ => {}
    }
    panel(r).width(Fill).into()
}

pub fn page(app: &Yukimi) -> Element<Msg> {
    let model = &app.model;
    let moves = app.movements();
    let failures = app.check.as_ref().map(|c| c.failures.len()).unwrap_or(0);
    let intro = match model.setup.kind {
        Kind::Flake => heading(
            "Where your system comes from",
            "Your system is built from these sources, each pinned to one exact version, so it is the same every time \
             it is built. Checking asks each source for its newest version without changing anything, and updating \
             moves to it. The version you have now stays in History.",
        ),
        Kind::Channels => heading(
            "Where your system comes from",
            "Your system is built with Nixpkgs from these channels. Checking asks each channel for its newest version \
             without changing anything, and updating fetches it and builds your system with it. The version you have \
             now stays in History.",
        ),
    };
    let mut c = column![intro].spacing(18.0).width(Fill);
    if !app.loading {
        c = c.push(completion(&model.completion));
        if app.sekka() && model.setup.kind == Kind::Flake {
            c = c.push(early(app));
        }
    }
    if let Some(moves) = &moves {
        let older = checks::older(moves);
        if !older.is_empty() {
            let (it, is) = if older.len() == 1 { ("it", "it is") } else { ("them", "they are") };
            c = c.push(banner(
                Icon::TriangleAlert,
                Role::Warning,
                format!(
                    "Updating {} would go back to an older version, so Yukimi leaves {it} as {is}.",
                    older.join(" and ")
                ),
            ));
        } else if failures == 0 && moves.values().all(|m| *m == Movement::Current) {
            c = c.push(banner(
                Icon::CircleCheck,
                Role::Success,
                "No new updates available. Your sources are current; the status above shows whether anything still \
                 needs to be applied."
                    .to_owned(),
            ));
        }
    }
    let rows = match model.setup.kind {
        Kind::Flake => inputs(app, moves.as_ref()),
        Kind::Channels => channels(app, moves.as_ref()),
    };
    let (title, whence) = match model.setup.kind {
        Kind::Flake => (
            "Sources",
            format!(
                "Running and prepared revisions come from saved generations. Pinned is from {}. Target is from \
                 the last update check. Last update compares the last saved change to each source.",
                model.setup.dir.join("flake.lock").display()
            ),
        ),
        Kind::Channels => ("Channels", "root's channels, which the system is built with.".to_owned()),
    };
    c.push(
        column![caption(title), text(whence).wrap().size(12.0).role(Role::Text3), super::list(rows)]
            .spacing(6.0)
            .width(Fill),
    )
    .into()
}

/// Early updates: Sekka from its main branch, as soon as changes are
/// merged, or from the nightly once its checks pass.
fn early(app: &Yukimi) -> Element<Msg> {
    let on = app.model.early();
    let words = if on {
        "Sekka's changes reach this computer as soon as they are merged, before the nightly checks run. Whatever \
         its binary cache doesn't have yet is built here."
    } else {
        "Get Sekka's changes as soon as they are merged, before the nightly checks run, to try what is new. \
         Whatever its binary cache doesn't have yet is built here, which takes longer."
    };
    let r = row![
        icon(Icon::Zap).size(20.0).role(if on { Role::Accent } else { Role::Text3 }),
        column![text("Early updates").weight(650.0), text(words).wrap().size(12.0).role(Role::Text2)]
            .spacing(3.0)
            .width(Fill),
        toggle(on).on_toggle(Msg::Early).id("early").tooltip(if on {
            "Go back to updating from the nightly"
        } else {
            "Update from Sekka's main branch"
        }),
    ]
    .spacing(12.0)
    .width(Fill);
    panel(r).width(Fill).into()
}

/// Choosing another branch for an input to follow: the repository's
/// branches (asked of GitHub in the background), or any branch typed in.
pub fn branch_sheet(_app: &Yukimi, chooser: &BranchChooser) -> Element<Msg> {
    let input = &chooser.input;
    let following = chooser.current.as_deref().unwrap_or("its repository's default branch");
    let mut c = column![
        text(format!("Branch for {input}")).size(17.0).weight(700.0),
        text(format!(
            "{input} follows {following}. Choose another branch, and Yukimi moves {input} to its newest commit; every \
             update after that comes from it. Push to that branch, then update here."
        ))
        .wrap()
        .role(Role::Text2),
        row![
            text_field(&chooser.typed)
                .placeholder("A branch's name")
                .mono()
                .on_change(Msg::BranchTyped)
                .on_submit(Msg::Follow(chooser.typed.clone()))
                .id("branch")
                .width(Fill),
            button("Follow")
                .primary()
                .default()
                .on_press_maybe((!chooser.typed.trim().is_empty()).then(|| Msg::Follow(chooser.typed.clone()))),
        ]
        .spacing(8.0)
        .width(Fill),
    ]
    .spacing(12.0)
    .width(Fill);
    if let Some((owner, name)) = &chooser.repo {
        let rows: Vec<Element<Msg>> = match &chooser.branches {
            None => vec![super::pending("Asking GitHub for its branches…")],
            Some(Err(e)) => vec![super::item_row(
                None,
                text("GitHub couldn't list the branches; type one above").weight(600.0).into(),
                Some(text(e.clone()).size(12.0).role(Role::Text3).wrap().into()),
                Vec::new(),
            )],
            Some(Ok(branches)) => branches
                .iter()
                .take(100)
                .map(|b| {
                    let mut r =
                        row![icon(Icon::GitBranch).size(14.0).role(Role::Text3), text(b.clone()).mono().size(12.5)]
                            .spacing(10.0)
                            .width(Fill)
                            .height(22.0);
                    if Some(b) == chooser.current.as_ref() {
                        r = r.push(spacer()).push(tag("following", Role::Success));
                    }
                    tap(r).padding([5.0, 12.0]).on_press(Msg::Follow(b.clone())).named(b.clone()).into()
                })
                .collect(),
        };
        c = c.push(
            column![caption(&format!("Branches of {owner}/{name}")), super::list(rows).max_height(300.0)]
                .spacing(6.0)
                .width(Fill),
        );
    }
    c = c.push(row![spacer(), button("Cancel").cancel().on_press(Msg::CloseSheet(chooser.sheet))].width(Fill));
    container(c).padding(20.0).width(520.0).into()
}

/// A source's row's right side, in columns that line up from row to row:
/// its age, how it would move, the branch it follows, a link to see it,
/// and Update.
fn slots(
    status: Vec<Element<Msg>>,
    branch: Option<Element<Msg>>,
    link: Option<Element<Msg>>,
    update: Option<Element<Msg>>,
) -> Vec<Element<Msg>> {
    let mut status = status.into_iter();
    let age = status.next().unwrap_or_else(|| space(0.0));
    let chip = status.next().unwrap_or_else(|| space(0.0));
    let slot = |e: Option<Element<Msg>>, w: f64, align: Align| -> Element<Msg> {
        container(e.unwrap_or_else(|| space(0.0))).width(w).align_x(align).into()
    };
    let mut slots = vec![
        slot(Some(age), 140.0, Align::End),
        slot(Some(chip), 100.0, Align::Start),
        slot(branch, 110.0, Align::Start),
    ];
    if link.is_some() {
        slots.push(slot(link, 28.0, Align::Center));
    }
    slots.push(slot(update, 64.0, Align::End));
    slots
}

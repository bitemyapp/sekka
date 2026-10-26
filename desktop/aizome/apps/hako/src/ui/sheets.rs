//! Sheets over a window — Get Info, Quick Look, a job's question, a
//! confirmation, a message — and the operations window.

use aizome_kit::prelude::*;
use aizome_kit::{column, row};

use super::app::{Confirm, Hako, Msg, Sheet};
use super::jobs::{Answer, Question};
use super::look;
use crate::fsops::{Conflict, ErrorChoice, Resolution};
use crate::model::entry::{Entry, Kind};
use crate::model::format;

/// A sheet's view.
pub fn view(h: &Hako, key: WindowKey, s: &Sheet) -> Element<Msg> {
    match s {
        Sheet::Info { entry, .. } => info(h, key, entry),
        Sheet::QuickLook { entry, .. } => quick_look(h, key, entry),
        Sheet::Ask { job, all } => ask(h, *job, *all),
        Sheet::Confirm { what, .. } => confirm(h, key, what),
        Sheet::Message { title, body } => message(key, title, body),
        Sheet::Unlock { name, pass, error, busy, .. } => unlock(key, name, pass, error, *busy),
    }
}

fn unlock(key: WindowKey, name: &str, pass: &str, error: &str, busy: bool) -> Element<Msg> {
    let mut content = vec![
        title(format!("Unlock {name}")),
        text("Enter this volume's passphrase to unlock it and open its files.").wrap().width(Length::Fill).into(),
        text_field(pass)
            .password()
            .id("unlock-pass")
            .placeholder("Passphrase")
            .enabled(!busy)
            .on_change(move |p| Msg::UnlockPass(key, p))
            .on_submit(Msg::UnlockSubmit(key))
            .width(Length::Fill)
            .into(),
    ];
    if !error.is_empty() {
        content.push(text(error.to_owned()).role(Role::Danger).wrap().width(Length::Fill).into());
    }
    content.push(
        row![
            spacer(),
            button("Cancel").cancel().enabled(!busy).on_press(Msg::CloseSheet(key)),
            button(if busy { "Unlocking…" } else { "Unlock" })
                .default()
                .enabled(!busy && !pass.is_empty())
                .on_press(Msg::UnlockSubmit(key)),
        ]
        .spacing(8.0)
        .width(Length::Fill)
        .into(),
    );
    frame(aizome_kit::column(content).spacing(14.0).width(Length::Fill).into(), 440.0)
}

fn frame(content: Element<Msg>, width: f64) -> Element<Msg> {
    container(content).padding(18.0).width(width).into()
}

fn title(s: impl Into<String>) -> Element<Msg> {
    text(s.into()).semibold().size(14.0).wrap().width(Length::Fill).into()
}

fn row_of(label: &str, value: impl Into<String>) -> Element<Msg> {
    row![
        text(label.to_owned()).size(12.0).role(Role::Text3).width(96.0),
        text(value.into()).size(12.0).selectable().wrap().width(Length::Fill),
    ]
    .spacing(8.0)
    .align(Align::Start)
    .width(Length::Fill)
    .into()
}

fn big_icon(h: &Hako, e: &Entry, size: f64) -> Element<Msg> {
    match h.thumbs.images.get(&e.path).cloned() {
        Some(img) => image(img).fit(Fit::Contain).width(size).height(size).into(),
        None => {
            let (ic, role) = look::entry_icon(e);
            icon(ic).size(size).role(role).into()
        }
    }
}

/// Get Info: what it is, where, how big, when, and who may do what.
fn info(h: &Hako, key: WindowKey, e: &Entry) -> Element<Msg> {
    let now = h.now();
    let size = if e.is_folder() {
        match h.sizes.get(&e.path) {
            Some((n, b)) => format!("{}, in {}", format::size_long(*b), format::items(*n as usize)),
            None => "Counting…".into(),
        }
    } else {
        format::size_long(e.size)
    };
    let where_ = match &e.trashed {
        Some(t) => format!(
            "In the Trash, from {}",
            t.original
                .as_ref()
                .and_then(|o| o.parent())
                .map(|p| p.display().to_string())
                .unwrap_or_else(|| "somewhere unknown".into())
        ),
        None => e.path.parent().map(|p| p.display().to_string()).unwrap_or_default(),
    };
    let mut rows: Vec<Element<Msg>> =
        vec![row_of("Kind", e.kind_name.to_string()), row_of("Size", size), row_of("Where", where_)];
    if let Some(t) = &e.link_target {
        rows.push(row_of("Link to", t.display().to_string()));
    }
    if let Some(c) = e.created {
        rows.push(row_of("Created", format::date_long(c)));
    }
    if let Some(m) = e.modified {
        rows.push(row_of("Modified", format::date_long(m)));
    }
    if let Some(a) = e.accessed {
        rows.push(row_of("Opened", format::date(a, now)));
    }
    if !e.is_folder() && e.kind != Kind::Special {
        rows.push(row_of("Type", e.mime.to_string()));
    }
    if let Some(apps) = &h.apps
        && !e.is_folder()
        && let Some(id) = apps.default_for(&e.mime)
        && let Some(app) = apps.get(&id)
    {
        rows.push(row_of("Opens with", app.name.clone()));
    }
    let m = e.mode;
    let perms: Vec<Element<Msg>> = vec![
        row_of(&format::user_name(e.uid), format!("{} (owner)", format::access(m & 0o400 != 0, m & 0o200 != 0))),
        row_of(&format::group_name(e.gid), format!("{} (group)", format::access(m & 0o040 != 0, m & 0o020 != 0))),
        row_of("Everyone", format::access(m & 0o004 != 0, m & 0o002 != 0)),
        row_of("Mode", format!("{} ({:04o})", format::permissions(m), m & 0o7777)),
    ];
    let body = column![
        row![
            big_icon(h, e, 48.0),
            column![title(e.display.clone()), text(e.kind_name.to_string()).size(12.0).role(Role::Text3)]
                .spacing(2.0)
                .width(Length::Fill),
        ]
        .spacing(12.0)
        .width(Length::Fill),
        divider(),
        aizome_kit::column(rows).spacing(6.0).width(Length::Fill),
        text("Sharing & Permissions").mono().size(10.5).letter_spacing(1.2).caps().role(Role::Text3),
        aizome_kit::column(perms).spacing(6.0).width(Length::Fill),
        row![spacer(), button("Done").default().cancel().id("sheet-default").on_press(Msg::CloseSheet(key))]
            .width(Length::Fill),
    ]
    .spacing(10.0)
    .width(Length::Fill);
    frame(body.into(), 440.0)
}

/// Quick Look: the item large.
fn quick_look(h: &Hako, key: WindowKey, e: &Entry) -> Element<Msg> {
    let p = h.previews.get(&e.path);
    let shown: Element<Msg> = if e.is_folder() {
        let what = match h.sizes.get(&e.path) {
            Some((n, b)) => format!("{} — {}", format::items(*n as usize), format::size(*b)),
            None => "Counting…".into(),
        };
        column![big_icon(h, e, 128.0), text(what).size(13.0).role(Role::Text2)]
            .spacing(12.0)
            .align(Align::Center)
            .into()
    } else if let Some(img) = p.and_then(|p| p.image.clone()).or_else(|| h.thumbs.images.get(&e.path).cloned()) {
        image(img).fit(Fit::Contain).width(Length::Fill).height(Length::Fill).into()
    } else if let Some(t) = p.and_then(|p| p.text.clone()) {
        scroll(container(text(t).mono().size(12.0).wrap().selectable()).padding(12.0).width(Length::Fill)).into()
    } else {
        column![
            big_icon(h, e, 128.0),
            text(format!("{} — {}", e.kind_name, format::size(e.size))).size(13.0).role(Role::Text2)
        ]
        .spacing(12.0)
        .align(Align::Center)
        .into()
    };
    let body = column![
        row![
            text(e.display.clone()).semibold().size(13.0).width(Length::Fill),
            button("Close").compact().default().cancel().id("sheet-default").on_press(Msg::CloseSheet(key)),
        ]
        .spacing(8.0)
        .width(Length::Fill),
        container(shown).center().fill(Tone::Sunk).border(Edge::Line).width(Length::Fill).height(420.0).clip(),
    ]
    .spacing(10.0)
    .width(Length::Fill);
    // The arrows move through the items shown (the sheet has the keyboard).
    super::keys::catcher(frame(body.into(), 620.0), move |k| super::view::quick_look_key(key, k))
}

fn when(t: Option<std::time::SystemTime>, now: i64) -> String {
    t.and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| format::date(d.as_secs() as i64, now))
        .unwrap_or_else(|| "—".into())
}

fn side(label: &str, is_dir: bool, size: u64, modified: Option<std::time::SystemTime>, now: i64) -> Element<Msg> {
    let what = if is_dir { "Folder".to_owned() } else { format::size(size) };
    column![
        text(label.to_owned()).size(11.0).role(Role::Text3).caps().letter_spacing(0.8),
        text(format!("{what}, modified {}", when(modified, now))).size(12.0).role(Role::Text2),
    ]
    .spacing(2.0)
    .width(Length::Fill)
    .into()
}

/// A job's question: a conflict, or an error.
fn ask(h: &Hako, job: u64, all: bool) -> Element<Msg> {
    let Some(j) = h.jobs.iter().find(|j| j.id == job) else { return text("").into() };
    let Some(q) = &j.question else { return text("").into() };
    let name = |p: &std::path::Path| {
        p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| p.display().to_string())
    };
    let body: Element<Msg> = match q {
        Question::Conflict(c) => conflict(job, c, all, &name(&c.target), &j.title, h.now()),
        Question::Failure(f) => {
            let mut buttons: Vec<Element<Msg>> = vec![
                checkbox("Do the same for the rest", all).on_toggle(move |on| Msg::AskAll(job, on)).into(),
                spacer().into(),
                button("Stop").cancel().on_press(Msg::Answer(job, Answer::Error(ErrorChoice::Cancel, false))).into(),
                button("Skip")
                    .id(if f.can_retry { "sheet-skip" } else { "sheet-default" })
                    .on_press(Msg::Answer(job, Answer::Error(ErrorChoice::Skip, false)))
                    .into(),
            ];
            if f.can_retry {
                buttons.push(
                    button("Try Again")
                        .default()
                        .id("sheet-default")
                        .on_press(Msg::Answer(job, Answer::Error(ErrorChoice::Retry, false)))
                        .into(),
                );
            }
            column![
                row![
                    icon(Icon::TriangleAlert).size(32.0).role(Role::Warning),
                    column![
                        title(format!("“{}” couldn't {}.", name(&f.path), action_words(f.action))),
                        text(format!("{}.", f.message)).size(12.0).role(Role::Text2).wrap().width(Length::Fill),
                        text(j.title.clone()).size(11.0).role(Role::Text3),
                    ]
                    .spacing(4.0)
                    .width(Length::Fill)
                ]
                .spacing(12.0)
                .align(Align::Start)
                .width(Length::Fill),
                aizome_kit::row(buttons).spacing(8.0).width(Length::Fill),
            ]
            .spacing(16.0)
            .width(Length::Fill)
            .into()
        }
    };
    frame(body, 480.0)
}

fn action_words(action: &str) -> String {
    match action {
        "read" => "be read".into(),
        "write" => "be written".into(),
        "copy" => "be copied".into(),
        "move" => "be moved".into(),
        "replace" => "be replaced".into(),
        "find" => "be found".into(),
        "make the folder" => "be made".into(),
        "read the folder" => "be read".into(),
        "put in place" => "be put in place".into(),
        "copy the link" => "be copied".into(),
        "move to the Trash" => "go to the Trash".into(),
        other => format!("be done ({other})"),
    }
}

fn conflict(job: u64, c: &Conflict, all: bool, name: &str, doing: &str, now: i64) -> Element<Msg> {
    let folder = c
        .target
        .parent()
        .and_then(|p| p.file_name())
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "/".into());
    let what = if c.target_is_dir { "A folder" } else { "An item" };
    let answer = move |r: Resolution| Msg::Answer(job, Answer::Conflict(r, false));
    let mut buttons: Vec<Element<Msg>> = vec![
        checkbox("Apply to all", all).on_toggle(move |on| Msg::AskAll(job, on)).into(),
        spacer().into(),
        button("Stop").cancel().on_press(answer(Resolution::Cancel)).into(),
        button("Skip").on_press(answer(Resolution::Skip)).into(),
        button("Keep Both").default().id("sheet-default").on_press(answer(Resolution::KeepBoth)).into(),
    ];
    if c.can_merge() {
        buttons.push(button("Merge").on_press(answer(Resolution::Merge)).into());
    }
    buttons.push(button("Replace").destructive().on_press(answer(Resolution::Replace)).into());
    column![
        row![
            icon(Icon::Copy).size(32.0).role(Role::Accent),
            column![
                title(format!("{what} named “{name}” is already in “{folder}”.")),
                text(if c.can_merge() {
                    "Merging puts the new folder's items into it; replacing moves it to the Trash first.".to_owned()
                } else {
                    "Replacing moves the one there to the Trash.".to_owned()
                })
                .size(12.0)
                .role(Role::Text2)
                .wrap()
                .width(Length::Fill),
                text(doing.to_owned()).size(11.0).role(Role::Text3),
            ]
            .spacing(4.0)
            .width(Length::Fill),
        ]
        .spacing(12.0)
        .align(Align::Start)
        .width(Length::Fill),
        row![
            side("Coming", c.source_is_dir, c.source_size, c.source_modified, now),
            side("There now", c.target_is_dir, c.target_size, c.target_modified, now)
        ]
        .spacing(16.0)
        .width(Length::Fill),
        aizome_kit::row(buttons).spacing(6.0).width(Length::Fill),
    ]
    .spacing(16.0)
    .width(Length::Fill)
    .into()
}

fn confirm(h: &Hako, key: WindowKey, what: &Confirm) -> Element<Msg> {
    let (head, body, action) = match what {
        Confirm::Erase(es) => (
            match es.as_slice() {
                [one] => format!("Delete “{}” for good?", one.display_name()),
                _ => format!("Delete these {} items for good?", es.len()),
            },
            "They'll be gone at once. This can't be undone.".to_owned(),
            "Delete",
        ),
        Confirm::EmptyTrash => (
            "Empty the Trash?".to_owned(),
            format!("Its {} will be deleted for good. This can't be undone.", format::items(h.trash_count)),
            "Empty Trash",
        ),
    };
    let content = column![
        row![
            icon(Icon::TriangleAlert).size(32.0).role(Role::Danger),
            column![title(head), text(body).size(12.0).role(Role::Text2).wrap().width(Length::Fill)]
                .spacing(4.0)
                .width(Length::Fill),
        ]
        .spacing(12.0)
        .align(Align::Start)
        .width(Length::Fill),
        row![
            spacer(),
            button("Cancel").cancel().default().id("sheet-default").on_press(Msg::CloseSheet(key)),
            button(action).destructive().on_press(Msg::Confirmed(key)),
        ]
        .spacing(8.0)
        .width(Length::Fill),
    ]
    .spacing(16.0)
    .width(Length::Fill);
    frame(content.into(), 420.0)
}

fn message(key: WindowKey, head: &str, body: &str) -> Element<Msg> {
    let content = column![
        row![
            icon(Icon::CircleAlert).size(32.0).role(Role::Warning),
            column![
                title(head.to_owned()),
                text(body.to_owned()).size(12.0).role(Role::Text2).wrap().selectable().width(Length::Fill)
            ]
            .spacing(6.0)
            .width(Length::Fill),
        ]
        .spacing(12.0)
        .align(Align::Start)
        .width(Length::Fill),
        row![spacer(), button("OK").default().cancel().id("sheet-default").on_press(Msg::CloseSheet(key))]
            .width(Length::Fill),
    ]
    .spacing(16.0)
    .width(Length::Fill);
    frame(content.into(), 440.0)
}

/// The operations window: each job, its progress, pause and cancel.
pub fn operations(h: &Hako) -> Element<Msg> {
    let mut rows: Vec<Element<Msg>> = Vec::new();
    for j in &h.jobs {
        let id = j.id;
        let bar: Element<Msg> = if j.progress.preparing || (j.progress.bytes_total == 0 && j.progress.items_total == 0)
        {
            progress_indeterminate().width(Length::Fill).into()
        } else {
            progress(j.progress.fraction()).width(Length::Fill).into()
        };
        let current = j
            .progress
            .current
            .as_ref()
            .and_then(|p| p.file_name())
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        rows.push(
            row![
                column![
                    text(j.title.clone()).semibold().size(12.5),
                    bar,
                    row![
                        text(j.status()).size(11.0).role(Role::Text3).mono().tabular(),
                        spacer(),
                        text(current).size(11.0).role(Role::Text3).max_lines(1),
                    ]
                    .spacing(8.0)
                    .width(Length::Fill),
                ]
                .spacing(4.0)
                .width(Length::Fill),
                icon_button(if j.paused() { Icon::Play } else { Icon::Pause })
                    .plain()
                    .on_press(Msg::PauseJob(id))
                    .tooltip(if j.paused() { "Resume" } else { "Pause" })
                    .label(if j.paused() { "Resume" } else { "Pause" }),
                icon_button(Icon::CircleX).plain().on_press(Msg::CancelJob(id)).tooltip("Stop").label("Stop"),
            ]
            .spacing(8.0)
            .width(Length::Fill)
            .into(),
        );
    }
    if rows.is_empty() {
        rows.push(text("Nothing under way.").role(Role::Text3).into());
    }
    container(aizome_kit::column(rows).spacing(14.0).width(Length::Fill))
        .padding(16.0)
        .width(Length::Fill)
        .height(Length::Fill)
        .fill(Tone::Surface)
        .into()
}

//! The Suggestions panel: what could be freed, by kind, each with its
//! risk, what it is in plain words, its paths, and its action.

use std::path::Path;

use aizome_kit::prelude::*;
use aizome_kit::{column, mouse_area, row};

use super::suggest::{HintAction, action_of, sections};
use super::{IndexState, Kura, Msg, Side, hint_key};
use crate::format;
use crate::hints::{Hint, Risk};

/// The panel's width (wider than the folder's list: paths and copies).
pub const WIDTH: f64 = 460.0;

/// Paths a hint lists in the window at most (the index has them all).
const SHOWN: usize = 40;

/// The toolbar's way in: what could be freed.
pub fn toolbar_button(k: &Kura) -> Element<Msg> {
    let r = k.analysis.reclaimable;
    let label = if r > 0 {
        format!("{} to free", format::size(r))
    } else if k.index == IndexState::Writing {
        "Looking…".to_owned()
    } else {
        "Suggestions".to_owned()
    };
    let to = if k.side == Side::Suggestions { Side::Contents } else { Side::Suggestions };
    button(label)
        .toolbar()
        .icon(Icon::ListChecks)
        .selected(k.side == Side::Suggestions)
        .on_press(Msg::ShowSide(to))
        .tooltip("What could be freed, and how")
        .label("Suggestions")
        .into()
}

fn caption(s: &str) -> Element<Msg> {
    text(s.to_owned()).mono().size(10.5).letter_spacing(1.2).caps().role(Role::Text3).into()
}

fn risk_badge(r: Risk) -> Element<Msg> {
    let role = match r {
        Risk::Safe => Role::Success,
        Risk::Low => Role::Cyan,
        Risk::Medium => Role::Warning,
        Risk::High => Role::Danger,
    };
    badge(match r {
        Risk::Safe => "safe",
        Risk::Low => "low risk",
        Risk::Medium => "your call",
        Risk::High => "high risk",
    })
    .role(role)
    .into()
}

/// The panel.
pub fn panel(k: &Kura) -> Element<Msg> {
    let hints = &k.analysis.hints;
    if hints.is_empty() {
        let words = if k.index == IndexState::Writing {
            "Looking for what could be freed…"
        } else {
            "Nothing stands out here: no caches, build outputs, old downloads or duplicates worth the trouble."
        };
        return container(text(words).role(Role::Text3).wrap().center())
            .center()
            .padding(24.0)
            .width(Length::Fill)
            .height(Length::Fill)
            .into();
    }
    let by = &k.analysis.by_risk;
    let get = |s: &str| by.get(s).copied().unwrap_or(0);
    let mut items: Vec<Element<Msg>> = vec![
        column![
            text(format!("About {} could be freed", format::size(k.analysis.reclaimable))).semibold().size(15.0),
            text(format!(
                "{} safe · {} low risk · {} for you to decide",
                format::size(get("safe")),
                format::size(get("low")),
                format::size(get("medium") + get("high"))
            ))
            .size(11.5)
            .tabular()
            .role(Role::Text3),
            text("Pick one to see it in the rings. Nothing is moved or deleted until you say yes.")
                .size(11.5)
                .role(Role::Text3)
                .wrap(),
        ]
        .spacing(3.0)
        .into(),
    ];
    for (section, idx) in sections(hints) {
        items.push(space(4.0));
        items.push(caption(section));
        for i in idx {
            items.push(card(k, i));
        }
    }
    scroll(aizome_kit::column(items).spacing(8.0).padding([10.0, 12.0, 14.0, 12.0]).width(Length::Fill))
        .width(Length::Fill)
        .height(Length::Fill)
        .id("suggestions")
        .into()
}

/// One hint.
fn card(k: &Kura, i: usize) -> Element<Msg> {
    let h = &k.analysis.hints[i];
    let key = hint_key(h);
    let picked = k.focus_hint.as_deref() == Some(key.as_str());
    let open = k.expanded.contains(&key);
    let action = action_of(h);
    let count = if action == HintAction::Duplicates {
        format::plural(h.groups.len() as u64, "set of copies", "sets of copies")
    } else if h.reclaimable_bytes == 0 && h.count.is_some() {
        String::new()
    } else {
        format::plural(h.paths.len() as u64 + h.more_paths, "path", "paths")
    };
    let mut body: Vec<Element<Msg>> = vec![
        row![text(h.title.clone()).semibold().width(Length::Fill), text(h.frees()).semibold().tabular()]
            .spacing(8.0)
            .width(Length::Fill)
            .into(),
        if count.is_empty() {
            risk_badge(h.risk)
        } else {
            row![risk_badge(h.risk), text(count).size(11.0).role(Role::Text3)].spacing(8.0).into()
        },
        text(h.summary.clone()).size(11.5).role(Role::Text2).wrap().into(),
    ];
    let has_list = !h.paths.is_empty() || !h.groups.is_empty();
    if has_list {
        let n = if action == HintAction::Duplicates { h.groups.len() } else { h.paths.len() };
        let what = if action == HintAction::Duplicates { "sets" } else { "paths" };
        body.push(
            button(match (open, n) {
                (true, _) => format!("Hide the {what}"),
                (false, 1) => format!("Show the {}", what.trim_end_matches('s')),
                (false, n) => format!("Show the {} {what}", format::count(n as u64)),
            })
            .toolbar()
            .icon(if open { Icon::ChevronDown } else { Icon::ChevronRight })
            .on_press(Msg::ExpandHint(i, !open))
            .into(),
        );
    }
    if open {
        if action == HintAction::Duplicates {
            body.extend(duplicate_sets(k, i, h));
        } else {
            body.extend(path_rows(k, h, action == HintAction::Trash));
        }
    }
    body.push(actions(k, i, h, action));
    let card = container(aizome_kit::column(body).spacing(6.0).width(Length::Fill))
        .padding([10.0, 12.0])
        .fill(Tone::Raised)
        .border(if picked { Edge::Focus } else { Edge::Line })
        .width(Length::Fill);
    mouse_area(card).on_press(Msg::FocusHint(if picked { None } else { Some(i) })).key(key).into()
}

/// A hint's paths, each with a tick when its action can leave it out.
fn path_rows(k: &Kura, h: &Hint, ticks: bool) -> Vec<Element<Msg>> {
    let mut rows: Vec<Element<Msg>> = h
        .paths
        .iter()
        .take(SHOWN)
        .map(|p| {
            let path = Path::new(&p.path);
            let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| p.path.clone());
            let mut under = path.parent().map(format::path).unwrap_or_default();
            if let Some(note) = &p.note {
                under = format!("{under} · {note}");
            }
            let size = if p.allocated_bytes > 0 { format::size(p.allocated_bytes) } else { String::new() };
            let mut r: Vec<Element<Msg>> = Vec::new();
            if ticks {
                let key = p.path.clone();
                r.push(checkbox("", k.chosen(h, &p.path)).on_toggle(move |on| Msg::Choose(key.clone(), on)).into());
            }
            r.push(
                column![text(name).size(12.5), text(under).size(11.0).role(Role::Text3)]
                    .spacing(1.0)
                    .width(Length::Fill)
                    .into(),
            );
            r.push(text(size).size(11.5).tabular().role(Role::Text2).into());
            aizome_kit::row(r).spacing(6.0).width(Length::Fill).key(p.path.clone()).into()
        })
        .collect();
    let more = h.paths.len().saturating_sub(SHOWN) as u64 + h.more_paths;
    if more > 0 {
        rows.push(
            text(format!("…and {} more in index.json", format::plural(more, "path", "paths")))
                .size(11.0)
                .role(Role::Text3)
                .into(),
        );
    }
    rows
}

/// Sets of identical files, the copies side by side: the person keeps one.
fn duplicate_sets(k: &Kura, i: usize, h: &Hint) -> Vec<Element<Msg>> {
    let mut out = Vec::new();
    for (g, set) in h.groups.iter().enumerate().take(20) {
        let kept = k.kept(&set.hash).min(set.paths.len().saturating_sub(1));
        let how = if set.verified == "full" {
            "identical, every byte compared"
        } else {
            "same size and ends: compare them first"
        };
        out.push(text(format!("{} each · {how}", format::size(set.bytes_each))).size(11.0).role(Role::Text3).into());
        let copies: Vec<Element<Msg>> = set
            .paths
            .iter()
            .enumerate()
            .map(|(j, p)| {
                let path = Path::new(p);
                let hash = set.hash.clone();
                let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                let folder = path.parent().map(format::path).unwrap_or_default();
                container(
                    column![
                        radio(if j == kept { "Keep" } else { "Keep this one" }, j, Some(kept))
                            .on_select(move |j| Msg::Keep(hash.clone(), j)),
                        text(name).size(12.5).semibold(),
                        text(folder).size(11.0).role(Role::Text3),
                    ]
                    .spacing(2.0),
                )
                .padding(6.0)
                .fill(if j == kept { Tone::Sunk } else { Tone::Raised })
                .border(if j == kept { Edge::Focus } else { Edge::Line })
                .width(Length::Fill)
                .into()
            })
            .collect();
        let mut copies = copies.into_iter();
        while let Some(a) = copies.next() {
            let b: Element<Msg> = copies.next().unwrap_or_else(|| spacer().into());
            out.push(row![a, b].spacing(6.0).align(Align::Start).width(Length::Fill).into());
        }
        let others = set.paths.len().saturating_sub(1) as u64;
        out.push(
            button(format!("Move {} to Trash…", format::plural(others, "other copy", "other copies")))
                .compact()
                .icon(Icon::Trash)
                .on_press(Msg::ActGroup(i, g))
                .into(),
        );
    }
    if h.groups.len() > 20 {
        out.push(
            text(format!("…and {} more sets in index.json", h.groups.len() - 20)).size(11.0).role(Role::Text3).into(),
        );
    }
    out
}

/// What the hint's button does.
fn actions(k: &Kura, i: usize, h: &Hint, action: HintAction) -> Element<Msg> {
    match action {
        HintAction::Trash => {
            let (paths, bytes) = k.chosen_paths(h);
            let n = paths.len() as u64;
            let note = if n == 0 {
                "Tick what to move first".to_owned()
            } else {
                format!("{}; freed once the Trash is emptied", format::size(bytes))
            };
            row![
                button(if n == 0 {
                    "Move to Trash…".to_owned()
                } else {
                    format!("Move {} to Trash…", format::plural(n, "item", "items"))
                })
                .compact()
                .icon(Icon::Trash)
                .on_press_maybe((n > 0).then_some(Msg::Act(i))),
                text(note).size(11.0).role(Role::Text3).width(Length::Fill),
            ]
            .spacing(8.0)
            .width(Length::Fill)
            .into()
        }
        HintAction::EmptyTrash => row![
            button("Empty Trash…").compact().destructive().icon(Icon::Trash).on_press(Msg::Act(i)),
            text("Deleted for good: it can't be undone").size(11.0).role(Role::Text3),
        ]
        .spacing(8.0)
        .into(),
        HintAction::Yukimi => {
            button("Open Yukimi's Storage").compact().icon(Icon::Snowflake).on_press(Msg::Act(i)).into()
        }
        HintAction::Command => {
            let cmd = h.action.command.clone().unwrap_or_default();
            column![
                text(h.action.description.clone()).size(11.0).role(Role::Text3).wrap(),
                row![
                    container(text(cmd.clone()).mono().size(11.5).selectable())
                        .padding([4.0, 6.0])
                        .fill(Tone::Sunk)
                        .border(Edge::Line)
                        .width(Length::Fill),
                    button("Copy").compact().icon(Icon::Copy).on_press(Msg::CopyText(cmd)),
                ]
                .spacing(6.0)
                .width(Length::Fill),
            ]
            .spacing(4.0)
            .width(Length::Fill)
            .into()
        }
        HintAction::Duplicates => text("Open the sets, keep one copy of each, and move the others to the Trash.")
            .size(11.0)
            .role(Role::Text3)
            .wrap()
            .into(),
    }
}

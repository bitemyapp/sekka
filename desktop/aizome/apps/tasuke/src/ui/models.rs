//! The Models page: this computer, then for each role the recommended
//! model and the others that fit, each with its fit bar, who made it, its
//! licence, how it did in Sekka's evaluations and what to know about it;
//! and the origins a person can hide.

use aizome_kit::prelude::*;
use aizome_kit::widgets::layout::column as column_of;
use aizome_kit::{column, row};

use super::bar;
use crate::app::{Msg, Tasuke};
use crate::models::{self, Card, Device, DeviceKind, Install, Section, size};

const WIDE: f64 = 860.0;

pub fn view(app: &Tasuke) -> Element<Msg> {
    let header = column![
        text("Models").size(22.0).weight(700.0),
        text(
            "Everything runs on this computer. Sekka measured each of these models doing the assistant's work, \
             and what fits is worked out from this computer's memory."
        )
        .size(13.0)
        .role(Role::Text2)
        .wrap().line_height(1.4),
    ]
    .spacing(4.0);
    let mut parts: Vec<Element<Msg>> = vec![header.into()];
    if let Some(p) = &app.models.problem {
        parts.push(
            container(row![icon(Icon::CircleAlert).size(15.0).role(Role::Danger), text(p.clone()).wrap().line_height(1.4)].spacing(8.0))
                .padding([8.0, 12.0])
                .fill(Tone::Chrome)
                .border(Edge::Line)
                .width(Fill)
                .into(),
        );
    }
    match &app.models.hardware {
        None => parts.push(
            column![text("Looking at this computer…").role(Role::Text2), progress_indeterminate().width(240.0)].spacing(8.0).into(),
        ),
        Some(hw) => {
            parts.push(machine(app, hw));
            let sections = models::sections(hw, &app.models.catalog, &app.models.prefs, &app.models.installs_now(), app.models.in_use.as_ref());
            for s in &sections {
                parts.push(section(s));
            }
            parts.push(origins(app));
        }
    }
    scroll(container(column_of(parts).spacing(22.0).max_width(WIDE)).align_x(Align::Center).padding([24.0, 32.0, 32.0, 32.0]).width(Fill))
        .key("models")
        .into()
}

fn device_line(d: &Device) -> Element<Msg> {
    let glyph = match d.kind {
        DeviceKind::Gpu | DeviceKind::UnifiedGpu => Icon::Microchip,
        DeviceKind::Memory => Icon::MemoryStick,
        DeviceKind::Processor => Icon::Cpu,
    };
    let mut line = row![
        icon(glyph).size(16.0).role(Role::Text2),
        text(d.name.clone()).size(13.0).weight(600.0),
        text(d.memory.clone()).size(12.5).role(Role::Text2),
    ]
    .spacing(10.0);
    for t in &d.tags {
        line = line.push(tag(t, Role::Text2));
    }
    line.into()
}

/// A small label in a box: "drives the display", "Recommended".
pub(crate) fn tag(words: &str, role: Role) -> Element<Msg> {
    container(text(words.to_owned()).size(10.5).role(role)).padding([1.0, 6.0]).border(Edge::Line).into()
}

/// This computer: each GPU, memory, processor; what a model may use.
pub(crate) fn machine(app: &Tasuke, hw: &tasuke_models::Hardware) -> Element<Msg> {
    let lines: Vec<Element<Msg>> = models::devices(hw).iter().map(device_line).collect();
    let mut body = column![
        aizome_kit::caption("This computer"),
        container(
            column![
                column_of(lines).spacing(8.0),
                divider().edge(Edge::Line),
                row![super::lead(icon(Icon::Info).size(13.0).role(Role::Text3)), text(models::budget_words(hw)).size(12.5).role(Role::Text2).wrap().line_height(1.4)]
                    .spacing(8.0)
                    .align(Align::Start),
            ]
            .spacing(10.0)
            .width(Fill)
        )
        .padding([12.0, 14.0])
        .fill(Tone::Chrome)
        .border(Edge::Line)
        .width(Fill),
    ]
    .spacing(0.0);
    if !app.models.server.is_empty() {
        body = body.push(
            container(row![icon(Icon::Server).size(13.0).role(Role::Text3), text(app.models.server.clone()).size(12.0).role(Role::Text3)].spacing(8.0))
                .padding([8.0, 2.0, 0.0, 2.0]),
        );
    }
    body.width(Fill).into()
}

fn section(s: &Section) -> Element<Msg> {
    let mut parts: Vec<Element<Msg>> = vec![
        column![text(s.title).size(16.0).weight(700.0), text(s.about).size(12.5).role(Role::Text2).wrap().line_height(1.4)].spacing(2.0).into(),
    ];
    match s.cards.split_first() {
        None => parts.push(
            text("Nothing in the catalog fits this computer for this.").size(12.5).role(Role::Text3).into(),
        ),
        Some((first, rest)) => {
            parts.push(card(first, true));
            if !rest.is_empty() {
                parts.push(aizome_kit::caption(format!("Also fits · {}", rest.len())));
                for c in rest {
                    parts.push(card(c, false));
                }
            }
        }
    }
    column_of(parts).spacing(10.0).width(Fill).key(s.title).into()
}

fn fact(glyph: Icon, words: String) -> Element<Msg> {
    row![icon(glyph).size(13.0).role(Role::Text3), text(words).size(12.0).role(Role::Text2)].spacing(6.0).into()
}

/// What can be done with it, by how its download stands.
fn actions(c: &Card) -> Element<Msg> {
    let id = c.id.clone();
    match &c.install {
        Install::Absent => button(format!("Download · {}", size(c.download))).icon(Icon::Download).on_press(Msg::Download(id)).into(),
        Install::Partial { have, total } => column![
            button("Resume Download").icon(Icon::Download).on_press(Msg::Download(id.clone())),
            text(format!("{} of {} here", size(*have), size(*total))).size(11.0).role(Role::Text3),
        ]
        .spacing(3.0)
        .align(Align::End)
        .into(),
        Install::Downloading(p) => {
            let toggle: Element<Msg> = if p.paused {
                button("Resume").icon(Icon::Play).on_press(Msg::Resume).into()
            } else {
                button("Pause").icon(Icon::Pause).on_press(Msg::Pause).into()
            };
            row![toggle, button("Cancel").on_press(Msg::CancelDownload)].spacing(6.0).into()
        }
        Install::Installed if c.in_use => {
            row![tag("In use", Role::Success), button("Remove").on_press(Msg::Remove(id))].spacing(8.0).into()
        }
        Install::Installed => {
            row![button("Use").primary().on_press(Msg::Use(id.clone())), button("Remove").on_press(Msg::Remove(id))].spacing(6.0).into()
        }
    }
}

/// A model: name and tags, maker, lineage and licence; the fit bar;
/// score, speed and size; why it's recommended; what to know.
fn card(c: &Card, first: bool) -> Element<Msg> {
    let mut tags = row![text(c.name.clone()).size(16.0).weight(700.0)].spacing(8.0);
    if first {
        tags = tags.push(tag("Recommended", Role::Accent));
    }
    if c.curated && !first {
        tags = tags.push(tag("Sekka's pick", Role::Text2));
    }
    if !c.evaluated {
        tags = tags.push(tag("Not evaluated yet", Role::Warning));
    }
    if c.vision {
        tags = tags.push(tag("Sees images", Role::Text2));
    }
    let licence = match &c.licence_note {
        Some(_) => format!("{} licence (see notes)", c.licence),
        None => format!("{} licence", c.licence),
    };
    let who = format!("By {} · {} · {} · {}", c.maker, c.lineage, c.origin, licence);
    let head = row![
        column![tags, text(who).size(12.0).role(Role::Text3).wrap().line_height(1.4)].spacing(4.0).width(Fill),
        actions(c),
    ]
    .spacing(12.0)
    .align(Align::Start);
    let mut parts: Vec<Element<Msg>> = vec![head.into()];
    if let Install::Downloading(p) = &c.install {
        parts.push(column![progress(p.fraction()).width(Fill), text(p.words()).size(12.0).role(Role::Text2)].spacing(5.0).into());
    }
    parts.push(column![bar::verdict(&c.bar), bar::bar(&c.bar, 12.0), bar::legend(&c.bar)].spacing(7.0).width(Fill).into());
    let score = match &c.score {
        Some((_, date)) if !date.is_empty() => format!("{} · measured {date}", c.score_words),
        _ => c.score_words.clone(),
    };
    let mut facts = row![fact(Icon::ListChecks, score)].spacing(18.0);
    if let Some(s) = c.speed {
        facts = facts.push(fact(Icon::Gauge, format!("about {s:.0} tokens/s here")));
    }
    facts = facts.push(fact(Icon::HardDriveDownload, format!("{} · {}", size(c.download), c.quant)));
    parts.push(facts.into());
    if let Some(why) = &c.why {
        parts.push(text(why.clone()).size(12.5).role(Role::Text2).wrap().line_height(1.4).into());
    }
    let notes: Vec<Element<Msg>> = c
        .incidents
        .iter()
        .map(|i| (Icon::TriangleAlert, Role::Warning, i.clone()))
        .chain(c.licence_note.iter().map(|n| (Icon::Info, Role::Text3, n.clone())))
        .chain(c.notes.iter().map(|n| (Icon::Dot, Role::Text3, n.clone())))
        .map(|(g, r, n)| row![super::lead(icon(g).size(13.0).role(r)), text(n).size(12.0).role(Role::Text2).wrap().line_height(1.4)].spacing(6.0).align(Align::Start).into())
        .collect();
    if !notes.is_empty() {
        parts.push(column_of(notes).spacing(4.0).width(Fill).into());
    }
    if let Some(m) = &c.measured {
        parts.push(text(format!("Measured at {m}.")).size(11.0).role(Role::Text3).into());
    }
    container(column_of(parts).spacing(10.0).width(Fill))
        .padding([14.0, 16.0])
        .fill(Tone::Chrome)
        .border(if first { Edge::Line2 } else { Edge::Line })
        .width(Fill)
        .key(c.id.0.clone())
        .into()
}

/// Hide models by where their makers are.
fn origins(app: &Tasuke) -> Element<Msg> {
    let boxes: Vec<Element<Msg>> = models::origins(&app.models.catalog)
        .into_iter()
        .map(|o| {
            let hidden = app.models.prefs.exclude_lineage.iter().any(|x| x.eq_ignore_ascii_case(&o));
            let origin = o.clone();
            checkbox(o, hidden).on_toggle(move |on| Msg::HideOrigin(origin.clone(), on)).into()
        })
        .collect();
    column![
        aizome_kit::caption("Hide models made in"),
        container(
            column![
                aizome_kit::widgets::layout::row(boxes).spacing(18.0),
                text("A model counts as made where its maker is, and where the models it was built from were made.")
                    .size(12.0)
                    .role(Role::Text3)
                    .wrap().line_height(1.4),
            ]
            .spacing(8.0)
            .width(Fill)
        )
        .padding([12.0, 14.0])
        .fill(Tone::Chrome)
        .border(Edge::Line)
        .width(Fill),
    ]
    .width(Fill)
    .into()
}

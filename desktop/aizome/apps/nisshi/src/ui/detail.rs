//! The details of the message selected: what wrote it and when, all of
//! what it says, and every field the journal keeps with it, each of which
//! can become a filter.

use aizome_kit::prelude::*;
use aizome_kit::widgets::layout::column as column_of;

use crate::app::{Msg, Nisshi};
use crate::entry::Transport;

/// Fields that say where an entry is, not what it is: nothing to filter by.
fn addressing(field: &str) -> bool {
    field.starts_with("__") || field.ends_with("_TIMESTAMP")
}

pub fn view(n: &Nisshi) -> Element<Msg> {
    let Some(e) = n.selection() else {
        let words = if n.selected.len() > 1 {
            format!("{} selected: ❖C copies them.", crate::count(n.selected.len(), "message", "messages"))
        } else {
            "Select a message to read all of it, and every field it was written with.".into()
        };
        return container(text(words).role(Role::Text3).center().max_lines(3))
            .center()
            .padding(16.0)
            .fill(Tone::Surface)
            .width(Length::Fill)
            .height(Length::Fill)
            .id("details")
            .into();
    };
    let mut head: Vec<Element<Msg>> = Vec::new();
    if let Some((glyph, role)) = super::table::level_glyph(e.priority) {
        head.push(icon(glyph).size(15.0).role(role).into());
    }
    head.push(text(e.source.to_string()).semibold().size(14.0).into());
    let level = if e.flags & crate::entry::WRITTEN_LEVEL != 0 {
        // journald filed the service's output at Info; the line says more.
        format!("{} (as written in the message)", e.priority.word())
    } else {
        e.priority.word().to_owned()
    };
    head.push(text(level).size(11.5).role(Role::Text3).into());
    if e.pid > 0 {
        head.push(text(format!("process {}", e.pid)).size(11.5).tabular().role(Role::Text3).into());
    }
    if let Some(u) = e.unit_shown() {
        head.push(text(u.to_string()).size(11.5).mono().role(Role::Text3).into());
    }
    head.push(spacer().into());
    head.push(button("Copy").compact().icon(Icon::Copy).on_press(Msg::CopyMessage).tooltip("Copy the message").into());
    head.push(
        button("Around This Time")
            .compact()
            .icon(Icon::Clock)
            .on_press(Msg::Context(e.seq))
            .tooltip("Every message from a minute either side of it")
            .id("context")
            .into(),
    );
    let running_here =
        e.pid > 0 && e.transport != Transport::File && std::path::Path::new(&format!("/proc/{}", e.pid)).exists();
    if running_here {
        head.push(
            button("Activity")
                .compact()
                .icon(Icon::Activity)
                .on_press(Msg::OpenActivity(e.pid))
                .tooltip(format!("Show process {} in Activity", e.pid))
                .into(),
        );
    }
    let when = if e.time > 0 { format!("{} · {}", n.clock.full(e.time), n.clock.ago(e.time)) } else { String::new() };
    let mut parts: Vec<Element<Msg>> = vec![
        aizome_kit::widgets::layout::row(head).spacing(8.0).width(Length::Fill).into(),
        text(when).size(11.5).tabular().role(Role::Text3).into(),
        container(message(e))
            .padding([8.0, 10.0])
            .fill(Tone::Sunk)
            .border(Edge::Line)
            .width(Length::Fill)
            .id("message")
            .into(),
    ];
    if e.transport != Transport::File {
        match &n.fields {
            Some((seq, fields)) if *seq == e.seq => {
                parts.push(caption("Fields"));
                let rows: Vec<Element<Msg>> = fields
                    .iter()
                    .filter(|(k, _)| k != "MESSAGE")
                    .enumerate()
                    .map(|(i, (k, v))| field_row(i, k, v))
                    .collect();
                parts.push(column_of(rows).spacing(1.0).width(Length::Fill).into());
            }
            _ => parts.push(text("Reading its fields…").size(11.5).role(Role::Faint).into()),
        }
    }
    scroll(column_of(parts).spacing(8.0).padding([10.0, 12.0]).width(Length::Fill))
        .width(Length::Fill)
        .height(Length::Fill)
        .id("details")
        .into()
}

fn field_row(i: usize, k: &str, v: &str) -> Element<Msg> {
    let mut row: Vec<Element<Msg>> = vec![
        text(k.to_owned()).mono().size(11.0).role(Role::Text3).width(200.0).into(),
        text(crate::ansi::parse(v).map_or_else(|| v.to_owned(), |(plain, _)| plain))
            .mono()
            .size(11.5)
            .wrap()
            .selectable()
            .width(Length::Fill)
            .into(),
    ];
    if !addressing(k) {
        row.push(
            icon_button(Icon::ListFilter)
                .plain()
                .compact()
                .on_press(Msg::FilterField(k.to_owned(), v.to_owned()))
                .tooltip(format!("Only messages with {k}={}", crate::filter::short(v)))
                .label(format!("Filter by {k}"))
                .into(),
        );
    }
    aizome_kit::widgets::layout::row(row).spacing(10.0).width(Length::Fill).key(format!("{i} {k}")).into()
}

/// The message whole: selectable plain text, or in its terminal colours
/// where it has them (Copy copies its plain text).
fn message(e: &crate::entry::Entry) -> Element<Msg> {
    match e.styles.as_deref() {
        Some(runs) => rich_text(super::table::styled(&e.message, runs, &[], Role::Text))
            .font(FontSpec::mono(12.0))
            .wrap()
            .width(Length::Fill)
            .into(),
        None => text(e.message.to_string()).mono().size(12.0).wrap().selectable().into(),
    }
}

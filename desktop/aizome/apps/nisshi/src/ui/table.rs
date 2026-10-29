//! The messages: when, how severe, what wrote it, and its first line, the
//! words searched for picked out in the focus colour. Only the rows that
//! show are built.

use std::rc::Rc;

use aizome_kit::prelude::*;

use crate::app::{Msg, Nisshi};
use crate::entry::{Entry, Priority};
use crate::search::Search;
use crate::time::Clock;

/// A level's glyph and colour (none for the ordinary ones).
pub fn level_glyph(p: Priority) -> Option<(Icon, Role)> {
    match p {
        Priority::Emergency | Priority::Alert | Priority::Critical => Some((Icon::Flame, Role::Danger)),
        Priority::Error => Some((Icon::CircleX, Role::Danger)),
        Priority::Warning => Some((Icon::TriangleAlert, Role::Warning)),
        Priority::Notice => Some((Icon::Info, Role::Accent)),
        Priority::Info => None,
        Priority::Debug => Some((Icon::Dot, Role::Faint)),
    }
}

struct Cells {
    shown: crate::model::Shown,
    newest_first: bool,
    search: Search,
    clock: Clock,
}

pub fn view(n: &Nisshi) -> Element<Msg> {
    let store = n.shown();
    let count = n.view.rows.len();
    if count == 0 {
        let words = if n.loading {
            "Reading the journal…".to_owned()
        } else if store.entries.is_empty() {
            if n.in_file() { "The file is empty.".into() } else { "There are no messages here.".into() }
        } else {
            "Nothing matches the filters.".to_owned()
        };
        let mut parts: Vec<Element<Msg>> = Vec::new();
        if n.loading {
            parts.push(progress_indeterminate().width(140.0).into());
        }
        parts.push(text(words).role(Role::Text3).into());
        if !n.loading && !store.entries.is_empty() {
            parts.push(button("Clear Filters").compact().on_press(Msg::ClearFilters).into());
        }
        return container(aizome_kit::widgets::layout::column(parts).spacing(10.0).align(Align::Center))
            .center()
            .fill(Tone::Surface)
            .width(Length::Fill)
            .height(Length::Fill)
            .id("messages")
            .into();
    }
    let cells = Rc::new(Cells {
        shown: n.view.shown.clone(),
        newest_first: n.newest_first,
        search: n.filter.search.clone(),
        clock: n.clock.clone(),
    });
    let file = n.in_file();
    let columns = vec![
        Column::new("Time").width(if file { 130.0 } else { 118.0 }).min_width(80.0),
        Column::new("").width(24.0).min_width(24.0),
        Column::new("Source").width(150.0).min_width(70.0),
        Column::new("Message").width(600.0).min_width(160.0),
    ];
    let names = {
        let c = cells.clone();
        move |r: usize| c.at(r).source.to_string()
    };
    let menus = {
        let c = cells.clone();
        move |r: usize| row_menu(c.at(r))
    };
    let c = cells;
    let mut t = table(columns, count, move |r, col| cell(c.at(r), col, &c))
        .selected(n.selected_rows())
        .on_select(Msg::SelectRows)
        .on_activate(Msg::Activate)
        .multiple()
        .type_select(names)
        .context_menu(menus)
        .row_height(22.0)
        .width(Length::Fill)
        .height(Length::Fill)
        .id("messages");
    if let Some(r) = n.reveal() {
        t = t.reveal(r);
    }
    t.into()
}

impl Cells {
    /// The message in a table row.
    fn at(&self, r: usize) -> &Entry {
        let n = self.shown.len();
        let row = if self.newest_first { n.saturating_sub(r + 1) } else { r };
        &self.shown[row.min(n - 1)]
    }
}

fn row_menu(e: &Entry) -> Menu<Msg> {
    let mut m = Menu::new("")
        .item(MenuItem::new("Copy", Msg::Copy).shortcut(Shortcut::cmd('C')))
        .item(MenuItem::new("Copy Message Only", Msg::CopyMessage))
        .separator()
        .item(MenuItem::new(format!("Only {}", e.source), Msg::OnlySource(e.source.clone())))
        .item(MenuItem::new("Show Messages Like This", Msg::FilterField("MESSAGE".into(), String::new())))
        .item(MenuItem::new("Around This Time", Msg::Context(e.seq)));
    if e.pid > 0 {
        m = m.separator().item(MenuItem::new(format!("Show Process {} in Activity", e.pid), Msg::OpenActivity(e.pid)));
    }
    m
}

fn cell(e: &Entry, col: usize, c: &Cells) -> Element<Msg> {
    match col {
        0 => text(c.clock.stamp(e.time)).mono().size(11.5).tabular().role(Role::Text3).into(),
        1 => match level_glyph(e.priority) {
            Some((glyph, role)) => icon(glyph).size(13.0).role(role).label(e.priority.word()).into(),
            None => space(1.0),
        },
        2 => text(e.source.to_string()).role(if e.priority.is_problem() { Role::Text } else { Role::Text2 }).into(),
        _ => message(e, &c.search),
    }
}

/// A message's first line, the search's words picked out, and how many
/// more lines it has.
fn message(e: &Entry, search: &Search) -> Element<Msg> {
    let (first, more) = e.first_line();
    let role = match e.priority {
        Priority::Emergency | Priority::Alert | Priority::Critical => Role::Danger,
        Priority::Debug => Role::Text3,
        _ => Role::Text,
    };
    let marks = search.highlights(first);
    let mut spans: Vec<Span> = Vec::new();
    let mut at = 0;
    for m in marks {
        if m.start > at {
            spans.push(Span::new(&first[at..m.start]).role(role));
        }
        spans.push(Span::new(&first[m.start..m.end]).role(Role::Focus).weight(700.0));
        at = m.end;
    }
    if at < first.len() || spans.is_empty() {
        spans.push(Span::new(&first[at..]).role(role));
    }
    if more > 0 {
        spans.push(
            Span::new(format!("   +{more} {}", if more == 1 { "line" } else { "lines" })).role(Role::Faint).size(11.0),
        );
    }
    rich_text(spans).into()
}

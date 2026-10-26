//! What Fusen's windows show: the main window (folders, the list with
//! its search and preview lines, the note) and a sticky (the note on its
//! colour's paper).

use std::rc::Rc;
use std::sync::{Arc, Mutex};

use aizome_kit::prelude::*;
use aizome_kit::{Highlighter, LineStyle, column, row};
use aizome_sumi::syntax::{self, Highlight, Kind};
use ropey::Rope;

use crate::app::{Fusen, Msg, Open};
use crate::notes::{Note, NoteColor};

pub const LIST_ROW: f64 = 58.0;

fn rgb(c: u32) -> Color {
    Color::rgba8((c >> 16) as u8, (c >> 8) as u8, c as u8, 255)
}

/// "Today 14:02", "Yesterday", "Mon", "12 Oct", "12 Oct 2025".
pub fn when(t: std::time::SystemTime, now: std::time::SystemTime) -> String {
    let secs = |t: std::time::SystemTime| t.duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs() as i64);
    let (a, b) = (secs(t), secs(now));
    let offset = local_offset();
    let day = |s: i64| (s + offset).div_euclid(86400);
    let (d, today) = (day(a), day(b));
    let hm = {
        let s = (a + offset).rem_euclid(86400);
        format!("{:02}:{:02}", s / 3600, s % 3600 / 60)
    };
    if d == today {
        return hm;
    }
    if d == today - 1 {
        return "Yesterday".to_owned();
    }
    let iso = aizome_sumi::recent::iso8601(
        std::time::UNIX_EPOCH + std::time::Duration::from_secs((a + offset).max(0) as u64),
    );
    let (y, m, dd) = (&iso[0..4], &iso[5..7], &iso[8..10]);
    const MONTHS: [&str; 12] = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];
    let month = MONTHS[m.parse::<usize>().unwrap_or(1).clamp(1, 12) - 1];
    let this_year = &aizome_sumi::recent::iso8601(now)[0..4];
    let dd = dd.trim_start_matches('0');
    if y == this_year { format!("{dd} {month}") } else { format!("{dd} {month} {y}") }
}

fn local_offset() -> i64 {
    // SAFETY: localtime_r writes only into the tm given.
    unsafe {
        let now = libc::time(std::ptr::null_mut());
        let mut tm: libc::tm = std::mem::zeroed();
        if libc::localtime_r(&now, &mut tm).is_null() {
            return 0;
        }
        tm.tm_gmtoff
    }
}

/// A note's styles: Markdown, lightly (code in mono, the first line a
/// title), from the note's line states.
pub fn highlighter(open: &Open, ink: Option<Color>) -> Highlighter {
    let cache: Arc<Mutex<Highlight>> = open.highlight.clone();
    let rope: Rope = open.content.rope().clone();
    Arc::new(move |line: usize, text: &str| {
        let Ok(mut h) = cache.lock() else { return Vec::new() };
        let tokens = h.tokens(line, text, &rope);
        drop(h);
        let mut out = Vec::new();
        if line == 0 && !text.trim().is_empty() && text.trim_end() != "---" {
            out.push(LineStyle { range: 0..text.len(), span: Span::new("").weight(700.0).size(17.0) });
        }
        for t in tokens {
            let mut span = syntax::span(t.kind);
            if matches!(t.kind, Kind::Code | Kind::FenceMark | Kind::FenceInfo | Kind::TaskBox) {
                span = span.mono();
            }
            // On a sticky's paper, the ink is the note's, kept dark.
            if let Some(c) = ink
                && span.role.is_some()
            {
                span.role = None;
                span.color = Some(c);
                if matches!(
                    t.kind,
                    Kind::TaskDone
                        | Kind::EmphasisMark
                        | Kind::HeadingMark
                        | Kind::Url
                        | Kind::Rule
                        | Kind::FenceMark
                        | Kind::Comment
                ) {
                    span.alpha = Some(0.5);
                }
            }
            out.push(LineStyle { range: t.range, span });
        }
        out
    })
}

fn dot(color: NoteColor, size: f64) -> Element<Msg> {
    canvas(color, move |c, r, color, _| {
        let s = size.min(r.width()).min(r.height());
        let rect =
            Rect::from_origin_size(Point::new(r.x0 + (r.width() - s) / 2.0, r.y0 + (r.height() - s) / 2.0), (s, s));
        c.fill_rect(rect, rgb(color.dye()));
    })
    .width(size)
    .height(size)
    .into()
}

#[derive(Clone)]
struct RowData {
    title: String,
    preview: String,
    when: String,
    color: NoteColor,
    pinned: bool,
}

fn note_row(r: &RowData) -> Element<Msg> {
    let title = row![
        text(r.title.clone()).semibold().width(Length::Fill),
        text(r.when.clone()).size(11.0).role(Role::Text3).tabular()
    ]
    .spacing(8.0)
    .width(Length::Fill);
    let mut top: Vec<Element<Msg>> = vec![dot(r.color, 8.0)];
    if r.pinned {
        top.push(icon(Icon::Pin).size(12.0).role(Role::Text3).into());
    }
    top.push(title.into());
    let preview = if r.preview.is_empty() { "No more text".to_owned() } else { r.preview.clone() };
    column![aizome_kit::row(top).spacing(6.0).width(Length::Fill), text(preview).size(12.0).role(Role::Text3),]
        .spacing(3.0)
        .padding([8.0, 12.0])
        .width(Length::Fill)
        .into()
}

fn sidebar(app: &Fusen) -> Element<Msg> {
    let mut items: Vec<Element<Msg>> = vec![caption("Fusen")];
    let all = app.notes.len();
    items.push(
        sidebar_item(Icon::StickyNote, "All Notes", app.folder.is_none())
            .detail(all.to_string())
            .on_press(Msg::Folder(None))
            .into(),
    );
    let pinned = app.notes.iter().filter(|n| n.meta.pinned).count();
    if pinned > 0 {
        items.push(
            sidebar_item(Icon::Pin, "Pinned", app.folder.as_deref() == Some(crate::app::PINNED))
                .detail(pinned.to_string())
                .on_press(Msg::Folder(Some(crate::app::PINNED.to_owned())))
                .into(),
        );
    }
    if !app.folders.is_empty() {
        items.push(caption("Folders"));
        for f in &app.folders {
            let n = app.notes.iter().filter(|x| x.folder == *f || x.folder.starts_with(&format!("{f}/"))).count();
            let depth = f.matches('/').count();
            let name = f.rsplit('/').next().unwrap_or(f);
            let label = format!("{}{name}", "    ".repeat(depth));
            items.push(
                sidebar_item(Icon::Folder, label, app.folder.as_deref() == Some(f.as_str()))
                    .detail(n.to_string())
                    .on_press(Msg::Folder(Some(f.clone())))
                    .into(),
            );
        }
    }
    aizome_kit::column(items).padding([4.0, 0.0]).width(Length::Fill).into()
}

fn note_list(app: &Fusen, shown: &[&Note]) -> Element<Msg> {
    let now = std::time::SystemTime::now();
    let rows: Rc<Vec<RowData>> = Rc::new(
        shown
            .iter()
            .map(|n| RowData {
                title: n.title.clone(),
                preview: n.preview.clone(),
                when: when(n.modified, now),
                color: n.meta.color,
                pinned: n.meta.pinned,
            })
            .collect(),
    );
    let ids: Vec<u64> = shown.iter().map(|n| n.id).collect();
    let selected = app.selected.and_then(|s| ids.iter().position(|i| *i == s));
    let ids = Rc::new(ids);
    let (a, b) = (ids.clone(), ids.clone());
    let search = search_field(&app.query).placeholder("Search").on_change(Msg::Search).id("search").width(Length::Fill);
    let list: Element<Msg> = if rows.is_empty() {
        container(text(if app.query.is_empty() { "No notes" } else { "No notes match" }).role(Role::Text3))
            .center()
            .width(Length::Fill)
            .height(Length::Fill)
            .into()
    } else {
        let r = rows.clone();
        virtual_list(rows.len(), LIST_ROW, move |i| note_row(&r[i]))
            .selected(selected)
            .on_select(move |v| Msg::Select(v.first().and_then(|i| a.get(*i)).copied()))
            .on_activate(move |i| Msg::Sticky(b.get(i).copied()))
            .id("notes")
            .into()
    };
    column![container(search).padding([8.0, 10.0]).width(Length::Fill), divider(), list]
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
}

fn editor(app: &Fusen) -> Element<Msg> {
    let Some(id) = app.selected else {
        return container(text("No note selected").role(Role::Text3))
            .center()
            .width(Length::Fill)
            .height(Length::Fill)
            .into();
    };
    let Some(open) = app.open.get(&id) else {
        return container(text("").role(Role::Text3)).width(Length::Fill).height(Length::Fill).into();
    };
    note_editor(id, open, None).id("editor").into()
}

/// A note's editor: in the main window, or (with its colour) a sticky's.
pub fn note_editor(
    id: u64,
    open: &Open,
    color: Option<NoteColor>,
) -> aizome_kit::widgets::text_editor::TextEditor<Msg> {
    let rope = open.content.rope().clone();
    let ink = color.map(|c| rgb(c.ink().0));
    let mut e = text_editor(&open.content)
        .on_action(move |a| Msg::Edit(id, a))
        .on_click(move |at, mods| {
            // A task's box ticks and unticks; ❖ opens a link.
            let line = rope.char_to_line(at.min(rope.len_chars()));
            let text = aizome_sumi::syntax::cache::line_text(&rope, line);
            let col = at - rope.line_to_char(line);
            let byte = text.char_indices().nth(col).map_or(text.len(), |(b, _)| b);
            if mods.super_key() {
                return aizome_sumi::syntax::markdown::link_at(&text, byte).map(Msg::OpenUrl);
            }
            let (r, _) = aizome_sumi::syntax::markdown::task_box(&text)?;
            (r.start <= byte && byte <= r.end).then_some(Msg::Tick(id, line))
        })
        .mono(false)
        .line_height(1.5)
        .current_line(false)
        .highlighter(highlighter(open, ink), open.styles)
        .padding(16.0)
        .placeholder("Write a note");
    if let Some(c) = color {
        let (ink, caret) = c.ink();
        e = e.text_color(rgb(ink)).caret_color(rgb(caret)).padding(10.0);
    }
    e
}

fn toolbar(app: &Fusen) -> Element<Msg> {
    let note = app.selected_note();
    let has = note.is_some();
    let pinned = note.is_some_and(|n| n.meta.pinned);
    let colors: Vec<&str> = NoteColor::ALL.iter().map(|c| c.name()).collect();
    let color = note.map(|n| NoteColor::ALL.iter().position(|c| *c == n.meta.color).unwrap_or(0));
    aizome_kit::toolbar(vec![
        icon_button(Icon::SquarePen).toolbar().on_press(Msg::New).tooltip("New Note (❖N)").label("New Note").into(),
        icon_button(Icon::Trash)
            .toolbar()
            .enabled(has)
            .on_press(Msg::Delete)
            .tooltip("Delete Note (❖⌫)")
            .label("Delete Note")
            .into(),
        toolbar_separator(),
        icon_button(Icon::Pin)
            .toolbar()
            .selected(pinned)
            .enabled(has)
            .on_press(Msg::Pin)
            .tooltip(if pinned { "Unpin" } else { "Pin to the Top" })
            .label("Pin")
            .into(),
        dropdown(colors, color)
            .compact()
            .enabled(has)
            .on_select(|i| Msg::Color(NoteColor::ALL[i.min(4)]))
            .label("Colour")
            .into(),
        icon_button(Icon::StickyNote)
            .toolbar()
            .enabled(has)
            .on_press(Msg::Sticky(None))
            .tooltip("Open as Sticky (❖T)")
            .label("Open as Sticky")
            .into(),
        spacer().into(),
        status_text(match app.notes.len() {
            1 => "1 note".to_owned(),
            n => format!("{n} notes"),
        }),
    ])
}

/// Persistent autosave failures stay visible even when another note is selected.
fn save_notice(app: &Fusen, note: Option<u64>) -> Option<Element<Msg>> {
    let error = app
        .notes
        .iter()
        .filter(|n| note.is_none_or(|id| id == n.id))
        .find_map(|n| app.open.get(&n.id)?.save_error.as_ref().map(|e| format!("{}: {e}", n.title)))?;
    if note.is_some() {
        // A sticky can be only 160×100. Keep its editor and recovery button
        // usable; the complete error is in the tooltip and close alert.
        return Some(
            row![
                text("Not saved")
                    .semibold()
                    .size(11.0)
                    .role(Role::Danger)
                    .wrap()
                    .tooltip(format!("{error}\nKeep Fusen open to preserve your changes."))
                    .width(Length::Fill),
                button("Retry").on_press(Msg::RetrySave),
            ]
            .spacing(4.0)
            .padding(8.0)
            .width(Length::Fill)
            .into(),
        );
    }
    Some(
        column![
            text("Note couldn't be saved").semibold().role(Role::Danger),
            text(error).wrap().size(11.0).width(Length::Fill),
            row![
                text("Keep Fusen open to preserve your changes.").wrap().size(11.0).width(Length::Fill),
                button("Retry").on_press(Msg::RetrySave)
            ]
            .spacing(8.0)
            .width(Length::Fill),
        ]
        .spacing(4.0)
        .padding(10.0)
        .width(Length::Fill)
        .into(),
    )
}

/// The main window.
pub fn main_window(app: &Fusen) -> Element<Msg> {
    let shown = app.shown();
    let content = split(note_list(app, &shown), editor(app)).first_size(280.0).min_sizes(200.0, 260.0);
    let mut children = vec![toolbar(app)];
    if let Some(notice) = save_notice(app, None) {
        children.push(notice);
    }
    children
        .push(sidebar_layout(sidebar(app), content).first_size(180.0).width(Length::Fill).height(Length::Fill).into());
    aizome_kit::column(children).width(Length::Fill).height(Length::Fill).into()
}

/// A sticky: the note on its paper.
pub fn sticky_window(app: &Fusen, note: u64) -> Element<Msg> {
    let color = app.note(note).map(|n| n.meta.color).unwrap_or_default();
    let body: Element<Msg> = match app.open.get(&note) {
        Some(open) => note_editor(note, open, Some(color)).id("sticky-editor").into(),
        None => text("").into(),
    };
    let paper: Element<Msg> =
        container(body).fill(Tone::Color(rgb(color.paper()))).width(Length::Fill).height(Length::Fill).into();
    if let Some(notice) = save_notice(app, Some(note)) {
        column![notice, paper].width(Length::Fill).height(Length::Fill).into()
    } else {
        paper
    }
}

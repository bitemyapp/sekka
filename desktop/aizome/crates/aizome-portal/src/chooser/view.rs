//! How a chooser looks: Hako's sidebar and path bar, the folder's list,
//! and the request's name field, filters, choices and buttons.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use aizome_kit::prelude::*;
use aizome_kit::widgets::layout::Edge;
use hako::model::entry::SortKey;
use hako::model::{format, places::PlaceKind};
use hako::ui::look;
use hako::ui::sidebar::{Crumb, PlaceRow, caption};

use super::model::Loc;
use super::{Chooser, Msg, Sheet, SheetKind};
use crate::request::Mode;

/// The list's columns: their titles, what each sorts by, and widths.
pub(super) const COLUMNS: [(&str, SortKey, f64); 4] = [
    ("Name", SortKey::Name, 270.0),
    ("Date Modified", SortKey::Modified, 150.0),
    ("Size", SortKey::Size, 80.0),
    ("Kind", SortKey::Kind, 160.0),
];

pub(super) fn window(c: &Chooser) -> Element<Msg> {
    let b = &c.browser;
    let mut parts: Vec<Element<Msg>> = Vec::new();
    if b.mode() == Mode::Save {
        parts.push(
            row![
                text("Save as").role(Role::Text2),
                text_field(&b.name)
                    .id("chooser-name")
                    .on_change(Msg::Name)
                    .on_submit(Msg::Accept)
                    .width(Length::Fixed(320.0)),
            ]
            .spacing(10.0)
            .padding([12.0, 16.0, 4.0, 16.0])
            .align(Align::Center)
            .into(),
        );
    }
    parts.push(path_bar(c));
    parts.push(
        container(sidebar_layout(sidebar(c), files(c)).first_size(190.0))
            .edges(Edge::Line, Sides { top: true, right: false, bottom: true, left: false })
            .width(Length::Fill)
            .height(Length::Fill)
            .into(),
    );
    if let Some(p) = &b.problem {
        parts.push(container(text(p.as_str()).role(Role::Danger).wrap()).padding([8.0, 16.0, 0.0, 16.0]).into());
    }
    if !b.request.choices.is_empty() {
        parts.push(choices(c));
    }
    parts.push(bottom(c));
    column(parts).width(Length::Fill).height(Length::Fill).into()
}

/// Back and forward, then each folder down to here (from home when it's
/// under home), and New Folder where folders can be made.
fn path_bar(c: &Chooser) -> Element<Msg> {
    let b = &c.browser;
    let mut items: Vec<Element<Msg>> = vec![
        icon_button(Icon::ChevronLeft).on_press(Msg::Back).enabled(!b.back.is_empty()).tooltip("Back").into(),
        icon_button(Icon::ChevronRight).on_press(Msg::Forward).enabled(!b.forward.is_empty()).tooltip("Forward").into(),
    ];
    match &b.loc {
        Loc::Recent => items.push(crumb(Some(Icon::Clock), "Recent".into(), true, None)),
        Loc::Folder(dir) => {
            let home = b.home.clone();
            let (mut acc, first, rest): (PathBuf, Element<Msg>, Vec<_>) = match dir.strip_prefix(&home) {
                Ok(rest) if home != Path::new("/") => (
                    home.clone(),
                    crumb(Some(Icon::House), "Home".into(), dir == &home, Some(Msg::Go(Loc::Folder(home.clone())))),
                    rest.components().collect(),
                ),
                _ => (
                    PathBuf::from("/"),
                    crumb(
                        Some(Icon::HardDrive),
                        "Computer".into(),
                        dir == Path::new("/"),
                        Some(Msg::Go(Loc::Folder("/".into()))),
                    ),
                    dir.components().skip(1).collect(),
                ),
            };
            items.push(first);
            let n = rest.len();
            for (i, part) in rest.into_iter().enumerate() {
                acc.push(part);
                items.push(text("›").role(Role::Faint).into());
                let label = part.as_os_str().to_string_lossy().into_owned();
                items.push(crumb(None, label, i + 1 == n, Some(Msg::Go(Loc::Folder(acc.clone())))));
            }
        }
    }
    items.push(spacer().into());
    if c.makes_folders() {
        items.push(button("New Folder").toolbar().on_press(Msg::NewFolder).tooltip("Make a folder here (⇧❖N)").into());
    }
    toolbar(items)
}

fn crumb(icon: Option<Icon>, label: String, current: bool, go: Option<Msg>) -> Element<Msg> {
    Crumb { icon, label: label.into(), current, lit: false, on_press: go.filter(|_| !current), drop: None }.into()
}

fn place(kind: PlaceKind, label: String, selected: bool, go: Msg) -> Element<Msg> {
    let (icon, role) = look::place_icon(kind);
    PlaceRow {
        icon,
        role,
        label: label.into(),
        detail: None,
        selected,
        lit: false,
        busy: false,
        on_press: go,
        on_eject: None,
        on_menu: None,
        drop: None,
    }
    .into()
}

fn sidebar(c: &Chooser) -> Element<Msg> {
    let b = &c.browser;
    let here = |p: &Path| b.loc.folder() == Some(p);
    let mut items: Vec<Element<Msg>> = vec![caption("Places")];
    if b.offers_recent() {
        items.push(place(PlaceKind::Recent, "Recent".into(), b.loc == Loc::Recent, Msg::Go(Loc::Recent)));
    }
    for p in &c.places.standard {
        items.push(place(p.kind, p.name.clone(), here(&p.path), Msg::Go(Loc::Folder(p.path.clone()))));
    }
    if let Some((label, folder)) = &c.places.desk {
        items.push(caption(label));
        let name = folder.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| label.clone());
        items.push(place(PlaceKind::Desk, name, here(folder), Msg::Go(Loc::Folder(folder.clone()))));
    }
    items.push(caption("Disks"));
    match &c.places.disk {
        Some(d) => {
            items.push(place(PlaceKind::Disk, d.name.clone(), here(&d.path), Msg::Go(Loc::Folder(d.path.clone()))))
        }
        None => items.push(place(
            PlaceKind::Disk,
            "Computer".into(),
            here(Path::new("/")),
            Msg::Go(Loc::Folder("/".into())),
        )),
    }
    for v in &c.places.volumes {
        items.push(place(v.kind, v.name.clone(), here(&v.path), Msg::Go(Loc::Folder(v.path.clone()))));
    }
    scroll(column(items).spacing(0.0).padding([0.0, 0.0, 8.0, 0.0]).width(Length::Fill)).into()
}

/// The location's list, or why there's nothing in it.
fn files(c: &Chooser) -> Element<Msg> {
    let b = &c.browser;
    let empty = |words: String| -> Element<Msg> {
        container(text(words).role(Role::Text3).wrap()).padding(24.0).width(Length::Fill).height(Length::Fill).into()
    };
    if let Some(e) = &b.error {
        return container(text(e.as_str()).role(Role::Danger).wrap())
            .padding(24.0)
            .width(Length::Fill)
            .height(Length::Fill)
            .into();
    }
    if b.shown.is_empty() && !b.loading {
        let words = match (&b.loc, b.active_filter()) {
            (Loc::Recent, _) => "No recent files.".to_owned(),
            (_, Some(f)) if !b.entries.is_empty() => format!("Nothing here is among “{}”.", f.name),
            _ if b.request.directory && !b.entries.is_empty() => "No folders here.".to_owned(),
            _ => "This folder is empty.".to_owned(),
        };
        return empty(words);
    }
    let shown = b.shown.clone();
    let names = b.shown.clone();
    let now = c.now.unwrap_or_else(format::now);
    let columns: Vec<Column> = COLUMNS
        .iter()
        .enumerate()
        .map(|(i, (title, _, width))| {
            let col = Column::new(*title).width(*width).sortable();
            if i == 2 { col.align_end() } else { col }
        })
        .collect();
    let sorted = COLUMNS.iter().position(|(_, k, _)| *k == b.sort.key).unwrap_or(0);
    let mut list = table(columns, shown.len(), move |r, col| -> Element<Msg> {
        let e = &shown[r];
        match col {
            0 => {
                let (glyph, role) = look::entry_icon(e);
                row![icon(glyph).size(16.0).role(role), text(e.display.as_str())].spacing(8.0).into()
            }
            1 => text(e.modified.map(|m| format::date(m, now)).unwrap_or_else(|| "—".into())).role(Role::Text2).into(),
            2 if e.is_folder() => text("—").role(Role::Text3).into(),
            2 => text(format::size(e.size)).tabular().role(Role::Text2).into(),
            _ => text(e.kind_name.to_string()).role(Role::Text2).into(),
        }
    })
    .selected(b.selected.iter().copied())
    .on_select(Msg::Select)
    .on_activate(Msg::Activate)
    .on_sort(Msg::Sort)
    .type_select(move |i| names[i].display.clone())
    .id("chooser-files");
    if b.loc != Loc::Recent {
        list = list.sorted(sorted, b.sort.ascending);
    }
    if b.request.multiple {
        list = list.multiple();
    }
    list.into()
}

/// The app's own choices: pop-ups and checkboxes.
fn choices(c: &Chooser) -> Element<Msg> {
    let b = &c.browser;
    let mut items: Vec<Element<Msg>> = Vec::new();
    for (i, (choice, value)) in b.request.choices.iter().zip(&b.choices).enumerate() {
        if choice.is_toggle() {
            items.push(
                checkbox(choice.label.as_str(), value == "true")
                    .on_toggle(move |on| Msg::Choice(i, on.to_string()))
                    .key(format!("choice-{i}"))
                    .into(),
            );
        } else {
            let ids: Arc<Vec<String>> = Arc::new(choice.options.iter().map(|(id, _)| id.clone()).collect());
            let at = ids.iter().position(|id| id == value);
            items.push(
                row![
                    text(choice.label.as_str()).role(Role::Text2),
                    dropdown(choice.options.iter().map(|(_, label)| label.clone()), at)
                        .on_select(move |j| Msg::Choice(i, ids.get(j).cloned().unwrap_or_default()))
                ]
                .spacing(8.0)
                .align(Align::Center)
                .key(format!("choice-{i}"))
                .into(),
            );
        }
    }
    row(items).spacing(16.0).padding([10.0, 16.0, 0.0, 16.0]).width(Length::Fill).align(Align::Center).into()
}

/// The filter, hidden files, and the buttons.
fn bottom(c: &Chooser) -> Element<Msg> {
    let b = &c.browser;
    let mut items: Vec<Element<Msg>> = Vec::new();
    if !b.request.filters.is_empty() {
        items.push(
            dropdown(b.request.filters.iter().map(|f| f.name.clone()), b.filter)
                .on_select(Msg::Filter)
                .id("chooser-filter")
                .into(),
        );
    }
    items.push(checkbox("Show hidden files", b.hidden).on_toggle(Msg::Hidden).into());
    items.push(spacer().into());
    items.push(button("Cancel").cancel().on_press(Msg::Cancel).into());
    items.push(button(b.request.accept()).default().enabled(b.can_accept()).on_press(Msg::Accept).into());
    row(items).spacing(10.0).padding([10.0, 16.0, 12.0, 16.0]).width(Length::Fill).align(Align::Center).into()
}

/// Go to Folder, or New Folder.
pub(super) fn sheet(s: &Sheet) -> Element<Msg> {
    let (title, hint, verb) = match s.kind {
        SheetKind::GoTo => ("Go to Folder", "A path, from here or from home (~/Documents)", "Go"),
        SheetKind::NewFolder => ("New Folder", "The folder's name", "Create"),
    };
    let mut parts: Vec<Element<Msg>> = vec![
        text(title).semibold().size(15.0).into(),
        text_field(&s.text)
            .id("chooser-sheet-field")
            .placeholder(hint)
            .on_change(Msg::SheetText)
            .on_submit(Msg::SheetSubmit)
            .width(Length::Fill)
            .into(),
    ];
    if let Some(p) = &s.problem {
        parts.push(text(p.as_str()).role(Role::Danger).wrap().into());
    }
    parts.push(
        row![
            spacer(),
            button("Cancel").cancel().on_press(Msg::SheetCancel),
            button(verb).default().enabled(!s.text.trim().is_empty()).on_press(Msg::SheetSubmit),
        ]
        .spacing(8.0)
        .into(),
    );
    column(parts).spacing(10.0).padding([18.0, 20.0, 16.0, 20.0]).width(Length::Fixed(420.0)).into()
}

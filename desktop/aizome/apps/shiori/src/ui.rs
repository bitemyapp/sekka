//! What Shiori's windows show: the toolbar, the find bar, the sidebar
//! (the pages, or the document's contents), the pages themselves on
//! `sunk`, the sheets, and the menus.

use std::collections::HashMap;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Arc;

use aizome_kit::column;
use aizome_kit::prelude::*;

use crate::app::{Doc, Find, Load, Msg, Sheet, Shiori, outline_items};
use crate::layout::Zoom;

/// The zoom pop-up's choices.
pub const ZOOM_CHOICES: [Zoom; 10] = [
    Zoom::FitWidth,
    Zoom::FitPage,
    Zoom::Fixed(0.5),
    Zoom::Fixed(0.75),
    Zoom::Fixed(1.0),
    Zoom::Fixed(1.25),
    Zoom::Fixed(1.5),
    Zoom::Fixed(2.0),
    Zoom::Fixed(3.0),
    Zoom::Fixed(4.0),
];

fn zoom_label(z: Zoom) -> String {
    match z {
        Zoom::FitWidth => "Fit Width".into(),
        Zoom::FitPage => "Fit Page".into(),
        Zoom::Fixed(f) => percent(f),
    }
}

pub fn percent(z: f64) -> String {
    format!("{:.0}%", z * 100.0)
}

pub fn window(app: &Shiori, w: WindowKey) -> Element<Msg> {
    let Some(d) = app.doc(w) else { return empty() };
    let pages = crate::pages::pages_area(app, d);
    if d.fullscreen {
        return aizome_kit::widgets::layout::column(vec![pages]).into();
    }
    let body: Element<Msg> = if d.sidebar && d.info().is_some() {
        sidebar_layout(sidebar(app, d), pages).first_size(196.0).into()
    } else {
        pages
    };
    let mut parts = vec![top_bar(d)];
    if let Some(f) = &d.find {
        parts.push(find_bar(d, f));
    }
    parts.push(body);
    aizome_kit::widgets::layout::column(parts).into()
}

/// A window with nothing open.
fn empty() -> Element<Msg> {
    let msg = column![
        icon(Icon::BookOpen).size(48.0).role(Role::Faint),
        text("No document").size(15.0).semibold().role(Role::Text2),
        text("Open a PDF with ❖O, or drop one here.").role(Role::Text3),
        space(8.0),
        button("Open…").on_press(Msg::OpenPanel),
    ]
    .spacing(8.0)
    .align(Align::Center);
    mouse_area(container(msg).center().width(Fill).height(Fill).fill(Tone::Sunk))
        .on_drop(|d| Msg::Open(d.urls.iter().filter_map(|u| file_path(u)).collect()))
        .key("empty")
        .into()
}

fn tool(icon: Icon, tip: &str, msg: Msg, enabled: bool) -> Element<Msg> {
    icon_button(icon).toolbar().on_press(msg).enabled(enabled).tooltip(tip).into()
}

fn top_bar(d: &Doc) -> Element<Msg> {
    let w = d.window;
    let open = d.info().is_some();
    let n = d.pages();
    let mut labels: Vec<String> = ZOOM_CHOICES.iter().map(|z| zoom_label(*z)).collect();
    let selected = ZOOM_CHOICES.iter().position(|z| match (z, d.zoom) {
        (Zoom::Fixed(a), Zoom::Fixed(b)) => (a - b).abs() < 1e-6,
        (a, b) => *a == b,
    });
    let (selected, offset) = match selected {
        Some(i) => (i, 0usize),
        None => {
            labels.insert(0, percent(d.layout.zoom));
            (0, 1)
        }
    };
    let status = match (&d.load, &d.trouble) {
        (_, Some(t)) => text(t.clone()).role(Role::Warning).into(),
        (Load::Open(_), None) => {
            status_text(format!("Page {} of {n} · {}", (d.current_page() + 1).min(n.max(1)), percent(d.layout.zoom)))
        }
        (Load::Opening, None) => status_text("Opening…"),
        _ => status_text(""),
    };
    toolbar(vec![
        icon_button(Icon::PanelLeft)
            .toolbar()
            .selected(d.sidebar)
            .enabled(open)
            .on_press(Msg::ToggleSidebar)
            .tooltip("Sidebar  ⌥❖S")
            .into(),
        toolbar_separator(),
        tool(Icon::ChevronUp, "Previous Page", Msg::PreviousPage, open && d.current_page() > 0),
        tool(Icon::ChevronDown, "Next Page", Msg::NextPage, open && d.current_page() + 1 < n),
        toolbar_separator(),
        tool(Icon::ZoomOut, "Zoom Out  ❖−", Msg::ZoomOut, open),
        dropdown(labels, selected)
            .compact()
            .enabled(open)
            .on_select(move |i| Msg::ZoomPick(w, i.wrapping_sub(offset)))
            .width(96.0)
            .into(),
        tool(Icon::ZoomIn, "Zoom In  ❖+", Msg::ZoomIn, open),
        toolbar_separator(),
        icon_button(Icon::Search)
            .toolbar()
            .selected(d.find.is_some())
            .enabled(open)
            .on_press(if d.find.is_some() { Msg::FindClose(w) } else { Msg::Find })
            .tooltip("Find  ❖F")
            .into(),
        spacer().into(),
        status,
        space(4.0),
    ])
}

fn find_bar(d: &Doc, f: &Find) -> Element<Msg> {
    let w = d.window;
    let count = f.count();
    let status = if f.query.is_empty() {
        String::new()
    } else if count == 0 {
        if f.done() { "Not found".into() } else { "Searching…".into() }
    } else {
        let more = if f.done() { "" } else { "+" };
        match f.ordinal() {
            Some(i) => format!("{i} of {count}{more}"),
            None => format!("{count}{more} found"),
        }
    };
    let field = search_field(&f.query)
        .placeholder("Find in document")
        .on_change(move |q| Msg::FindQuery(w, q))
        .on_submit(Msg::FindNext)
        .id("find")
        .width(260.0);
    container(
        row![
            field,
            icon_button(Icon::ChevronUp)
                .toolbar()
                .enabled(count > 0)
                .on_press(Msg::FindPrevious)
                .tooltip("Previous  ⇧❖G"),
            icon_button(Icon::ChevronDown).toolbar().enabled(count > 0).on_press(Msg::FindNext).tooltip("Next  ❖G"),
            checkbox("Match Case", f.case).on_toggle(move |on| Msg::FindCase(w, on)),
            spacer(),
            text(status).role(if count == 0 && f.done() && !f.query.is_empty() { Role::Danger } else { Role::Text2 }),
            button("Done").compact().on_press(Msg::FindClose(w)),
        ]
        .spacing(8.0),
    )
    .padding([4.0, 8.0])
    .fill(Tone::Chrome)
    .edges(Edge::BevelLo, Sides::BOTTOM)
    .width(Fill)
    .into()
}

// The sidebar.

fn sidebar(app: &Shiori, d: &Doc) -> Element<Msg> {
    let w = d.window;
    let has_outline = d.info().is_some_and(|i| !i.outline.is_empty());
    let tab = if has_outline { d.sidebar_tab } else { 0 };
    let content = if tab == 1 { contents(d) } else { thumbnails(app, d) };
    if !has_outline {
        return content;
    }
    column![
        container(segmented(["Pages", "Contents"], tab).on_select(move |t| Msg::SidebarTab(w, t)))
            .padding([6.0, 8.0])
            .center()
            .width(Fill),
        content,
    ]
    .into()
}

/// Each page small, the one being read selected; previews are asked for
/// as rows show.
fn thumbnails(app: &Shiori, d: &Doc) -> Element<Msg> {
    let w = d.window;
    let pw = (crate::app::PREVIEW * d.scale).round() as u32;
    let images: Rc<HashMap<usize, Image>> = Rc::new(
        (0..d.pages()).filter_map(|i| d.tiles.get(&d.layout.preview(i, pw)).map(|img| (i, img.clone()))).collect(),
    );
    let rows = d.thumbs.clone();
    let tx = app.stream.clone();
    let layout = d.layout.clone();
    let scale = d.scale;
    let n = d.pages();
    virtual_list(n, 176.0, move |i| -> Element<Msg> {
        // This row and those either side: kept, and drawn if they aren't.
        let ahead = i.saturating_sub(crate::app::THUMBS_AHEAD)..(i + crate::app::THUMBS_AHEAD + 1).min(n);
        let mut asked = false;
        {
            let mut t = rows.borrow_mut();
            for j in ahead.clone().filter(|j| *j != i).chain([i]) {
                t.saw(j as u32);
                if !images.contains_key(&j) && t.asked.insert(j as u32) {
                    asked = true;
                }
            }
        }
        if asked && let Some(tx) = &tx {
            tx.send(Msg::Thumb(w, i as u32));
        }
        let picture: Element<Msg> = match images.get(&i) {
            Some(img) => image(img.with_scale(1.0 / scale)).fit(Fit::Contain).width(Fill).height(Fill).into(),
            None => {
                let (pw, ph) = layout.pages[i];
                let h = 140.0_f64.min(112.0 * ph / pw.max(1.0));
                container(space(0.0)).fill(Tone::Raised).border(Edge::Line).width(h * pw / ph.max(1.0)).height(h).into()
            }
        };
        column![
            container(picture).center().width(Fill).height(148.0),
            text(format!("{}", i + 1)).role(Role::Text2).mono().tabular().center().width(Fill),
        ]
        .spacing(4.0)
        .padding([6.0, 8.0])
        .into()
    })
    .selected([d.current_page()])
    .reveal(d.current_page())
    .on_select(move |rows| rows.first().map_or(Msg::Nothing, |r| Msg::GoPage(w, *r)))
    .key("thumbnails")
    .into()
}

fn contents(d: &Doc) -> Element<Msg> {
    let w = d.window;
    let Some(info) = d.info() else { return space(0.0) };
    let mut targets = HashMap::new();
    let items = outline_items(&info.outline, &mut 1, &mut targets);
    tree(Arc::new(items))
        .expanded(Arc::new(d.expanded.clone()))
        .on_toggle(move |id, open| Msg::OutlineToggle(w, id, open))
        .on_activate(move |id| Msg::Outline(w, id))
        .on_select(move |ids| ids.first().map_or(Msg::Nothing, |id| Msg::Outline(w, *id)))
        .key("contents")
        .into()
}

/// The path of a `file://` URL.
pub fn file_path(url: &str) -> Option<PathBuf> {
    let rest = url.strip_prefix("file://")?;
    let rest = rest.strip_prefix("localhost").unwrap_or(rest);
    let bytes = rest.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%'
            && i + 2 < bytes.len()
            && let Ok(b) = u8::from_str_radix(std::str::from_utf8(&bytes[i + 1..i + 3]).ok()?, 16)
        {
            out.push(b);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    use std::os::unix::ffi::OsStringExt;
    Some(PathBuf::from(std::ffi::OsString::from_vec(out)))
}

// Sheets.

pub fn sheet(app: &Shiori, s: WindowKey, sheet: &Sheet) -> Element<Msg> {
    match sheet {
        Sheet::Password { parent, text: pw } => {
            let d = app.doc(*parent);
            let name = d.map(Doc::name).unwrap_or_default();
            let tried = d.is_some_and(|d| matches!(d.load, Load::Locked { tried: true }));
            column![
                text(format!("“{name}” is protected by a password")).semibold(),
                text("Enter its password to open it.").role(Role::Text2),
                text_field(pw)
                    .password()
                    .placeholder("Password")
                    .on_change(move |t| Msg::SheetText(s, t))
                    .on_submit(Msg::SheetOk(s))
                    .error(tried.then(|| "That isn't its password.".to_owned()))
                    .id("password")
                    .width(Fill),
                row![
                    spacer(),
                    button("Cancel").cancel().on_press(Msg::SheetCancel(s)),
                    button("Open").default().on_press(Msg::SheetOk(s))
                ]
                .spacing(8.0),
            ]
            .spacing(10.0)
            .padding(20.0)
            .width(380.0)
            .into()
        }
        Sheet::GoTo { parent, text: t, error } => {
            let n = app.doc(*parent).map_or(0, Doc::pages);
            column![
                text("Go to Page").semibold(),
                text_field(t)
                    .placeholder(format!("1–{n}"))
                    .on_change(move |t| Msg::SheetText(s, t))
                    .on_submit(Msg::SheetOk(s))
                    .error(error.then(|| format!("Pages go from 1 to {n}.")))
                    .id("goto")
                    .width(Fill),
                row![
                    spacer(),
                    button("Cancel").cancel().on_press(Msg::SheetCancel(s)),
                    button("Go").default().on_press(Msg::SheetOk(s))
                ]
                .spacing(8.0),
            ]
            .spacing(10.0)
            .padding(20.0)
            .width(300.0)
            .into()
        }
        Sheet::Info { parent } => {
            let Some(d) = app.doc(*parent) else { return space(0.0) };
            let Some(info) = d.info() else { return space(0.0) };
            let mut rows: Vec<(String, String)> = Vec::new();
            for key in ["Title", "Author", "Subject", "Keywords"] {
                if let Some(v) = info.meta(key) {
                    rows.push((key.to_owned(), v.to_owned()));
                }
            }
            rows.push(("Pages".into(), info.pages.len().to_string()));
            if let Some((w, h)) = info.pages.first() {
                rows.push(("Page size".into(), page_size(*w, *h)));
            }
            if let Ok(m) = std::fs::metadata(&d.path) {
                rows.push(("Size".into(), file_size(m.len())));
            }
            if !info.version.is_empty() {
                rows.push(("PDF version".into(), info.version.clone()));
            }
            for (key, label) in [("Creator", "Made with"), ("Producer", "Written by")] {
                if let Some(v) = info.meta(key) {
                    rows.push((label.into(), v.to_owned()));
                }
            }
            if info.encrypted {
                rows.push((
                    "Security".into(),
                    if info.can_copy { "Encrypted".into() } else { "Encrypted; copying not allowed".into() },
                ));
            }
            if info.forms {
                rows.push(("Forms".into(), "Has fields".into()));
            }
            rows.push(("Where".into(), d.path.parent().map(|p| p.display().to_string()).unwrap_or_default()));
            let grid: Vec<Element<Msg>> = rows
                .into_iter()
                .map(|(k, v)| {
                    row![text(k).role(Role::Text2).width(96.0), text(v).wrap().selectable().width(Fill)]
                        .spacing(12.0)
                        .align(Align::Start)
                        .into()
                })
                .collect();
            column![
                text(d.name()).semibold(),
                aizome_kit::widgets::layout::column(grid).spacing(6.0),
                row![spacer(), button("OK").default().on_press(Msg::SheetOk(s))],
            ]
            .spacing(12.0)
            .padding(20.0)
            .width(440.0)
            .into()
        }
    }
}

/// `8.5 × 11 in (Letter)`, `210 × 297 mm (A4)`, else the points.
pub fn page_size(w: f32, h: f32) -> String {
    let named = [
        ("Letter", 612.0, 792.0),
        ("Legal", 612.0, 1008.0),
        ("A4", 595.0, 842.0),
        ("A3", 842.0, 1191.0),
        ("A5", 420.0, 595.0),
    ];
    let close = |a: f32, b: f32| (a - b).abs() <= 2.0;
    for (name, nw, nh) in named {
        if (close(w, nw) && close(h, nh)) || (close(w, nh) && close(h, nw)) {
            return if name.starts_with('A') {
                format!("{:.0} × {:.0} mm ({name})", w / 72.0 * 25.4, h / 72.0 * 25.4)
            } else {
                format!("{} × {} in ({name})", trim(w / 72.0), trim(h / 72.0))
            };
        }
    }
    format!("{:.0} × {:.0} pt", w, h)
}

fn trim(v: f32) -> String {
    let s = format!("{v:.2}");
    s.trim_end_matches('0').trim_end_matches('.').to_owned()
}

pub fn file_size(n: u64) -> String {
    const K: f64 = 1000.0;
    let f = n as f64;
    if f < K {
        format!("{n} bytes")
    } else if f < K * K {
        format!("{:.0} KB", f / K)
    } else if f < K * K * K {
        format!("{:.1} MB", f / K / K)
    } else {
        format!("{:.2} GB", f / K / K / K)
    }
}

// Menus.

pub fn menus(app: &Shiori) -> MenuBar<Msg> {
    let d = app.focused_doc();
    let open = d.is_some_and(|d| d.info().is_some());
    let any = d.is_some();
    let selected = d.is_some_and(|d| d.selection.is_some_and(|s| !s.is_empty()));
    let can_copy = selected && d.and_then(Doc::info).is_some_and(|i| i.can_copy);
    let finding = d.is_some_and(|d| d.find.as_ref().is_some_and(|f| f.count() > 0));
    let zoom = d.map(|d| d.zoom);
    let n = d.map_or(0, Doc::pages);
    let page = d.map_or(0, Doc::current_page);
    let file = Menu::new("File")
        .item(MenuItem::new("Open…", Msg::OpenPanel).shortcut(Shortcut::cmd('O')))
        .item(MenuItem::new("Reload", Msg::Reload).shortcut(Shortcut::cmd('R')).enabled(any))
        .separator()
        .item(MenuItem::new("Show in Hako", Msg::ShowInHako).shortcut(Shortcut::cmd('R').alt()).enabled(any))
        .item(MenuItem::new("Get Info", Msg::Info).shortcut(Shortcut::cmd('I')).enabled(open))
        .separator()
        .item(MenuItem::role_item("Close Window", MenuRole::CloseWindow).shortcut(Shortcut::cmd('W')));
    let edit = Menu::new("Edit")
        .item(MenuItem::new("Copy", Msg::Copy).role(MenuRole::Copy).shortcut(Shortcut::cmd('C')).enabled(can_copy))
        .item(
            MenuItem::new("Select All", Msg::SelectAll)
                .role(MenuRole::SelectAll)
                .shortcut(Shortcut::cmd('A'))
                .enabled(open),
        )
        .separator()
        .submenu(
            Menu::new("Find")
                .item(MenuItem::new("Find…", Msg::Find).role(MenuRole::Find).shortcut(Shortcut::cmd('F')).enabled(open))
                .item(
                    MenuItem::new("Find Next", Msg::FindNext)
                        .role(MenuRole::FindNext)
                        .shortcut(Shortcut::cmd('G'))
                        .enabled(finding),
                )
                .item(
                    MenuItem::new("Find Previous", Msg::FindPrevious)
                        .role(MenuRole::FindPrevious)
                        .shortcut(Shortcut::cmd('G').shift())
                        .enabled(finding),
                ),
        );
    let is = |z: Zoom| zoom == Some(z);
    let view = Menu::new("View")
        .item(
            MenuItem::new("Show Sidebar", Msg::ToggleSidebar)
                .shortcut(Shortcut::cmd('S').alt())
                .enabled(open)
                .checked(d.is_some_and(|d| d.sidebar)),
        )
        .separator()
        .item(MenuItem::new("Zoom In", Msg::ZoomIn).shortcut(Shortcut::cmd('+')).enabled(open))
        .item(MenuItem::new("Zoom Out", Msg::ZoomOut).shortcut(Shortcut::cmd('-')).enabled(open))
        .item(
            MenuItem::new("Actual Size", Msg::ActualSize)
                .shortcut(Shortcut::cmd('0'))
                .enabled(open)
                .checked(is(Zoom::Fixed(1.0))),
        )
        .item(
            MenuItem::new("Fit Width", Msg::FitWidth)
                .shortcut(Shortcut::cmd('8'))
                .enabled(open)
                .checked(is(Zoom::FitWidth)),
        )
        .item(
            MenuItem::new("Fit Page", Msg::FitPage)
                .shortcut(Shortcut::cmd('9'))
                .enabled(open)
                .checked(is(Zoom::FitPage)),
        )
        .separator()
        .item(
            MenuItem::new(
                if d.is_some_and(|d| d.fullscreen) { "Exit Full Screen" } else { "Enter Full Screen" },
                Msg::FullScreen,
            )
            .shortcut(Shortcut::cmd('F').ctrl())
            .enabled(any),
        );
    let go = Menu::new("Go")
        .item(
            MenuItem::new("Previous Page", Msg::PreviousPage)
                .shortcut(Shortcut::cmd_key(ShortcutKey::Up).alt())
                .enabled(open && page > 0),
        )
        .item(
            MenuItem::new("Next Page", Msg::NextPage)
                .shortcut(Shortcut::cmd_key(ShortcutKey::Down).alt())
                .enabled(open && page + 1 < n),
        )
        .separator()
        .item(MenuItem::new("First Page", Msg::FirstPage).shortcut(Shortcut::cmd_key(ShortcutKey::Up)).enabled(open))
        .item(MenuItem::new("Last Page", Msg::LastPage).shortcut(Shortcut::cmd_key(ShortcutKey::Down)).enabled(open))
        .separator()
        .item(MenuItem::new("Go to Page…", Msg::GoToPanel).shortcut(Shortcut::cmd('G').alt()).enabled(open));
    MenuBar::standard(&aizome_kit::AppInfo::new(crate::app::APP_ID, "Shiori", "Documents"))
        .replace("Edit", edit)
        .with_file_menu(file)
        .with_menu(view)
        .with_menu(go)
}

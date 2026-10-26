//! Desktop & Deskbar: the Desks (names, folders, added and removed), the
//! Deskbar (folded, its pinned apps, the Shelf), the desktop's icons, and
//! how windows behave (stacking, a tab's double-click, focus, the command
//! key everywhere). `[desks]`, `[deskbar]`, `[desktop]`, `[windows]` and
//! `[keys]`, applied by the compositor and the shell.

use std::path::PathBuf;

use aizome_config::{DeskConfig, FocusFollows};
use aizome_kit::prelude::*;
use aizome_kit::row;
use aizome_sys::apps::AppIndex;

use crate::extra;
use crate::form;
use crate::pane::{Confirm, Group, Pane, PaneCx, SettingEntry, Tile, ViewCx};

#[derive(Default)]
pub struct Desktop {
    /// Desks' names as typed, before Return.
    drafts: std::collections::HashMap<usize, String>,
    /// Apps that can be pinned, as (app id, name).
    apps: Option<Vec<(String, String)>>,
}

#[derive(Clone, Debug)]
pub enum Msg {
    Apps(Vec<(String, String)>),
    DeskName(usize, String),
    SaveDesk(usize),
    ChooseFolder(usize),
    Folder(usize, Option<Vec<PathBuf>>),
    ClearFolder(usize),
    AddDesk,
    RemoveDesk(usize),
    RemoveConfirmed(usize),
    Collapsed(bool),
    AutoHide(bool),
    Shelf(bool),
    Pin(usize),
    Unpin(String),
    Icon(&'static str, bool),
    Stack(bool),
    DoubleClick(usize),
    FocusFollows(bool),
    Remember(bool),
    CommandKey(bool),
}

/// A change to the Desks, made inside a settings change.
type DeskEdit = Box<dyn FnOnce(&mut Vec<DeskConfig>)>;

pub const DOUBLE_CLICK: [(&str, &str); 3] =
    [("fill", "Fill the screen"), ("minimise", "Minimise"), ("nothing", "Nothing")];

fn sample_apps() -> Vec<(String, String)> {
    [
        ("org.aizome.Hako", "Hako"),
        ("org.aizome.Sumi", "Sumi"),
        ("foot", "Foot"),
        ("org.mozilla.firefox", "Firefox"),
        ("org.aizome.Fusen", "Fusen"),
    ]
    .iter()
    .map(|(a, n)| ((*a).to_owned(), (*n).to_owned()))
    .collect()
}

impl Pane for Desktop {
    type Msg = Msg;

    fn id(&self) -> &'static str {
        "desktop"
    }

    fn title(&self) -> &'static str {
        "Desktop & Deskbar"
    }

    fn icon(&self) -> Icon {
        Icon::LayoutDashboard
    }

    fn tile(&self) -> Tile {
        Tile::Ai
    }

    fn group(&self) -> Group {
        Group::System
    }

    fn keywords(&self) -> &'static [&'static str] {
        &["desks", "workspaces", "dock", "taskbar", "windows", "virtual desktops"]
    }

    fn entries(&self) -> Vec<SettingEntry> {
        vec![
            SettingEntry::new("desks", "Desks").keywords(&["workspaces", "virtual desktops", "rename", "folder"]),
            SettingEntry::new("collapsed", "Fold the Deskbar").keywords(&["hide deskbar", "collapse", "apps"]),
            SettingEntry::new("auto-hide", "Automatically hide the Deskbar").keywords(&["auto hide", "dock", "apps"]),
            SettingEntry::new("pinned", "Apps kept in the Deskbar").keywords(&["pin", "favourites", "keep in deskbar"]),
            SettingEntry::new("shelf", "Show the Shelf").keywords(&["drag", "park", "shelf"]),
            SettingEntry::new("desktop-icons", "Icons on the desktop").keywords(&["home", "disks", "trash"]),
            SettingEntry::new("stack-new", "Stack new windows of the same app").keywords(&["tabs", "tab groups"]),
            SettingEntry::new("double-click", "Double-click a tab").keywords(&[
                "zoom",
                "maximise",
                "minimise",
                "title bar",
            ]),
            SettingEntry::new("focus-follows", "Focus follows the pointer").keywords(&[
                "sloppy focus",
                "mouse focus",
                "click to focus",
            ]),
            SettingEntry::new("remember", "Reopen windows where they were").keywords(&["geometry", "position", "size"]),
            SettingEntry::new("command-key", "Command key for all apps").keywords(&[
                "super",
                "ctrl",
                "copy paste",
                "translate",
            ]),
        ]
    }

    fn shown(&mut self, cx: &mut PaneCx<Msg>) {
        if self.apps.is_some() {
            return;
        }
        if cx.sample() {
            self.apps = Some(sample_apps());
        } else {
            cx.spawn(|| {
                let index = AppIndex::load();
                let mut v: Vec<(String, String)> =
                    index.apps().into_iter().map(|a| (a.app_id().to_owned(), a.name.clone())).collect();
                v.sort_by_key(|(_, n)| n.to_lowercase());
                Msg::Apps(v)
            });
        }
    }

    fn update(&mut self, msg: Msg, cx: &mut PaneCx<Msg>) {
        let desks = |cx: &mut PaneCx<Msg>, f: DeskEdit| {
            cx.change(move |s| {
                let mut d = s.desks_or_default();
                f(&mut d);
                s.desks = d;
            });
        };
        match msg {
            Msg::Apps(a) => self.apps = Some(a),
            Msg::DeskName(i, s) => {
                self.drafts.insert(i, s);
            }
            Msg::SaveDesk(i) => {
                let Some(name) = self.drafts.remove(&i) else { return };
                let name = name.trim().to_owned();
                if name.is_empty() {
                    return;
                }
                desks(
                    cx,
                    Box::new(move |d| {
                        if let Some(desk) = d.get_mut(i) {
                            desk.name = name;
                        }
                    }),
                );
            }
            Msg::ChooseFolder(i) => {
                let dialog =
                    FileDialog::new().title("The Desk's Folder").directory(aizome_config::home_dir()).directories();
                cx.open_panel(dialog, move |p| Msg::Folder(i, p));
            }
            Msg::Folder(i, Some(paths)) => {
                let Some(p) = paths.into_iter().next() else { return };
                let p = super::search::tilde(&p.display().to_string());
                desks(
                    cx,
                    Box::new(move |d| {
                        if let Some(desk) = d.get_mut(i) {
                            desk.folder = Some(p);
                        }
                    }),
                );
            }
            Msg::Folder(_, None) => {}
            Msg::ClearFolder(i) => desks(
                cx,
                Box::new(move |d| {
                    if let Some(desk) = d.get_mut(i) {
                        desk.folder = None;
                    }
                }),
            ),
            Msg::AddDesk => desks(
                cx,
                Box::new(|d| {
                    let n = d.len() + 1;
                    d.push(DeskConfig::named(n.to_string()));
                }),
            ),
            Msg::RemoveDesk(i) => {
                let name = cx.settings().desks_or_default().get(i).map(|d| d.name.clone()).unwrap_or_default();
                cx.confirm(
                    Confirm::new(
                        format!("Remove the Desk “{name}”?"),
                        "Its windows move to the Desk before it.",
                        "Remove",
                        Msg::RemoveConfirmed(i),
                    )
                    .destructive(),
                );
            }
            Msg::RemoveConfirmed(i) => {
                self.drafts.clear();
                desks(
                    cx,
                    Box::new(move |d| {
                        if d.len() > 1 && i < d.len() {
                            d.remove(i);
                        }
                    }),
                );
            }
            Msg::Collapsed(on) => cx.change(|s| {
                let mut d = s.deskbar_settings();
                d.collapsed = on;
                s.set_shell_section("deskbar", &d);
            }),
            Msg::AutoHide(on) => cx.change(|s| {
                let mut d = s.deskbar_settings();
                d.auto_hide = on;
                s.set_shell_section("deskbar", &d);
            }),
            Msg::Shelf(on) => cx.change(|s| extra::set(&mut s.deskbar, "show_shelf", on)),
            Msg::Pin(i) => {
                let Some((id, _)) = self.apps.as_ref().and_then(|a| a.get(i)).cloned() else { return };
                cx.change(|s| {
                    let mut d = s.deskbar_settings();
                    if !d.pinned.contains(&id) {
                        d.pinned.push(id);
                    }
                    s.set_shell_section("deskbar", &d);
                });
            }
            Msg::Unpin(id) => cx.change(|s| {
                let mut d = s.deskbar_settings();
                d.pinned.retain(|x| *x != id);
                s.set_shell_section("deskbar", &d);
            }),
            Msg::Icon(key, on) => cx.change(|s| extra::set(extra::section(s, "desktop"), key, on)),
            Msg::Stack(on) => cx.change(|s| s.windows.stack_new_windows_of_same_app = on),
            Msg::DoubleClick(i) => {
                cx.change(|s| extra::set(&mut s.windows.extra, "tab_double_click", DOUBLE_CLICK[i.min(2)].0))
            }
            Msg::FocusFollows(on) => {
                cx.change(|s| s.windows.focus_follows = if on { FocusFollows::Mouse } else { FocusFollows::Click })
            }
            Msg::Remember(on) => cx.change(|s| s.windows.remember_geometry = on),
            Msg::CommandKey(on) => cx.change(|s| s.keys.command_key_for_all_apps = on),
        }
    }

    fn view(&self, vcx: &ViewCx) -> Element<Msg> {
        let s = vcx.settings();
        let list = s.desks_or_default();
        let n = list.len();
        let mut desk_rows: Vec<form::Row<Msg>> = list
            .iter()
            .enumerate()
            .map(|(i, d)| {
                let name = self.drafts.get(&i).cloned().unwrap_or_else(|| d.name.clone());
                let folder: Element<Msg> = match &d.folder {
                    Some(f) => row![
                        button(f.clone()).icon(Icon::Folder).plain().compact().on_press(Msg::ChooseFolder(i)),
                        icon_button(Icon::X).toolbar().on_press(Msg::ClearFolder(i)).tooltip("No folder"),
                    ]
                    .spacing(2.0)
                    .align(Align::Center)
                    .into(),
                    None => button("Choose a Folder…").plain().compact().on_press(Msg::ChooseFolder(i)).into(),
                };
                let number = container(text(format!("{}", i + 1)).mono().size(11.0).weight(650.0))
                    .center()
                    .width(22.0)
                    .height(22.0)
                    .fill(Tone::Sunk)
                    .border(Edge::Line2);
                let line = row![
                    number,
                    text_field(&name)
                        .on_change(move |t| Msg::DeskName(i, t))
                        .on_submit(Msg::SaveDesk(i))
                        .on_blur(Msg::SaveDesk(i))
                        .width(180.0),
                    spacer(),
                    folder,
                    icon_button(Icon::Minus).toolbar().enabled(n > 1).on_press(Msg::RemoveDesk(i)).tooltip("Remove"),
                ]
                .spacing(10.0)
                .align(Align::Center)
                .width(Fill);
                form::whole(container(line).padding([6.0, form::INSET]).width(Fill))
            })
            .collect();
        let first = desk_rows.remove(0);
        desk_rows.insert(0, first.entry("desks"));
        desk_rows.push(form::whole(
            container(
                row![button("Add a Desk").icon(Icon::Plus).enabled(n < 9).on_press(Msg::AddDesk), spacer()].width(Fill),
            )
            .padding([8.0, form::INSET])
            .width(Fill),
        ));

        let db = s.deskbar_settings();
        let names = self.apps.clone().unwrap_or_default();
        let name_of = |id: &str| names.iter().find(|(a, _)| a == id).map_or_else(|| id.to_owned(), |(_, n)| n.clone());
        let mut deskbar = vec![
            form::row("Fold the Deskbar", toggle(db.collapsed).on_toggle(Msg::Collapsed))
                .entry("collapsed")
                .help("Folded, it's the menu bar's “N apps ⌄”, which opens it."),
            form::row("Automatically hide the Deskbar", toggle(db.auto_hide).on_toggle(Msg::AutoHide))
                .entry("auto-hide")
                .help("Point at the menu bar's apps control to reveal it. It hides after the pointer leaves."),
            form::row("Show the Shelf", toggle(extra::get_bool(&s.deskbar, "show_shelf", true)).on_toggle(Msg::Shelf))
                .entry("shelf")
                .help("A place to park what you're dragging: files, text, pictures."),
        ];
        let pin_list: Vec<String> =
            names.iter().filter(|(id, _)| !db.pinned.contains(id)).map(|(_, n)| n.clone()).collect();
        let unpinned: Vec<usize> =
            names.iter().enumerate().filter(|(_, (id, _))| !db.pinned.contains(id)).map(|(i, _)| i).collect();
        let pins: Element<Msg> = {
            let mut col = aizome_kit::column![].spacing(4.0).width(Fill);
            for id in &db.pinned {
                let id2 = id.clone();
                col = col.push(
                    row![
                        icon(crate::widgets::app_glyph(id)).size(14.0).role(Role::Text2),
                        text(name_of(id)).width(Fill),
                        icon_button(Icon::Minus).toolbar().on_press(Msg::Unpin(id2)).tooltip("Unpin"),
                    ]
                    .spacing(8.0)
                    .align(Align::Center),
                );
            }
            col = col.push(
                dropdown(pin_list, None)
                    .placeholder("Keep an app in the Deskbar…")
                    .on_select(move |i| Msg::Pin(unpinned.get(i).copied().unwrap_or(usize::MAX)))
                    .min_width(220.0),
            );
            col.into()
        };
        deskbar.push(form::row("Kept in the Deskbar", pins).entry("pinned").stacked().help(
            "Pinned apps stay listed while closed, ready to open. Drag app rows in the Deskbar to arrange them.",
        ));

        let desktop = extra::section_ref(s, "desktop");
        let icons = vec![
            form::row(
                "Home",
                toggle(extra::get_bool(desktop, "show_home", true)).on_toggle(|on| Msg::Icon("show_home", on)),
            )
            .leading(icon(Icon::House).size(16.0).role(Role::Accent))
            .entry("desktop-icons")
            .compact(),
            form::row(
                "Disks and disk images",
                toggle(extra::get_bool(desktop, "show_disks", true)).on_toggle(|on| Msg::Icon("show_disks", on)),
            )
            .leading(icon(Icon::HardDrive).size(16.0).role(Role::Accent))
            .compact(),
            form::row(
                "Trash",
                toggle(extra::get_bool(desktop, "show_trash", true)).on_toggle(|on| Msg::Icon("show_trash", on)),
            )
            .leading(icon(Icon::Trash).size(16.0).role(Role::Accent))
            .compact(),
        ];

        let w = &s.windows;
        let dc = extra::get_str(&w.extra, "tab_double_click").unwrap_or("fill");
        let dc_i = DOUBLE_CLICK.iter().position(|(k, _)| *k == dc).unwrap_or(0);
        let windows = vec![
            form::row(
                "Stack new windows of the same app",
                toggle(w.stack_new_windows_of_same_app).on_toggle(Msg::Stack),
            )
            .entry("stack-new")
            .help("A new window joins the app's window as a tab, as in Haiku."),
            form::row("Double-click a tab to", form::choice(&DOUBLE_CLICK.map(|d| d.1), dc_i, Msg::DoubleClick))
                .entry("double-click"),
            form::row(
                "Focus follows the pointer",
                toggle(w.focus_follows == FocusFollows::Mouse).on_toggle(Msg::FocusFollows),
            )
            .entry("focus-follows")
            .help("The window under the pointer takes the keyboard, without coming to the front."),
            form::row("Reopen windows where they were", toggle(w.remember_geometry).on_toggle(Msg::Remember))
                .entry("remember"),
            form::row("Command key for all apps", toggle(s.keys.command_key_for_all_apps).on_toggle(Msg::CommandKey))
                .entry("command-key")
                .help("❖C, ❖V and the rest work in apps that expect Ctrl too (Ctrl+Shift in terminals)."),
        ];

        form::page(vec![
            vcx.section_with(
                "Desks",
                desk_rows,
                Some("A Desk's folder is where ❖Return opens a terminal and where the palette looks first.".into()),
            ),
            vcx.section("Deskbar", deskbar),
            vcx.section("Desktop Icons", icons),
            vcx.section("Windows", windows),
        ])
    }
}

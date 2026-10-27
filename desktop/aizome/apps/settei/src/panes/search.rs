//! Search: what the command palette (❖Space) looks through, which folders
//! its file index covers and skips, and forgetting what it has learnt.
//! `[palette]`, which the shell follows; the index and the history are the
//! shell's files, which this pane only removes.

use std::path::PathBuf;
use std::time::SystemTime;

use aizome_kit::prelude::*;
use aizome_kit::row;

use crate::extra;
use crate::form;
use crate::pane::{Confirm, Group, Pane, PaneCx, SettingEntry, Tile, ViewCx};

/// The palette's scopes, as the shell names them, and what they find.
pub const SCOPES: [(&str, &str, &str, Icon); 6] = [
    ("commands", "Commands", "Every menu item of the app in front", Icon::Menu),
    ("windows", "Windows", "Open windows, on any Desk", Icon::AppWindow),
    ("files", "Files", "Files and folders in the folders below", Icon::File),
    ("apps", "Apps", "Installed apps", Icon::LayoutGrid),
    ("settings", "Settings", "Every pane and option here", Icon::Settings),
    ("actions", "Actions", "Lock, Sleep, Night and Day, Screenshot…", Icon::Zap),
];

#[derive(Default)]
pub struct Search {
    index: Option<(u64, Option<SystemTime>)>,
}

#[derive(Clone, Debug)]
pub enum Msg {
    Scope(&'static str, bool),
    AddFolder(bool),
    Added(bool, Option<Vec<PathBuf>>),
    RemoveFolder(bool, String),
    Rebuild,
    Forget,
    Forgotten,
    Index(u64, Option<SystemTime>),
}

fn index_path() -> PathBuf {
    aizome_search::files::FileIndex::default_cache_path()
}

/// "~/work" for a path in the home folder.
pub fn tilde(path: &str) -> String {
    let home = aizome_config::home_dir();
    let home = home.to_string_lossy();
    match path.strip_prefix(home.as_ref()) {
        Some("") => "~".into(),
        Some(rest) if rest.starts_with('/') => format!("~{rest}"),
        _ => path.to_owned(),
    }
}

fn ago(t: SystemTime) -> String {
    let s = SystemTime::now().duration_since(t).map_or(0, |d| d.as_secs());
    match s {
        0..=89 => "just now".into(),
        90..=5399 => format!("{} minutes ago", (s + 30) / 60),
        5400..=129_599 => format!("{} hours ago", (s + 1800) / 3600),
        _ => format!("{} days ago", s / 86400),
    }
}

impl Pane for Search {
    type Msg = Msg;

    fn id(&self) -> &'static str {
        "search"
    }

    fn title(&self) -> &'static str {
        "Search"
    }

    fn icon(&self) -> Icon {
        Icon::Search
    }

    fn tile(&self) -> Tile {
        Tile::Violet
    }

    fn group(&self) -> Group {
        Group::System
    }

    fn keywords(&self) -> &'static [&'static str] {
        &["palette", "spotlight", "find", "index"]
    }

    fn entries(&self) -> Vec<SettingEntry> {
        vec![
            SettingEntry::new("scopes", "What the palette finds").keywords(&["scopes", "categories", "results"]),
            SettingEntry::new("folders", "Folders to search").keywords(&["file index", "locations", "roots"]),
            SettingEntry::new("exclude", "Folders to skip").keywords(&["exclusions", "privacy", "ignore"]),
            SettingEntry::new("rebuild", "Rebuild the file index").keywords(&["reindex", "refresh"]),
            SettingEntry::new("forget", "Forget the palette's history").keywords(&[
                "clear history",
                "frecency",
                "recent",
                "privacy",
            ]),
        ]
    }

    fn shown(&mut self, cx: &mut PaneCx<Msg>) {
        if cx.sample() {
            self.index = Some((48_213_504, Some(SystemTime::now() - std::time::Duration::from_secs(420))));
            return;
        }
        cx.spawn(|| {
            let m = std::fs::metadata(index_path()).ok();
            Msg::Index(m.as_ref().map_or(0, |m| m.len()), m.and_then(|m| m.modified().ok()))
        });
    }

    fn update(&mut self, msg: Msg, cx: &mut PaneCx<Msg>) {
        match msg {
            Msg::Scope(id, on) => cx.change(|s| {
                let p = extra::section(s, "palette");
                let mut list = extra::get_list(p, "scopes").unwrap_or_else(|| SCOPES.iter().map(|s| s.0.to_owned()).collect());
                list.retain(|x| x != id);
                if on {
                    list.push(id.to_owned());
                }
                list.sort_by_key(|x| SCOPES.iter().position(|s| s.0 == x).unwrap_or(99));
                extra::set_list(p, "scopes", &list);
            }),
            Msg::AddFolder(skip) => {
                let dialog = FileDialog::new().title(if skip { "Skip a Folder" } else { "Search a Folder" }).directory(aizome_config::home_dir()).directories();
                cx.open_panel(dialog, move |p| Msg::Added(skip, p));
            }
            Msg::Added(skip, Some(paths)) => {
                let Some(p) = paths.into_iter().next() else { return };
                let p = tilde(&p.display().to_string());
                cx.change(|s| {
                    if skip {
                        let t = extra::section(s, "palette");
                        let mut l = extra::get_list(t, "exclude").unwrap_or_default();
                        if !l.contains(&p) {
                            l.push(p);
                        }
                        extra::set_list(t, "exclude", &l);
                    } else {
                        let mut ps = s.palette_settings();
                        if !ps.roots.contains(&p) {
                            ps.roots.push(p);
                        }
                        s.set_shell_section("palette", &ps);
                    }
                });
            }
            Msg::Added(_, None) => {}
            Msg::RemoveFolder(skip, p) => cx.change(|s| {
                if skip {
                    let t = extra::section(s, "palette");
                    let mut l = extra::get_list(t, "exclude").unwrap_or_default();
                    l.retain(|x| *x != p);
                    extra::set_list(t, "exclude", &l);
                } else {
                    let mut ps = s.palette_settings();
                    ps.roots.retain(|x| *x != p);
                    s.set_shell_section("palette", &ps);
                }
            }),
            Msg::Rebuild => {
                if !cx.sample() {
                    let _ = std::fs::remove_file(index_path());
                }
                self.index = Some((0, None));
                cx.say("The palette indexes the folders again the next time it opens");
            }
            Msg::Forget => cx.confirm(
                Confirm::new(
                    "Forget what the palette has learnt?",
                    "It forgets which files, apps and commands you choose most, and the commands it counted for the shortcut coach.",
                    "Forget",
                    Msg::Forgotten,
                )
                .destructive(),
            ),
            Msg::Forgotten => {
                if !cx.sample() {
                    let _ = std::fs::remove_file(aizome_search::frecency::Frecency::default_path());
                }
                cx.say("The palette's history is gone");
            }
            Msg::Index(size, when) => self.index = Some((size, when)),
        }
    }

    fn view(&self, vcx: &ViewCx) -> Element<Msg> {
        let s = vcx.settings();
        let palette = extra::section_ref(s, "palette");
        let on: Vec<String> =
            extra::get_list(palette, "scopes").unwrap_or_else(|| SCOPES.iter().map(|s| s.0.to_owned()).collect());
        let scopes: Vec<form::Row<Msg>> = SCOPES
            .iter()
            .map(|(id, title, about, glyph)| {
                let id: &'static str = id;
                form::row(*title, toggle(on.iter().any(|x| x == id)).on_toggle(move |v| Msg::Scope(id, v)))
                    .leading(icon(*glyph).size(16.0).role(Role::Text2))
                    .detail(*about)
                    .compact()
            })
            .collect();
        let mut scopes = scopes;
        let first = scopes.remove(0);
        scopes.insert(0, first.entry("scopes"));

        let folder_rows = |list: &[String], skip: bool, entry: &str, empty: &str| -> Vec<form::Row<Msg>> {
            let mut rows: Vec<form::Row<Msg>> = list
                .iter()
                .map(|p| {
                    let p2 = p.clone();
                    form::row(
                        if p == "~" { "Home (~)".to_owned() } else { p.clone() },
                        icon_button(Icon::Minus).toolbar().on_press(Msg::RemoveFolder(skip, p2)).tooltip("Remove"),
                    )
                    .leading(icon(if skip { Icon::EyeOff } else { Icon::Folder }).size(16.0).role(Role::Text3))
                    .compact()
                })
                .collect();
            if rows.is_empty() {
                rows.push(form::whole(
                    container(text(empty.to_owned()).role(Role::Text3)).padding([10.0, form::INSET]),
                ));
            }
            let first = rows.remove(0);
            rows.insert(0, first.entry(entry.to_owned()));
            rows.push(form::whole(
                container(
                    row![
                        button(if skip { "Skip a Folder…" } else { "Add a Folder…" })
                            .icon(Icon::Plus)
                            .on_press(Msg::AddFolder(skip)),
                        spacer()
                    ]
                    .width(Fill),
                )
                .padding([8.0, form::INSET])
                .width(Fill),
            ));
            rows
        };
        let roots = s.palette_settings().roots;
        let exclude = extra::get_list(palette, "exclude").unwrap_or_default();
        let status = match self.index {
            None => "Looking at the index…".to_owned(),
            Some((0, _)) => "Not indexed yet: the palette builds the index when it next opens.".to_owned(),
            Some((size, when)) => {
                format!("{:.1} MB, made {}", size as f64 / 1e6, when.map(ago).unwrap_or_else(|| "a while ago".into()))
            }
        };
        form::page(vec![
            vcx.section("Find in the Palette", scopes),
            vcx.section_with(
                "Search These Folders",
                folder_rows(&roots, false, "folders", "No folders: Files finds nothing."),
                Some("Build outputs, caches and what .gitignore files name are skipped by themselves.".into()),
            ),
            vcx.section(
                "Skip These Folders",
                folder_rows(&exclude, true, "exclude", "Nothing skipped beyond the usual."),
            ),
            vcx.section(
                "Index and History",
                vec![
                    form::row("File index", button("Rebuild").on_press(Msg::Rebuild)).entry("rebuild").help(status),
                    form::row("What the palette has learnt", button("Forget…").destructive().on_press(Msg::Forget))
                        .entry("forget")
                        .help("The palette puts what you choose often first. Forgetting starts it afresh."),
                ],
            ),
        ])
    }
}

//! The shell's menus: the focused app's (published, or the fallback app
//! and Window menus), Aizome's own when no window has focus, the
//! Toolchest, the NeXT menu at the pointer, the Desks' and Deskbar's
//! context menus, and Services; and the kit menus they're shown as.

use std::path::PathBuf;

use aizome_ipc::{AppSel, DeskInfo, Request, Side, WindowInfo};
use aizome_kit::Icon;
use aizome_search::Scope;
use aizome_shell_proto as proto;
use aizome_sys::apps::App;

use super::model::{Entry, Kind, Menu, MenuIcon, sc};
use crate::actions::{Action, ServiceVerb, SessionVerb};
use crate::apps::catalog;

/// The roles the shell carries out itself rather than asking the app.
pub fn shell_role(role: &str, pid: Option<i32>) -> Option<Action> {
    let app = AppSel { app_id: None, pid };
    Some(match role {
        "hide" => Action::Wm(Request::HideApp { app }),
        "hide-others" => Action::Wm(Request::HideOthers),
        "show-all" => Action::Wm(Request::ShowApp { app }),
        "minimise" => Action::Wm(Request::Minimise { id: None }),
        "fill" => Action::Wm(Request::Fill { id: None, state: None }),
        "stack-with" => Action::Wm(Request::Action { action: "stack-with-behind".into() }),
        "bring-all-to-front" => Action::Wm(Request::ShowApp { app }),
        // Help: the palette on the app's commands, and the tour.
        "help-search" => Action::Palette { scope: Some(Scope::Command), query: String::new() },
        "tour" => Action::Spawn { argv: vec!["aizome-annai".into()] },
        _ => return None,
    })
}

/// An app's published menus, roles the shell carries out rewired.
pub fn app_menus(published: &[proto::Menu], pid: u32) -> Vec<Menu> {
    published
        .iter()
        .map(|m| {
            let mut menu = super::model::from_proto(m, pid);
            rewire(&mut menu.items, Some(pid as i32));
            menu
        })
        .collect()
}

fn rewire(items: &mut [Entry], pid: Option<i32>) {
    for e in items {
        if let Some(a) = e.role.as_deref().and_then(|r| shell_role(r, pid)) {
            e.action = a;
        }
        rewire(&mut e.items, pid);
    }
}

/// Desks as a submenu's items: go to each (the current checked).
pub fn desk_items(desks: &[DeskInfo], go: impl Fn(u32) -> Action) -> Vec<Entry> {
    desks
        .iter()
        .map(|d| {
            let label = if d.name == d.index.to_string() {
                format!("Desk {}", d.index)
            } else {
                format!("{}  {}", d.index, d.name)
            };
            let mut e =
                Entry::action(label, go(d.index)).checked(d.current).with_id(super::model::hash_id(&("desk", d.id)));
            if d.index <= 9 {
                e = e.shortcut(sc("⌃❖", &d.index.to_string()));
            }
            e
        })
        .collect()
}

/// The app menu and Window menu for an app that publishes none.
pub fn fallback(name: &str, window: Option<&WindowInfo>, desks: &[DeskInfo]) -> Vec<Menu> {
    let pid = window.and_then(|w| w.pid);
    let app_id = window.map(|w| w.app_id.clone()).unwrap_or_default();
    let id = window.map(|w| w.id);
    let app = Menu::new(
        name,
        vec![
            Entry::action(format!("About {name}"), Action::About { app_id: Some(app_id.clone()) }),
            Entry::separator(),
            Entry::action(format!("Hide {name}"), Action::Wm(Request::HideApp { app: AppSel { app_id: None, pid } }))
                .cmd("H"),
            Entry::action("Hide Others", Action::Wm(Request::HideOthers)).shortcut(sc("⌥❖", "H")),
            Entry::separator(),
            Entry::action(format!("Quit {name}"), Action::QuitApp { app_id: app_id.clone() }).cmd("Q").role("quit"),
        ],
    );
    let window_menu = Menu::new(
        "Window",
        vec![
            Entry::action("Minimise", Action::Wm(Request::Minimise { id })).cmd("M"),
            Entry::action("Fill", Action::Wm(Request::Fill { id, state: None })).shortcut(sc("⌥❖", "↑")),
            Entry::action("Split Left", Action::Wm(Request::Split { id, side: Side::Left })).shortcut(sc("⌥❖", "←")),
            Entry::action("Split Right", Action::Wm(Request::Split { id, side: Side::Right })).shortcut(sc("⌥❖", "→")),
            Entry::action("Stack with…", Action::Wm(Request::Action { action: "stack-with-behind".into() }))
                .shortcut(sc("⌥❖", "S")),
            Entry::submenu("Move to Desk", desk_items(desks, |i| crate::actions::wm::move_to_desk(id, i))),
            Entry::separator(),
            Entry::action("Full Screen", Action::Wm(Request::Fullscreen { id, state: None })).shortcut(sc("⌃❖", "F")),
            Entry::separator(),
            Entry::action("Close", crate::actions::wm::close(id)).cmd("W"),
        ],
    );
    vec![app, window_menu]
}

/// The Window menu's verbs the palette offers for any app.
pub fn window_verbs(window: Option<&WindowInfo>, desks: &[DeskInfo]) -> Menu {
    fallback("", window, desks).remove(1)
}

/// Aizome's own menus, when no window has focus (the desktop is the app).
pub fn desktop(desks: &[DeskInfo]) -> Vec<Menu> {
    vec![
        Menu::new(
            "Aizome",
            vec![
                Entry::action("About Aizome", Action::About { app_id: None }),
                Entry::separator(),
                Entry::action("Settings…", Action::Settings { pane: "appearance".into(), anchor: None }).cmd(","),
                Entry::separator(),
                Entry::action("Lock", Action::Session(SessionVerb::Lock)).shortcut(sc("⌃❖", "Q")),
                Entry::action("Log Out…", Action::Session(SessionVerb::LogOut)).shortcut(sc("⇧❖", "Q")),
            ],
        ),
        Menu::new(
            "File",
            vec![
                Entry::action("New Folder on Desktop", Action::NewFolderOnDesktop).shortcut(sc("⇧❖", "N")),
                Entry::separator(),
                Entry::action("Find…", Action::Palette { scope: Some(Scope::File), query: String::new() }).cmd("F"),
            ],
        ),
        Menu::new("Desks", desks_menu_items(desks)),
        Menu::new(
            "Help",
            vec![
                Entry::action("Search", Action::Palette { scope: Some(Scope::Command), query: String::new() })
                    .shortcut(sc("❖", "Space")),
                Entry::action(
                    "Aizome Help",
                    Action::OpenUrl { url: "https://github.com/bitemyapp/sekka/tree/main/desktop/aizome".into() },
                ),
                Entry::separator(),
                Entry::action("Aizome Tour", Action::Spawn { argv: vec!["aizome-annai".into()] }),
            ],
        ),
    ]
}

fn desks_menu_items(desks: &[DeskInfo]) -> Vec<Entry> {
    let mut items = desk_items(desks, crate::actions::wm::set_desk);
    items.push(Entry::separator());
    items.push(Entry::action("New Desk", Action::NewDesk));
    items.push(Entry::action("Desks Overview", Action::Wm(Request::Overview { state: None })).shortcut(sc("⌃❖", "↑")));
    items
}

/// What the Toolchest needs to be built.
pub struct ToolchestInput<'a> {
    pub apps: &'a [&'a App],
    pub desks: &'a [DeskInfo],
    /// The current Desk's folder.
    pub desk_folder: Option<PathBuf>,
    /// Mounted disks: (label, mount point).
    pub disks: Vec<(String, PathBuf)>,
    pub recent: Vec<PathBuf>,
}

/// The Toolchest: the mark in its title strip, then Applications, Places,
/// Desks, Find, System and Session.
pub fn toolchest(input: &ToolchestInput) -> Menu {
    let apps: Vec<Entry> = catalog::by_category(input.apps)
        .into_iter()
        .map(|(cat, apps)| {
            let rows = apps
                .iter()
                .map(|a| {
                    let glyph = catalog::glyph_for(a.app_id())
                        .map(|g| g.0)
                        .unwrap_or_else(|| catalog::glyph_by_category(&a.categories));
                    let mut e = Entry::action(a.name.clone(), Action::Launch { app_id: a.app_id().to_owned() })
                        .glyph(glyph)
                        .with_id(super::model::hash_id(&("app", a.app_id())));
                    if let Some(g) = &a.generic_name {
                        e = e.detail(g.clone());
                    }
                    e
                })
                .collect();
            Entry::submenu(cat, rows).glyph(catalog::category_glyph(cat))
        })
        .collect();
    let mut places: Vec<Entry> = crate::launch::places()
        .into_iter()
        .map(|(name, path)| {
            let glyph = if name == "Home" { Icon::House } else { Icon::Folder };
            Entry::action(name, Action::Open { path }).glyph(glyph)
        })
        .collect();
    if let Some(f) = &input.desk_folder {
        let name = f.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| f.display().to_string());
        places.push(
            Entry::action(format!("{name} (this Desk)"), Action::Open { path: f.clone() }).glyph(Icon::FolderOpen),
        );
    }
    if !input.disks.is_empty() {
        places.push(Entry::separator());
        for (label, mount) in &input.disks {
            places.push(Entry::action(label.clone(), Action::Open { path: mount.clone() }).glyph(Icon::HardDrive));
        }
    }
    places.push(Entry::separator());
    places.push(
        Entry::action("Trash", Action::Open { path: crate::launch::trash_dir().join("files") }).glyph(Icon::Trash),
    );
    let recent: Vec<Entry> = input
        .recent
        .iter()
        .take(12)
        .map(|p| {
            let name = p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
            Entry::action(name, Action::Open { path: p.clone() }).glyph(crate::palette::model::file_glyph(p))
        })
        .collect();
    places.push(Entry::submenu("Recent", recent).glyph(Icon::Clock));
    let find = [
        ("Commands", Scope::Command, Icon::Command),
        ("Windows", Scope::Window, Icon::AppWindow),
        ("Files", Scope::File, Icon::File),
        ("Apps", Scope::App, Icon::LayoutGrid),
        ("Settings", Scope::Setting, Icon::Settings),
    ]
    .into_iter()
    .map(|(l, s, g)| Entry::action(l, Action::Palette { scope: Some(s), query: String::new() }).glyph(g))
    .collect();
    let system = [
        ("Settei", "Settings", "org.aizome.Settei", Icon::Settings),
        ("Yukimi", "Software", "org.aizome.Yukimi", Icon::Package),
        ("Myaku", "Activity", "org.aizome.Myaku", Icon::Activity),
        ("Tansu", "Disks", "org.aizome.Tansu", Icon::HardDrive),
        ("Kura", "Disk Usage", "org.aizome.Kura", Icon::ChartPie),
    ]
    .into_iter()
    .map(|(n, g, id, icon)| Entry::action(n, Action::Launch { app_id: id.into() }).glyph(icon).detail(g))
    .chain(std::iter::once(
        Entry::action("Application Launches…", Action::LaunchDetails { id: None }).glyph(Icon::Activity),
    ))
    .chain(std::iter::once(
        Entry::action("Terminal", Action::Wm(Request::Action { action: "terminal".into() }))
            .glyph(Icon::SquareTerminal)
            .shortcut(sc("❖", "↩")),
    ))
    .collect();
    let session = vec![
        Entry::action("Lock", Action::Session(SessionVerb::Lock)).glyph(Icon::Lock).shortcut(sc("⌃❖", "Q")),
        Entry::action("Sleep", Action::Session(SessionVerb::Sleep)).glyph(Icon::Moon),
        Entry::separator(),
        Entry::action("Restart…", Action::Session(SessionVerb::Restart)).glyph(Icon::RotateCw),
        Entry::action("Shut Down…", Action::Session(SessionVerb::ShutDown)).glyph(Icon::Power),
        Entry::action("Log Out…", Action::Session(SessionVerb::LogOut)).glyph(Icon::LogOut),
    ];
    Menu {
        title: "Toolchest".into(),
        items: vec![
            Entry::submenu("Applications", apps).glyph(Icon::LayoutGrid),
            Entry::submenu("Places", places).glyph(Icon::Folder),
            Entry::submenu("Desks", desks_menu_items(input.desks)).glyph(Icon::Grid2x2),
            Entry::submenu("Find", find).glyph(Icon::Search),
            Entry::submenu("System", system).glyph(Icon::SlidersHorizontal),
            Entry::submenu("Session", session).glyph(Icon::Power),
        ],
        pid: None,
        proto_id: None,
        mark: true,
    }
}

/// The NeXT menu at the pointer: the app's name in its strip, each of its
/// menus as a submenu (its app menu as Info), Services, then Hide and
/// Quit at the bottom.
pub fn pointer_menu(name: &str, menus: &[Menu], services: Vec<Entry>, hide: Action, quit: Action) -> Menu {
    let mut items: Vec<Entry> = Vec::new();
    for (i, m) in menus.iter().enumerate() {
        let title = if i == 0 { "Info".to_owned() } else { m.title.clone() };
        // The app menu's Hide and Quit move to the bottom; Services gets its
        // own row.
        let sub: Vec<Entry> = m
            .items
            .iter()
            .filter(|e| {
                !matches!(e.role.as_deref(), Some("hide" | "quit" | "services")) && !(i == 0 && e.label == "Services")
            })
            .cloned()
            .collect();
        let sub = trim_separators(sub);
        if !sub.is_empty() {
            let mut e = Entry::submenu(title, sub);
            e.id = m.proto_id.map(u64::from).unwrap_or(e.id);
            items.push(e);
        }
    }
    items.push(Entry::submenu("Services", services));
    items.push(Entry::action("Hide", hide).cmd("H"));
    items.push(Entry::action("Quit", quit).cmd("Q"));
    Menu { title: name.to_owned(), items, pid: menus.first().and_then(|m| m.pid), proto_id: None, mark: false }
}

/// Separators without doubles or ends.
pub fn trim_separators(items: Vec<Entry>) -> Vec<Entry> {
    let mut out: Vec<Entry> = Vec::new();
    for e in items {
        if e.kind == Kind::Separator && out.last().is_none_or(|l| l.kind == Kind::Separator) {
            continue;
        }
        out.push(e);
    }
    while out.last().is_some_and(|l| l.kind == Kind::Separator) {
        out.pop();
    }
    out
}

/// Services ▸ for the selected text.
pub fn services(text: Option<&str>) -> Vec<Entry> {
    let verbs = text.map(ServiceVerb::for_text).unwrap_or_default();
    if verbs.is_empty() {
        return vec![Entry::action("No Services Apply", Action::None).enabled(false)];
    }
    verbs.into_iter().map(|v| Entry::action(v.label(), Action::Service(v))).collect()
}

/// Put Services ▸ for `text` into an app menu (replacing the app's own
/// Services submenu, or adding one before Hide).
pub fn with_services(mut app_menu: Menu, text: Option<&str>) -> Menu {
    let entries = services(text);
    if let Some(e) = app_menu.items.iter_mut().find(|e| {
        e.kind == Kind::Submenu
            && (e.label == "Services" || e.items.iter().any(|i| i.role.as_deref() == Some("services")))
    }) {
        e.items = entries;
        e.enabled = true;
        return app_menu;
    }
    let at = app_menu
        .items
        .iter()
        .position(|e| e.role.as_deref() == Some("hide") || e.label.starts_with("Hide"))
        .unwrap_or(app_menu.items.len());
    app_menu.items.insert(at, Entry::separator());
    app_menu.items.insert(at, Entry::submenu("Services", entries));
    app_menu
}

/// A Desk's context menu in the strip.
pub fn desk_context(desk: &DeskInfo, can_remove: bool) -> Menu {
    Menu::new(
        desk.name.clone(),
        vec![
            Entry::action("Rename", Action::RenameDesk { index: desk.index }),
            Entry::action(
                "Set Folder…",
                Action::Settings { pane: "desktop".into(), anchor: Some(format!("desks.{}", desk.index)) },
            ),
            Entry::separator(),
            Entry::action("Remove", Action::Wm(Request::RemoveDesk { desk: aizome_ipc::DeskSel::Index(desk.index) }))
                .enabled(can_remove),
        ],
    )
}

// The kit's menus.

/// A shortcut as the kit's.
pub fn kit_shortcut(s: &proto::Shortcut) -> Option<aizome_kit::Shortcut> {
    use aizome_kit::ShortcutKey as K;
    let key = match s.key.as_str() {
        "Enter" | "Return" | "↩" => K::Enter,
        "Tab" => K::Tab,
        "Space" => K::Space,
        "Escape" => K::Escape,
        "Backspace" => K::Backspace,
        "Delete" => K::Delete,
        "Up" | "↑" => K::Up,
        "Down" | "↓" => K::Down,
        "Left" | "←" => K::Left,
        "Right" | "→" => K::Right,
        "Home" => K::Home,
        "End" => K::End,
        "PageUp" => K::PageUp,
        "PageDown" => K::PageDown,
        k if k.starts_with('F') && k.len() > 1 && k[1..].parse::<u8>().is_ok() => K::F(k[1..].parse().ok()?),
        k => K::Char(k.chars().next()?),
    };
    Some(aizome_kit::Shortcut { key, command: s.command, shift: s.shift, alt: s.alt, ctrl: s.ctrl })
}

/// A menu as the kit shows it, picking items sending `wrap(action)`.
pub fn to_kit<Msg: Clone + Send + 'static>(menu: &Menu, wrap: &impl Fn(Action) -> Msg) -> aizome_kit::Menu<Msg> {
    let mut m = aizome_kit::Menu::new(menu.title.clone());
    push_items(&mut m, &menu.items, wrap);
    m
}

fn push_items<Msg: Clone + Send + 'static>(
    m: &mut aizome_kit::Menu<Msg>,
    items: &[Entry],
    wrap: &impl Fn(Action) -> Msg,
) {
    for e in items {
        match e.kind {
            Kind::Separator => m.push(aizome_kit::MenuItem::separator()),
            Kind::Header => {
                let taken = std::mem::replace(m, aizome_kit::Menu::new(""));
                *m = taken.header(e.label.clone());
            }
            Kind::Submenu => {
                let mut sub = aizome_kit::Menu::new(e.label.clone());
                push_items(&mut sub, &e.items, wrap);
                let mut item = aizome_kit::MenuItem::submenu(sub);
                if let Some(MenuIcon::Glyph(g)) = &e.icon {
                    item = item.icon(*g);
                }
                m.push(item);
            }
            Kind::Action => {
                let label = match &e.detail {
                    Some(d) => format!("{}  —  {d}", e.label),
                    None => e.label.clone(),
                };
                let mut item =
                    aizome_kit::MenuItem::new(label, wrap(e.action.clone())).enabled(e.enabled).checked(e.checked);
                if let Some(s) = e.shortcut.as_ref().and_then(kit_shortcut) {
                    item = item.shortcut(s);
                }
                if let Some(MenuIcon::Glyph(g)) = &e.icon {
                    item = item.icon(*g);
                }
                m.push(item);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn desks() -> Vec<DeskInfo> {
        vec![
            DeskInfo { index: 1, id: 1, name: "Mail".into(), ..Default::default() },
            DeskInfo { index: 2, id: 2, name: "2".into(), current: true, ..Default::default() },
        ]
    }

    #[test]
    fn apps_without_menus_get_an_app_and_a_window_menu() {
        let w = WindowInfo { id: 5, app_id: "foot".into(), pid: Some(77), ..Default::default() };
        let m = fallback("Foot", Some(&w), &desks());
        assert_eq!(m[0].title, "Foot");
        let quit = m[0].items.iter().find(|e| e.label == "Quit Foot").unwrap();
        assert_eq!(quit.action, Action::QuitApp { app_id: "foot".into() });
        assert_eq!(quit.shortcut.as_ref().unwrap().label(), "❖Q");
        let move_to = m[1].items.iter().find(|e| e.label == "Move to Desk").unwrap();
        assert_eq!(move_to.items[0].label, "1  Mail");
        assert_eq!(move_to.items[1].label, "Desk 2");
        assert!(move_to.items[1].checked);
        assert_eq!(move_to.items[0].action, crate::actions::wm::move_to_desk(Some(5), 1));
    }

    #[test]
    fn the_shell_carries_out_window_roles() {
        let published = vec![proto::Menu {
            id: 1,
            title: "Window".into(),
            items: vec![
                proto::Item {
                    id: 3,
                    label: "Minimise".into(),
                    role: Some("minimise".into()),
                    enabled: true,
                    ..Default::default()
                },
                proto::Item { id: 4, label: "Zoom In".into(), enabled: true, ..Default::default() },
                proto::Item {
                    id: 5,
                    label: "Search".into(),
                    role: Some("help-search".into()),
                    enabled: true,
                    ..Default::default()
                },
            ],
        }];
        let m = app_menus(&published, 9);
        assert_eq!(m[0].items[0].action, Action::Wm(Request::Minimise { id: None }));
        assert_eq!(m[0].items[1].action, Action::AppItem { pid: 9, id: 4 });
        assert_eq!(m[0].items[2].action, Action::Palette { scope: Some(Scope::Command), query: String::new() });
        let help = desktop(&desks()).pop().unwrap();
        let tour = help.items.iter().find(|e| e.label == "Aizome Tour").unwrap();
        assert_eq!(tour.action, Action::Spawn { argv: vec!["aizome-annai".into()] });
    }

    #[test]
    fn the_pointer_menu_gathers_every_menu() {
        let w = WindowInfo { id: 5, app_id: "foot".into(), ..Default::default() };
        let menus = fallback("Foot", Some(&w), &desks());
        let p = pointer_menu("Foot", &menus, services(Some("hi")), Action::None, Action::None);
        let labels: Vec<&str> = p.items.iter().map(|e| e.label.as_str()).collect();
        assert_eq!(labels, ["Info", "Window", "Services", "Hide", "Quit"]);
        assert!(p.items[0].items.iter().all(|e| !e.label.starts_with("Quit")));
        assert!(p.items[2].items.iter().any(|e| e.label == "Search the Web"));
    }

    #[test]
    fn services_fill_the_app_menu() {
        let app = Menu::new(
            "Sumi",
            vec![
                Entry::action("About Sumi", Action::None),
                Entry::submenu("Services", vec![Entry::action("No Services Apply", Action::None).role("services")]),
                Entry::action("Hide Sumi", Action::None).role("hide"),
            ],
        );
        let m = with_services(app, Some("https://example.org"));
        let s = m.items.iter().find(|e| e.label == "Services").unwrap();
        assert!(s.items.iter().any(|e| e.action == Action::Service(ServiceVerb::OpenUrl)));
        let bare = with_services(Menu::new("X", vec![Entry::action("Hide X", Action::None)]), None);
        assert_eq!(bare.items[0].label, "Services");
        assert!(!bare.items[0].items[0].enabled);
    }

    #[test]
    fn the_toolchest_has_its_six_submenus() {
        let t =
            toolchest(&ToolchestInput { apps: &[], desks: &desks(), desk_folder: None, disks: vec![], recent: vec![] });
        let labels: Vec<&str> = t.items.iter().map(|e| e.label.as_str()).collect();
        assert_eq!(labels, ["Applications", "Places", "Desks", "Find", "System", "Session"]);
        assert!(t.mark);
        let session = &t.items[5].items;
        assert_eq!(session[0].shortcut.as_ref().unwrap().label(), "⌃❖Q");
        let find = &t.items[3].items;
        assert_eq!(find[2].action, Action::Palette { scope: Some(Scope::File), query: String::new() });
    }

    #[test]
    fn shortcuts_become_the_kits() {
        let s = kit_shortcut(&sc("⌥❖", "H")).unwrap();
        assert_eq!(s.label(), "⌥❖H");
        assert_eq!(kit_shortcut(&sc("⌃❖", "↑")).unwrap().key, aizome_kit::ShortcutKey::Up);
        assert_eq!(
            kit_shortcut(&proto::Shortcut { key: "F10".into(), ..Default::default() }).unwrap().key,
            aizome_kit::ShortcutKey::F(10)
        );
    }

    #[test]
    fn separators_trim() {
        let items = vec![
            Entry::separator(),
            Entry::action("a", Action::None),
            Entry::separator(),
            Entry::separator(),
            Entry::action("b", Action::None),
            Entry::separator(),
        ];
        let t = trim_separators(items);
        assert_eq!(t.len(), 3);
    }
}

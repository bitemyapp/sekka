//! Menus as data: what every menu the shell shows is made of — the focused
//! app's (from `aizome-shell-proto`), the fallback ones for apps that
//! publish none, Aizome's own, the Toolchest's, the Deskbar's and the
//! desktop's — and what type-ahead and the palette search in them.

use std::hash::{Hash, Hasher};
use std::path::PathBuf;

use aizome_kit::Icon;
use aizome_shell_proto as proto;

use crate::actions::Action;

/// An icon at an item's left.
#[derive(Clone, Debug, PartialEq)]
pub enum MenuIcon {
    Glyph(Icon),
    /// Aizome's mark (the Toolchest).
    Mark,
    /// An app's own icon, from its file.
    File(PathBuf),
}

/// What an item is.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Kind {
    #[default]
    Action,
    Separator,
    Submenu,
    /// A section's name: text that does nothing.
    Header,
}

/// An item of a menu.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Entry {
    pub kind: Kind,
    pub label: String,
    /// Text after the label in `text3` (an app's generic name).
    pub detail: Option<String>,
    pub shortcut: Option<proto::Shortcut>,
    pub enabled: bool,
    pub checked: bool,
    pub icon: Option<MenuIcon>,
    pub action: Action,
    /// A submenu's items.
    pub items: Vec<Entry>,
    /// Stays the same while the item does: the app's id for its items.
    pub id: u64,
    /// What the item is to the system (`"quit"`, `"services"`).
    pub role: Option<String>,
    /// A mark after the label: an app's toolkit.
    pub mark: Option<aizome_kit::ItemMark>,
}

impl Entry {
    pub fn action(label: impl Into<String>, action: Action) -> Entry {
        let label = label.into();
        Entry { id: hash_id(&label), kind: Kind::Action, label, enabled: true, action, ..Entry::default() }
    }

    pub fn submenu(label: impl Into<String>, items: Vec<Entry>) -> Entry {
        let label = label.into();
        Entry {
            id: hash_id(&("menu", &label)),
            kind: Kind::Submenu,
            enabled: !items.is_empty(),
            label,
            items,
            ..Entry::default()
        }
    }

    pub fn separator() -> Entry {
        Entry { kind: Kind::Separator, ..Entry::default() }
    }

    pub fn header(label: impl Into<String>) -> Entry {
        Entry { kind: Kind::Header, label: label.into(), ..Entry::default() }
    }

    pub fn shortcut(mut self, s: proto::Shortcut) -> Entry {
        self.shortcut = Some(s);
        self
    }

    /// ❖ and a key.
    pub fn cmd(self, key: &str) -> Entry {
        self.shortcut(proto::Shortcut { key: key.into(), command: true, ..Default::default() })
    }

    pub fn icon(mut self, icon: MenuIcon) -> Entry {
        self.icon = Some(icon);
        self
    }

    pub fn glyph(self, icon: Icon) -> Entry {
        self.icon(MenuIcon::Glyph(icon))
    }

    pub fn detail(mut self, d: impl Into<String>) -> Entry {
        self.detail = Some(d.into());
        self
    }

    pub fn mark(mut self, mark: Option<aizome_kit::ItemMark>) -> Entry {
        self.mark = mark;
        self
    }

    pub fn enabled(mut self, on: bool) -> Entry {
        self.enabled = on;
        self
    }

    pub fn checked(mut self, on: bool) -> Entry {
        self.checked = on;
        self
    }

    pub fn role(mut self, role: &str) -> Entry {
        self.role = Some(role.into());
        self
    }

    pub fn with_id(mut self, id: u64) -> Entry {
        self.id = id;
        self
    }

    /// Whether the pointer or the keyboard can land on it.
    pub fn selectable(&self) -> bool {
        self.enabled && matches!(self.kind, Kind::Action | Kind::Submenu)
    }
}

/// A shortcut from its label's parts: `sc("⌃❖", "Q")`.
pub fn sc(mods: &str, key: &str) -> proto::Shortcut {
    proto::Shortcut {
        key: key.into(),
        command: mods.contains('❖'),
        shift: mods.contains('⇧'),
        alt: mods.contains('⌥'),
        ctrl: mods.contains('⌃'),
    }
}

pub fn hash_id(v: &impl Hash) -> u64 {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    v.hash(&mut h);
    h.finish() | 1 << 63
}

/// A whole menu: a menu of the bar, a submenu, the Toolchest.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Menu {
    pub title: String,
    pub items: Vec<Entry>,
    /// The app whose menu it is (for `about-to-show` and `activate`).
    pub pid: Option<u32>,
    /// The app's id for the menu, for `about-to-show`.
    pub proto_id: Option<u32>,
    /// The title strip shows Aizome's mark (the Toolchest).
    pub mark: bool,
}

impl Menu {
    pub fn new(title: impl Into<String>, items: Vec<Entry>) -> Menu {
        Menu { title: title.into(), items, ..Menu::default() }
    }

    /// The submenu at `path` (indices from this menu down), as a menu.
    pub fn at(&self, path: &[usize]) -> Option<Menu> {
        let mut items = &self.items;
        let mut title = self.title.clone();
        let mut proto_id = self.proto_id;
        for &i in path {
            let e = items.get(i)?;
            if e.kind != Kind::Submenu {
                return None;
            }
            title = e.label.clone();
            proto_id = (self.pid.is_some()).then_some(e.id as u32);
            items = &e.items;
        }
        Some(Menu { title, items: items.clone(), pid: self.pid, proto_id, mark: self.mark && path.is_empty() })
    }

    /// The entry at `path`.
    pub fn entry(&self, path: &[usize]) -> Option<&Entry> {
        let (last, up) = path.split_last()?;
        let mut items = &self.items;
        for &i in up {
            items = &items.get(i)?.items;
        }
        items.get(*last)
    }

    /// Every action, depth first, with the titles of the submenus it's in.
    pub fn flatten(&self) -> Vec<Flat> {
        let mut out = Vec::new();
        flatten_into(&self.items, &mut vec![], &mut vec![], &mut out);
        out
    }
}

/// An action somewhere in a menu, for type-ahead and the palette.
#[derive(Clone, Debug, PartialEq)]
pub struct Flat {
    /// Indices from the menu down to it.
    pub path: Vec<usize>,
    /// The titles of the submenus it's in, outermost first.
    pub trail: Vec<String>,
    pub entry: Entry,
}

impl Flat {
    /// "Edit › Find", the submenus it's in.
    pub fn trail_label(&self) -> String {
        self.trail.join(" › ")
    }
}

fn flatten_into(items: &[Entry], path: &mut Vec<usize>, trail: &mut Vec<String>, out: &mut Vec<Flat>) {
    for (i, e) in items.iter().enumerate() {
        path.push(i);
        match e.kind {
            Kind::Action => out.push(Flat { path: path.clone(), trail: trail.clone(), entry: e.clone() }),
            Kind::Submenu => {
                trail.push(e.label.clone());
                flatten_into(&e.items, path, trail, out);
                trail.pop();
            }
            _ => {}
        }
        path.pop();
    }
}

/// The rows type-ahead leaves: actions matching `query`, best first, with
/// where their label matched.
pub fn type_ahead(menu: &Menu, query: &str) -> Vec<(Flat, Vec<std::ops::Range<usize>>)> {
    let flat: Vec<Flat> = menu.flatten().into_iter().filter(|f| f.entry.enabled).collect();
    let candidates: Vec<aizome_search::Candidate> = flat
        .iter()
        .map(|f| aizome_search::Candidate { title: f.entry.label.clone(), keywords: vec![f.trail_label()], boost: 0 })
        .collect();
    let mut q = aizome_search::Query::new(query);
    aizome_search::rank(&mut q, &candidates, 40).into_iter().map(|r| (flat[r.index].clone(), r.ranges)).collect()
}

// The app's menus.

/// An app's published menu as the shell's.
pub fn from_proto(menu: &proto::Menu, pid: u32) -> Menu {
    Menu {
        title: menu.title.clone(),
        items: entries(&menu.items, pid),
        pid: Some(pid),
        proto_id: Some(menu.id),
        mark: false,
    }
}

fn entries(items: &[proto::Item], pid: u32) -> Vec<Entry> {
    items
        .iter()
        .map(|i| {
            let kind = match i.kind {
                proto::ItemKind::Action => Kind::Action,
                proto::ItemKind::Separator => Kind::Separator,
                proto::ItemKind::Submenu => Kind::Submenu,
                proto::ItemKind::Header => Kind::Header,
            };
            Entry {
                kind,
                label: i.label.clone(),
                detail: None,
                shortcut: i.shortcut.clone(),
                enabled: i.enabled,
                checked: i.checked,
                icon: i.icon.as_deref().and_then(Icon::from_name).map(MenuIcon::Glyph),
                action: if kind == Kind::Action { Action::AppItem { pid, id: i.id } } else { Action::None },
                items: entries(&i.items, pid),
                id: u64::from(i.id),
                role: i.role.clone(),
                mark: None,
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn editor() -> proto::Menu {
        let item = |id, label: &str| proto::Item { id, label: label.into(), enabled: true, ..Default::default() };
        proto::Menu {
            id: 1,
            title: "Edit".into(),
            items: vec![
                proto::Item {
                    shortcut: Some(proto::Shortcut { key: "Z".into(), command: true, ..Default::default() }),
                    ..item(10, "Undo Bold")
                },
                proto::Item { kind: proto::ItemKind::Separator, ..Default::default() },
                proto::Item {
                    id: 20,
                    kind: proto::ItemKind::Submenu,
                    label: "Find".into(),
                    enabled: true,
                    items: vec![
                        item(21, "Find…"),
                        item(22, "Find and Replace…"),
                        proto::Item { enabled: false, ..item(23, "Find Next") },
                    ],
                    ..Default::default()
                },
            ],
        }
    }

    #[test]
    fn proto_menus_keep_ids_and_activate_the_app() {
        let m = from_proto(&editor(), 42);
        assert_eq!(m.items[0].action, Action::AppItem { pid: 42, id: 10 });
        assert_eq!(m.items[0].shortcut.as_ref().unwrap().label(), "❖Z");
        assert_eq!(m.items[2].kind, Kind::Submenu);
        let find = m.at(&[2]).unwrap();
        assert_eq!(find.title, "Find");
        assert_eq!(find.proto_id, Some(20));
        assert_eq!(m.entry(&[2, 1]).unwrap().label, "Find and Replace…");
    }

    #[test]
    fn flattening_keeps_the_trail() {
        let m = from_proto(&editor(), 1);
        let flat = m.flatten();
        assert_eq!(flat.len(), 4);
        let far = flat.iter().find(|f| f.entry.label == "Find and Replace…").unwrap();
        assert_eq!(far.trail_label(), "Find");
        assert_eq!(far.path, [2, 1]);
    }

    #[test]
    fn type_ahead_reaches_into_submenus_and_skips_disabled_items() {
        let m = from_proto(&editor(), 1);
        let rows = type_ahead(&m, "repl");
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].0.entry.label, "Find and Replace…");
        assert!(!rows[0].1.is_empty());
        assert!(type_ahead(&m, "next").is_empty());
    }

    #[test]
    fn shortcuts_from_glyphs() {
        assert_eq!(sc("⌃❖", "Q").label(), "⌃❖Q");
        assert_eq!(sc("⌥❖", "H").label(), "⌥❖H");
    }
}

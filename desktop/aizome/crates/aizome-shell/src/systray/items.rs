//! Apps' status items in the systray (Steam, Discord, Dropbox…): their
//! icons from the icon theme or their own pixels, what clicking them does,
//! and their menus (`com.canonical.dbusmenu`) as the shell's menus.

use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::path::Path;

use aizome_kit::layout::{Rect, Vec2};
use aizome_kit::prelude::*;
use aizome_kit::{Image, MenuStyle};
use aizome_sys::apps::IconLookup;
use aizome_sys::statusnotifier::{ItemStatus, MenuItem, StatusItem, Toggle, best_pixmap};

use crate::actions::Action;
use crate::look::Pic;
use crate::menubar::{ItemClick, Slot, TrayItem};
use crate::menus::model::{Entry, Menu as ShellMenu, MenuIcon, hash_id};
use crate::shell::{Msg, Shell};

/// Icons are drawn 16pt; read them for a 2× screen.
const ICON_PX: u32 = 32;

/// What the shell keeps of each item's icon: its signature and the image.
pub type IconCache = HashMap<String, (u64, Option<Pic>)>;

/// The slot of the item listed as `key`.
pub fn slot_of(key: &str) -> u64 {
    hash_id(&("status-item", key))
}

fn signature(item: &StatusItem) -> u64 {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    (item.current_icon(), &item.icon_theme_path).hash(&mut h);
    h.finish()
}

/// The image for an item's icon now: by name from its own folder, then the
/// icon theme (or the file, when the name is a path; SVGs drawn at 16pt),
/// else from the pixels it sent.
pub fn image_of(item: &StatusItem, lookup: Option<&IconLookup>) -> Option<Image> {
    let icon = item.current_icon();
    let name = icon.name.trim();
    if name.starts_with('/') {
        if let Some(image) = crate::icons::read(Path::new(name), ICON_PX / 2, 2) {
            return Some(image);
        }
    } else if !name.is_empty() {
        let own = (!item.icon_theme_path.is_empty())
            .then(|| IconLookup::with_base_dirs(vec![Path::new(&item.icon_theme_path).to_path_buf()], "hicolor"));
        let found = own
            .iter()
            .chain(lookup)
            .filter_map(|l| l.lookup(name, ICON_PX / 2, 2))
            .find_map(|p| crate::icons::read(&p, ICON_PX / 2, 2));
        if found.is_some() {
            return found;
        }
    }
    let p = best_pixmap(&icon.pixmaps, ICON_PX)?;
    Image::from_rgba(p.width, p.height, &p.rgba)
}

/// Whole wheel steps in what has come (`delta`, in points, added to what
/// was left over in `left`), a step being a line as the kit counts a wheel
/// notch: (across, up). What's less than a step waits for more, so a
/// touchpad's small movements add up.
pub fn wheel_steps(left: &mut Vec2, delta: Vec2) -> (i32, i32) {
    *left += delta;
    let take = |v: &mut f64| {
        let n = (*v / aizome_kit::LINE).trunc();
        *v -= n * aizome_kit::LINE;
        n as i32
    };
    (take(&mut left.x), take(&mut left.y))
}

/// The systray's items from the watcher's: the ones that want to be seen,
/// with their icons (read again only when they change).
pub fn tray_items(items: &[StatusItem], lookup: Option<&IconLookup>, cache: &mut IconCache) -> Vec<TrayItem> {
    cache.retain(|k, _| items.iter().any(|i| &i.key == k));
    items
        .iter()
        .filter(|i| i.status != ItemStatus::Passive)
        .map(|i| {
            let sig = signature(i);
            let image = match cache.get(&i.key) {
                Some((s, image)) if *s == sig => image.clone(),
                _ => {
                    let image = image_of(i, lookup).map(Pic);
                    cache.insert(i.key.clone(), (sig, image.clone()));
                    image
                }
            };
            TrayItem { key: i.key.clone(), slot: slot_of(&i.key), label: i.label().to_owned(), image, sig }
        })
        .collect()
}

/// An item's dbusmenu as one of the shell's menus: each row picked sends
/// its id back.
pub fn menu_of(key: &str, title: &str, root: &MenuItem, lookup: Option<&IconLookup>) -> ShellMenu {
    fn rows(key: &str, items: &[MenuItem], lookup: Option<&IconLookup>) -> Vec<Entry> {
        items
            .iter()
            .map(|i| {
                if i.separator {
                    return Entry::separator();
                }
                let mut e = if i.is_submenu() {
                    Entry::submenu(i.label.clone(), rows(key, &i.children, lookup))
                } else {
                    Entry::action(i.label.clone(), Action::StatusMenuItem { key: key.to_owned(), id: i.id })
                };
                let enabled = i.enabled && e.enabled;
                e = e.with_id(hash_id(&("status-row", key, i.id))).enabled(enabled);
                if let Toggle::Check(on) | Toggle::Radio(on) = i.toggle {
                    e = e.checked(on);
                }
                if let Some(path) = i.icon_name.as_deref().and_then(|n| lookup?.lookup(n, 16, 2)) {
                    e = e.icon(MenuIcon::File(path));
                }
                e
            })
            .collect()
    }
    ShellMenu::new(title, crate::menus::build::trim_separators(rows(key, &root.children, lookup)))
}

impl Shell {
    /// The systray's items, from the watcher's snapshot.
    pub fn sync_status_items(&mut self) {
        let Some(sn) = &self.sys.status else { return };
        let s = sn.snapshot();
        self.tray.items = tray_items(&s.items, self.icon_lookup.as_ref(), &mut self.item_icons);
    }

    fn status_item(&self, slot: u64) -> Option<StatusItem> {
        let sn = self.sys.status.as_ref()?;
        sn.snapshot().items.iter().find(|i| slot_of(&i.key) == slot).cloned()
    }

    /// Where on the screen a click at `rect` of `bar` is, for the items
    /// that place a window or menu of their own there.
    fn screen_point(&self, bar: WindowKey, rect: Rect) -> (i32, i32) {
        let origin = self.screen_of(bar).map(|s| (s.output.frame.x0, s.output.frame.y0)).unwrap_or_default();
        ((origin.0 + rect.x0) as i32, (origin.1 + crate::menubar::HEIGHT) as i32)
    }

    /// The wheel over a status item: whole steps to the item, as `Scroll`
    /// (a volume control turns its volume).
    pub fn item_scroll(&mut self, slot: u64, delta: Vec2) {
        let Some(item) = self.status_item(slot) else { return };
        let Some(sn) = &self.sys.status else { return };
        let (across, up) = wheel_steps(self.item_wheel.entry(slot).or_default(), delta);
        if up != 0 {
            let _ = sn.scroll(&item.key, up, true);
        }
        if across != 0 {
            let _ = sn.scroll(&item.key, across, false);
        }
    }

    /// A status item was clicked.
    pub fn item_click(&mut self, bar: WindowKey, slot: u64, click: ItemClick, rect: Rect, cx: &mut Cx<Msg>) {
        let Some(item) = self.status_item(slot) else { return };
        let Some(sn) = &self.sys.status else { return };
        let (x, y) = self.screen_point(bar, rect);
        match click {
            ItemClick::Secondary => {
                let _ = sn.secondary_activate(&item.key, x, y);
            }
            ItemClick::Primary if !item.item_is_menu || item.menu.is_none() => {
                // Activate; an item that turns out to be only a menu shows
                // its menu instead.
                let task = sn.activate(&item.key, x, y);
                let has_menu = item.menu.is_some();
                cx.spawn(move || match task.wait() {
                    Err(error) if has_menu => {
                        log::debug!("status item {}: {error}; its menu instead", item.key);
                        Msg::ItemMenuWanted(bar, slot, rect)
                    }
                    Err(error) => {
                        log::info!("status item {}: {error}", item.key);
                        Msg::Nothing
                    }
                    Ok(()) => Msg::Nothing,
                });
            }
            ItemClick::Primary | ItemClick::Menu => {
                if self.pressed_to_close(bar, Slot::Item(slot)) {
                    return;
                }
                self.item_menu(bar, slot, rect, cx);
            }
        }
    }

    /// Read a status item's menu, to show it under the item; items without
    /// one are asked to show their own.
    pub fn item_menu(&mut self, bar: WindowKey, slot: u64, rect: Rect, cx: &mut Cx<Msg>) {
        let Some(item) = self.status_item(slot) else { return };
        let Some(sn) = &self.sys.status else { return };
        if item.menu.is_none() {
            let (x, y) = self.screen_point(bar, rect);
            let _ = sn.context_menu(&item.key, x, y);
            return;
        }
        let task = sn.menu(&item.key);
        cx.spawn(move || Msg::ItemMenu(bar, slot, rect, task.wait().map_err(|e| e.to_string())));
    }

    /// A status item's menu was read: show it.
    pub fn show_item_menu(
        &mut self,
        bar: WindowKey,
        slot: u64,
        rect: Rect,
        menu: Result<MenuItem, String>,
        cx: &mut Cx<Msg>,
    ) {
        let Some(item) = self.status_item(slot) else { return };
        let root = match menu {
            Ok(root) if !root.children.is_empty() => root,
            Ok(_) => return log::info!("status item {}: an empty menu", item.key),
            Err(error) => return log::info!("status item {}: its menu: {error}", item.key),
        };
        let menu = menu_of(&item.key, item.label(), &root, self.icon_lookup.as_ref());
        let anchor = Rect::new(rect.x0, rect.y0, rect.x1, crate::menubar::HEIGHT - 1.0);
        let style = self.as_bar_menu(MenuStyle::below(), bar, Slot::Item(slot), false);
        self.show_menu(bar, anchor, &menu, style, cx);
    }

    /// A row of a status item's menu was picked.
    pub fn status_menu_item(&self, key: &str, id: i32) {
        if let Some(sn) = &self.sys.status {
            let _ = sn.menu_clicked(key, id);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aizome_sys::statusnotifier::{ItemIcon, Pixmap};

    fn item(key: &str, status: ItemStatus) -> StatusItem {
        StatusItem {
            key: key.into(),
            id: key.into(),
            status,
            icon: ItemIcon {
                name: String::new(),
                pixmaps: vec![Pixmap { width: 2, height: 2, rgba: vec![255; 16].into() }],
            },
            ..StatusItem::default()
        }
    }

    #[test]
    fn passive_items_hide_and_icons_come_from_pixels() {
        let mut cache = IconCache::new();
        let items = [item(":1.4/a", ItemStatus::Active), item(":1.5/b", ItemStatus::Passive)];
        let tray = tray_items(&items, None, &mut cache);
        assert_eq!(tray.len(), 1);
        assert_eq!(tray[0].label, ":1.4/a");
        assert_eq!(tray[0].slot, slot_of(":1.4/a"));
        assert_eq!(tray[0].image.as_ref().map(|p| p.0.pixel_size()), Some((2, 2)));
        // Gone items leave the cache.
        tray_items(&items[1..], None, &mut cache);
        assert!(!cache.contains_key(":1.4/a"));
    }

    #[test]
    fn icons_by_name_from_the_items_own_folder() {
        // As Steam sends it: a name, and a folder with name.png at its top.
        let dir = std::env::temp_dir().join(format!("aizome-item-icons-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        aizome_kit::testing::write_png(&dir.join("steam_tray_mono.png"), 4, 4, &[200; 64]).unwrap();
        let mut i = item(":1.4/a", ItemStatus::Active);
        i.icon.name = "steam_tray_mono".into();
        i.icon_theme_path = dir.display().to_string();
        let image = image_of(&i, None);
        std::fs::remove_dir_all(&dir).unwrap();
        assert_eq!(image.map(|i| i.pixel_size()), Some((4, 4)), "the file, not the 2×2 pixels");
    }

    #[test]
    fn an_svg_icon_by_name_is_drawn_rather_than_the_pixels() {
        // As many apps send it: a name whose only file is a scalable one,
        // and small pixels besides.
        let dir = std::env::temp_dir().join(format!("aizome-item-svg-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let svg = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 16 16"><rect width="16" height="16" fill="#e0a030"/></svg>"##;
        std::fs::write(dir.join("indicator.svg"), svg).unwrap();
        let mut i = item(":1.5/a", ItemStatus::Active);
        i.icon.name = "indicator".into();
        i.icon_theme_path = dir.display().to_string();
        let image = image_of(&i, None);
        i.icon.name = dir.join("indicator.svg").display().to_string();
        let by_path = image_of(&i, None);
        std::fs::remove_dir_all(&dir).unwrap();
        assert_eq!(image.map(|i| i.pixel_size()), Some((ICON_PX, ICON_PX)), "drawn at 16pt on a 2× screen");
        assert_eq!(by_path.map(|i| i.pixel_size()), Some((ICON_PX, ICON_PX)), "a path names the file");
    }

    #[test]
    fn the_wheel_turns_into_whole_steps() {
        let line = aizome_kit::LINE;
        let mut left = Vec2::ZERO;
        // A wheel notch is a step; up (positive) stays up.
        assert_eq!(wheel_steps(&mut left, Vec2::new(0.0, line)), (0, 1));
        assert_eq!(wheel_steps(&mut left, Vec2::new(0.0, -2.0 * line)), (0, -2));
        // A touchpad's small movements add up to one.
        for _ in 0..3 {
            assert_eq!(wheel_steps(&mut left, Vec2::new(line / 4.0, 0.0)), (0, 0));
        }
        assert_eq!(wheel_steps(&mut left, Vec2::new(line / 4.0, 0.0)), (1, 0));
        assert_eq!(left, Vec2::ZERO);
    }

    #[test]
    fn menus_become_the_shells() {
        let root = MenuItem {
            children: vec![
                MenuItem { id: 1, label: "Library".into(), enabled: true, ..MenuItem::default() },
                MenuItem { separator: true, ..MenuItem::default() },
                MenuItem {
                    id: 3,
                    label: "Friends".into(),
                    enabled: true,
                    children: vec![MenuItem {
                        id: 4,
                        label: "Online".into(),
                        enabled: true,
                        toggle: Toggle::Radio(true),
                        ..MenuItem::default()
                    }],
                    ..MenuItem::default()
                },
                MenuItem { id: 5, label: "Exit".into(), enabled: false, ..MenuItem::default() },
            ],
            ..MenuItem::default()
        };
        let m = menu_of("k", "Steam", &root, None);
        assert_eq!(m.title, "Steam");
        let labels: Vec<&str> = m.items.iter().map(|e| e.label.as_str()).collect();
        assert_eq!(labels, ["Library", "", "Friends", "Exit"]);
        assert_eq!(m.items[0].action, Action::StatusMenuItem { key: "k".into(), id: 1 });
        assert!(m.items[2].items[0].checked);
        assert!(!m.items[3].enabled);
    }
}

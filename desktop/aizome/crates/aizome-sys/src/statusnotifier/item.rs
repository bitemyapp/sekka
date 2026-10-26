//! One status item as the host sees it: what `org.kde.StatusNotifierItem`'s
//! properties say, read leniently.

use crate::bus::{Props, PropsExt, unwrap_variant, value_bytes, value_i64, value_str};
use std::sync::Arc;
use zbus::zvariant::Value;

/// The interface every item serves.
pub const ITEM_IFACE: &str = "org.kde.StatusNotifierItem";
/// Where an item registered by bus name alone serves it.
pub const DEFAULT_PATH: &str = "/StatusNotifierItem";

/// What an item is about.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Category {
    #[default]
    ApplicationStatus,
    Communications,
    SystemServices,
    Hardware,
}

impl Category {
    pub fn parse(s: &str) -> Category {
        match s {
            "Communications" => Category::Communications,
            "SystemServices" => Category::SystemServices,
            "Hardware" => Category::Hardware,
            _ => Category::ApplicationStatus,
        }
    }
}

/// Whether the item wants to be seen.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum ItemStatus {
    /// Nothing to show now: hosts may hide it.
    Passive,
    #[default]
    Active,
    /// Something wants the person's attention (show the attention icon).
    NeedsAttention,
}

impl ItemStatus {
    pub fn parse(s: &str) -> ItemStatus {
        match s {
            "Passive" => ItemStatus::Passive,
            "NeedsAttention" => ItemStatus::NeedsAttention,
            _ => ItemStatus::Active,
        }
    }
}

/// An icon sent as pixels: straight RGBA, rows `width × 4` bytes apart.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Pixmap {
    pub width: u32,
    pub height: u32,
    pub rgba: Arc<[u8]>,
}

impl Pixmap {
    /// One `(iiay)`: width, height and ARGB32 pixels in network byte order
    /// (A, R, G, B), as the specification sends them. `None` when the
    /// bytes don't fill the size.
    pub fn from_argb(width: i64, height: i64, argb: &[u8]) -> Option<Pixmap> {
        let (w, h) = (u32::try_from(width).ok()?, u32::try_from(height).ok()?);
        if w == 0 || h == 0 || w > 1024 || h > 1024 || argb.len() != w as usize * h as usize * 4 {
            return None;
        }
        let rgba: Vec<u8> = argb.as_chunks::<4>().0.iter().flat_map(|p| [p[1], p[2], p[3], p[0]]).collect();
        Some(Pixmap { width: w, height: h, rgba: rgba.into() })
    }
}

/// An `a(iiay)` value's pixmaps; ones that don't read are left out.
pub fn pixmaps(value: &Value<'_>) -> Vec<Pixmap> {
    let Value::Array(array) = unwrap_variant(value) else { return Vec::new() };
    array
        .iter()
        .filter_map(|v| {
            let Value::Structure(s) = unwrap_variant(v) else { return None };
            let [w, h, px] = s.fields() else { return None };
            Pixmap::from_argb(value_i64(w)?, value_i64(h)?, &value_bytes(px))
        })
        .collect()
}

/// The pixmap to draw `px` pixels square: the smallest at least that big,
/// else the biggest.
pub fn best_pixmap(pixmaps: &[Pixmap], px: u32) -> Option<&Pixmap> {
    let big_enough = pixmaps.iter().filter(|p| p.width.min(p.height) >= px).min_by_key(|p| p.width * p.height);
    big_enough.or_else(|| pixmaps.iter().max_by_key(|p| p.width * p.height))
}

/// An icon by name (from the icon theme, or a file when it's a path),
/// and by pixels.
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct ItemIcon {
    pub name: String,
    pub pixmaps: Vec<Pixmap>,
}

impl ItemIcon {
    pub fn is_empty(&self) -> bool {
        self.name.is_empty() && self.pixmaps.is_empty()
    }
}

/// A status item on the bus.
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct StatusItem {
    /// How the watcher lists it: the bus name and the object path,
    /// `:1.42/org/ayatana/NotificationItem/steam`.
    pub key: String,
    /// Where to call it: its bus name (unique or well-known) and path.
    pub service: String,
    pub path: String,
    /// The app's name for it (`steam`).
    pub id: String,
    pub title: String,
    pub category: Category,
    pub status: ItemStatus,
    pub icon: ItemIcon,
    pub attention_icon: ItemIcon,
    /// A folder to look for its icons' names in first (KDE's addition).
    pub icon_theme_path: String,
    /// The tooltip's title and text (the text may carry markup).
    pub tooltip_title: String,
    pub tooltip_text: String,
    /// Its `com.canonical.dbusmenu` object, if it has a menu.
    pub menu: Option<String>,
    /// Only a menu: a click shows it rather than activating the item.
    pub item_is_menu: bool,
    /// The order it registered in, oldest first.
    pub order: u64,
}

impl StatusItem {
    /// An item from its properties.
    pub fn from_props(key: &str, service: &str, path: &str, order: u64, props: &Props) -> StatusItem {
        let mut item = StatusItem {
            key: key.to_owned(),
            service: service.to_owned(),
            path: path.to_owned(),
            id: props.str("Id"),
            title: props.str("Title"),
            category: Category::parse(&props.str("Category")),
            status: ItemStatus::parse(&props.str("Status")),
            icon: ItemIcon {
                name: props.str("IconName"),
                pixmaps: props.value("IconPixmap").map(pixmaps).unwrap_or_default(),
            },
            attention_icon: ItemIcon {
                name: props.str("AttentionIconName"),
                pixmaps: props.value("AttentionIconPixmap").map(pixmaps).unwrap_or_default(),
            },
            icon_theme_path: props.str("IconThemePath"),
            menu: props.path("Menu"),
            item_is_menu: props.bool("ItemIsMenu"),
            order,
            ..StatusItem::default()
        };
        if let Some(Value::Structure(s)) = props.value("ToolTip").map(unwrap_variant)
            && let [_, _, title, text] = s.fields()
        {
            item.tooltip_title = value_str(title).unwrap_or_default();
            item.tooltip_text = value_str(text).unwrap_or_default();
        }
        item
    }

    /// The icon to show now: the attention icon while it asks for
    /// attention (if it has one).
    pub fn current_icon(&self) -> &ItemIcon {
        if self.status == ItemStatus::NeedsAttention && !self.attention_icon.is_empty() {
            &self.attention_icon
        } else {
            &self.icon
        }
    }

    /// What to call it: its title, its tooltip's, or its id.
    pub fn label(&self) -> &str {
        [&self.title, &self.tooltip_title, &self.id, &self.service]
            .into_iter()
            .find(|s| !s.trim().is_empty())
            .map_or("", |s| s.trim())
    }
}

/// Where a registration points: the service (a bus name) and the object
/// path. Apps register a bus name (the item at [`DEFAULT_PATH`]), or an
/// object path on their own connection (libappindicator does), or both
/// run together (`:1.42/org/ayatana/…`, as watchers list them).
pub fn registration(sender: &str, arg: &str) -> Option<(String, String)> {
    let arg = arg.trim();
    if sender.is_empty() && (arg.is_empty() || arg.starts_with('/')) {
        return None;
    }
    if arg.is_empty() {
        return Some((sender.to_owned(), DEFAULT_PATH.to_owned()));
    }
    if arg.starts_with('/') {
        return Some((sender.to_owned(), arg.to_owned()));
    }
    match arg.find('/') {
        Some(i) => Some((arg[..i].to_owned(), arg[i..].to_owned())),
        None => Some((arg.to_owned(), DEFAULT_PATH.to_owned())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registrations_by_name_path_or_both() {
        let r = |sender, arg| registration(sender, arg);
        assert_eq!(
            r(":1.5", "org.kde.StatusNotifierItem-7-1"),
            Some(("org.kde.StatusNotifierItem-7-1".into(), DEFAULT_PATH.into()))
        );
        assert_eq!(
            r(":1.5", "/org/ayatana/NotificationItem/steam"),
            Some((":1.5".into(), "/org/ayatana/NotificationItem/steam".into()))
        );
        assert_eq!(r(":1.5", ":1.9/StatusNotifierItem"), Some((":1.9".into(), "/StatusNotifierItem".into())));
        assert_eq!(r(":1.5", ""), Some((":1.5".into(), DEFAULT_PATH.into())));
        assert_eq!(r("", "/x"), None);
    }

    #[test]
    fn pixmaps_turn_argb_into_rgba() {
        let p = Pixmap::from_argb(1, 2, &[255, 10, 20, 30, 128, 1, 2, 3]).unwrap();
        assert_eq!(&*p.rgba, &[10, 20, 30, 255, 1, 2, 3, 128]);
        assert_eq!(Pixmap::from_argb(2, 2, &[0; 4]), None);
        assert_eq!(Pixmap::from_argb(-1, 1, &[0; 4]), None);
    }

    #[test]
    fn the_smallest_pixmap_big_enough() {
        let p = |n: u32| Pixmap { width: n, height: n, rgba: vec![0; (n * n * 4) as usize].into() };
        let all = [p(16), p(64), p(32), p(22)];
        assert_eq!(best_pixmap(&all, 24).map(|p| p.width), Some(32));
        assert_eq!(best_pixmap(&all, 128).map(|p| p.width), Some(64));
        assert_eq!(best_pixmap(&[], 16), None);
    }

    #[test]
    fn labels_fall_back() {
        let mut i = StatusItem { id: "steam".into(), service: ":1.4".into(), ..StatusItem::default() };
        assert_eq!(i.label(), "steam");
        i.tooltip_title = "Steam".into();
        assert_eq!(i.label(), "Steam");
        i.title = " ".into();
        assert_eq!(i.label(), "Steam");
    }

    #[test]
    fn attention_icon_while_it_needs_attention() {
        let mut i = StatusItem {
            icon: ItemIcon { name: "a".into(), pixmaps: vec![] },
            attention_icon: ItemIcon { name: "b".into(), pixmaps: vec![] },
            ..StatusItem::default()
        };
        assert_eq!(i.current_icon().name, "a");
        i.status = ItemStatus::NeedsAttention;
        assert_eq!(i.current_icon().name, "b");
        i.attention_icon = ItemIcon::default();
        assert_eq!(i.current_icon().name, "a");
    }
}

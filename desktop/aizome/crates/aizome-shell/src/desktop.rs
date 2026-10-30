//! The desktop (C2-Desktop): a background surface per output with the
//! theme's wallpaper, scaled to cover and cropped, and icons down the left —
//! Home, the disks and mounted images, the Trash, then what's in
//! `~/Desktop` — on `desktop_label` chips. Click selects (a rubber band
//! too), double-click opens, drag moves (onto the Trash: trashes), and the
//! positions are remembered.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use aizome_kit::element::Element;
use aizome_kit::layout::{Align, Point, Rect, Size};
use aizome_kit::widget::SemanticRole;
use aizome_kit::{Icon, Image, Layoutable};
use aizome_theme::typeface;

use crate::look::{self, Hue};
use crate::widgets::drawn::{DragPhase, Press, drawn, free};

pub const ICON_W: f64 = 84.0;
pub const ICON_H: f64 = 72.0;
pub const GLYPH: f64 = 46.0;
/// The first icon's place, under the bar, and how far apart they are.
pub const LEFT: f64 = 14.0;
pub const TOP: f64 = 44.0;
pub const STEP: f64 = 88.0;

/// What an icon is.
#[derive(Clone, Debug, PartialEq)]
pub enum IconKind {
    Home(PathBuf),
    /// A disk or image. `system`: the disk the system runs from, which is
    /// opened but never unmounted or ejected.
    Volume {
        block: String,
        mount: Option<String>,
        ejectable: bool,
        drive: Option<String>,
        image: bool,
        system: bool,
    },
    Trash {
        full: bool,
    },
    Item {
        path: PathBuf,
        dir: bool,
    },
}

#[derive(Clone, Debug, PartialEq)]
pub struct DeskIcon {
    /// Stable across sessions: `home`, `trash`, `vol:<block>`, `file:<name>`.
    pub key: String,
    pub label: String,
    pub glyph: Icon,
    pub hue: Hue,
    pub kind: IconKind,
}

/// What the desktop's elements say.
#[derive(Clone, Debug, PartialEq)]
pub enum DesktopMsg {
    /// An icon pressed: select it (with ⇧ or ❖: add it).
    Select(String, bool),
    Open(String),
    IconMenu(String, Point),
    IconDrag(String, DragPhase),
    /// The empty desktop: a press clears the selection, a drag draws a
    /// rubber band.
    Band(DragPhase),
    Clear,
    /// Right-click on the empty desktop, at (the surface's coordinates).
    Menu(Point),
    /// Files dropped on an icon (the Trash, a disk, a folder).
    DropOn(String, Vec<String>),
}

/// The desktop's state that outlives a view.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct DesktopState {
    pub selected: Vec<String>,
    /// Where the person put icons (else they line up down the left).
    pub positions: HashMap<String, Point>,
    /// The rubber band: from and to.
    pub band: Option<(Point, Point)>,
    /// An icon being dragged, and where it is now.
    pub dragging: Option<(String, Point)>,
    /// What's in `~/Desktop`.
    pub items: Vec<(PathBuf, bool)>,
}

impl DesktopState {
    pub fn path() -> PathBuf {
        aizome_config::state_dir().join("desktop.json")
    }

    pub fn load_positions(&mut self, path: &Path) {
        if let Ok(b) = std::fs::read(path)
            && let Ok(map) = serde_json::from_slice::<HashMap<String, [f64; 2]>>(&b)
        {
            self.positions = map.into_iter().map(|(k, [x, y])| (k, Point::new(x, y))).collect();
        }
    }

    pub fn save_positions(&self, path: &Path) {
        let map: HashMap<&String, [f64; 2]> = self.positions.iter().map(|(k, p)| (k, [p.x, p.y])).collect();
        if let Ok(b) = serde_json::to_vec_pretty(&map) {
            if let Some(d) = path.parent() {
                let _ = std::fs::create_dir_all(d);
            }
            let _ = std::fs::write(path, b);
        }
    }
}

/// Where each icon goes: placed ones where they were put, the rest down
/// the left (a new column when the output's height runs out), `left` in
/// from it (clear of a Deskbar standing there).
pub fn layout(icons: &[DeskIcon], positions: &HashMap<String, Point>, height: f64, left: f64) -> Vec<Point> {
    let rows = (((height - TOP - 8.0) / STEP).floor() as usize).max(1);
    let mut slot = 0usize;
    icons
        .iter()
        .map(|i| match positions.get(&i.key) {
            Some(p) => *p,
            None => {
                let (col, row) = (slot / rows, slot % rows);
                slot += 1;
                Point::new(left + LEFT + col as f64 * (ICON_W + 8.0), TOP + row as f64 * STEP)
            }
        })
        .collect()
}

/// The icons: Home, the volumes, the Trash, then the desktop folder's
/// items.
pub fn icons(
    user: &str,
    volumes: &[aizome_sys::disks::Volume],
    trash_full: bool,
    items: &[(PathBuf, bool)],
) -> Vec<DeskIcon> {
    let mut v = vec![DeskIcon {
        key: "home".into(),
        label: user.into(),
        glyph: Icon::House,
        hue: Hue::Blue,
        kind: IconKind::Home(aizome_config::home_dir()),
    }];
    for vol in volumes {
        let image = vol.image_file.is_some();
        v.push(DeskIcon {
            key: format!("vol:{}", vol.block),
            label: vol
                .image_file
                .as_deref()
                .and_then(|f| Path::new(f).file_name())
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_else(|| vol.name.clone()),
            glyph: if image {
                Icon::Disc
            } else if vol.ejectable && !vol.system {
                Icon::Usb
            } else {
                Icon::HardDrive
            },
            hue: if image { Hue::Violet } else { Hue::Text2 },
            kind: IconKind::Volume {
                block: vol.block.clone(),
                mount: vol.mount_point.clone(),
                ejectable: vol.ejectable && !vol.system,
                drive: vol.drive.clone(),
                image,
                system: vol.system,
            },
        });
    }
    v.push(DeskIcon {
        key: "trash".into(),
        label: "Trash".into(),
        glyph: Icon::Trash,
        hue: Hue::Text2,
        kind: IconKind::Trash { full: trash_full },
    });
    for (path, dir) in items {
        let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        v.push(DeskIcon {
            key: format!("file:{name}"),
            label: name,
            glyph: if *dir { Icon::Folder } else { crate::palette::model::file_glyph(path) },
            hue: if *dir { Hue::Blue } else { Hue::Text2 },
            kind: IconKind::Item { path: path.clone(), dir: *dir },
        });
    }
    v
}

/// What's in a folder (not hidden files), folders first, by name.
pub fn read_items(dir: &Path) -> Vec<(PathBuf, bool)> {
    let Ok(rd) = std::fs::read_dir(dir) else { return Vec::new() };
    let mut v: Vec<(PathBuf, bool)> = rd
        .flatten()
        .filter(|e| !e.file_name().to_string_lossy().starts_with('.'))
        .map(|e| (e.path(), e.path().is_dir()))
        .collect();
    v.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.file_name().cmp(&b.0.file_name())));
    v
}

fn icon_el(icon: &DeskIcon, selected: bool) -> Element<DesktopMsg> {
    let (label, glyph, hue) = (icon.label.clone(), icon.glyph, icon.hue);
    let full = matches!(icon.kind, IconKind::Trash { full: true });
    let key = icon.key.clone();
    let (k1, k2, k3, k4, k5) = (key.clone(), key.clone(), key.clone(), key.clone(), key.clone());
    let text = label.clone();
    drawn(ICON_W, ICON_H, (label.clone(), glyph, hue, selected, full), move |cx| {
        let t = cx.theme();
        let b = cx.bounds();
        let g = Rect::new(((ICON_W - GLYPH) / 2.0).round(), 0.0, ((ICON_W - GLYPH) / 2.0).round() + GLYPH, GLYPH);
        if selected {
            cx.canvas.fill_rect(g.inflate(3.0, 3.0), t.selection().with_alpha(0.35));
        }
        // Lucide at 46pt with a 2pt stroke, as the boards draw them.
        look::glyph(cx.canvas, glyph, g.origin(), GLYPH, hue.color(t), 2.0);
        if full {
            cx.canvas.fill_rect(Rect::new(g.x1 - 10.0, g.y0 + 4.0, g.x1 - 4.0, g.y0 + 10.0), Hue::Amber.color(t));
        }
        let s = look::ui(
            12.0 * t.metrics.factor,
            typeface::WEIGHT_SEMIBOLD,
            if selected { t.on_selection() } else { t.text() },
        );
        let w = cx.layout_text(&label, &s, None).size().width.min(b.width() + 2.0 * 40.0 - 12.0).ceil();
        let chip = Rect::new(
            ((b.width() - w) / 2.0 - 6.0).floor(),
            GLYPH + 5.0,
            ((b.width() + w) / 2.0 + 6.0).ceil(),
            GLYPH + 5.0 + 18.0,
        );
        let fill = if selected { t.selection() } else { aizome_kit::theme::color(t.palette.desktop_label) };
        cx.canvas.fill_rect(chip, fill);
        cx.draw_text_in(&label, &s, Rect::new(chip.x0 + 6.0, chip.y0, chip.x1 - 6.0, chip.y1), Align::Center);
    })
    .overflow(40.0)
    .on_press(move |_p: Press| DesktopMsg::Select(k1.clone(), false))
    .on_double_click(DesktopMsg::Open(k2))
    .on_right_click(move |p: Press| DesktopMsg::IconMenu(k3.clone(), p.at))
    .on_drag(move |d| DesktopMsg::IconDrag(k4.clone(), d))
    .on_drop(move |urls, _| DesktopMsg::DropOn(k5.clone(), urls))
    .text(text, SemanticRole::ListItem)
    .key(("icon", key.as_str()))
    .into()
}

/// The wallpaper (scaled to cover the output and cropped already, so it
/// lands one to one on the output's pixels), else the desktop colour.
/// `image` carries which picture it is, for redrawing when it changes.
fn ground(image: Option<(Image, u64)>, size: Size) -> Element<DesktopMsg> {
    let key = image.as_ref().map(|(_, id)| *id);
    drawn(size.width, size.height, key, move |cx| {
        let t = cx.theme();
        let b = cx.bounds();
        match &image {
            Some((img, _)) if img.pixel_size().0 > 0 && img.pixel_size().1 > 0 => {
                // The part of it the output shows, in its pixels.
                let s = Size::new(img.pixel_size().0 as f64, img.pixel_size().1 as f64);
                let k = (b.width() / s.width).max(b.height() / s.height);
                let (cw, ch) = (b.width() / k, b.height() / k);
                let src =
                    Rect::new((s.width - cw) / 2.0, (s.height - ch) / 2.0, (s.width + cw) / 2.0, (s.height + ch) / 2.0);
                cx.canvas.draw_image_with(img, Some(src), b, crate::wallpaper::options(img, b, cx.canvas.scale()));
            }
            _ => cx.canvas.fill_rect(b, aizome_kit::theme::color(t.palette.desktop)),
        }
    })
    .on_drag(DesktopMsg::Band)
    .on_press(|_| DesktopMsg::Clear)
    .on_right_click(|p: Press| DesktopMsg::Menu(p.at))
    .text("Desktop", SemanticRole::Group)
    .key("ground")
    .into()
}

/// One output's desktop.
pub fn view(
    icons: &[DeskIcon],
    state: &DesktopState,
    wallpaper: Option<(Image, u64)>,
    size: Size,
    left: f64,
) -> Element<DesktopMsg> {
    let points = layout(icons, &state.positions, size.height, left);
    let mut placed: Vec<(Point, Element<DesktopMsg>)> = icons
        .iter()
        .zip(points)
        .map(|(i, p)| {
            let at = match &state.dragging {
                Some((k, to)) if *k == i.key => *to,
                _ => p,
            };
            (at, icon_el(i, state.selected.contains(&i.key)))
        })
        .collect();
    if let Some((a, b)) = state.band {
        let r = Rect::from_points(a, b);
        placed.push((
            r.origin(),
            drawn(r.width().max(1.0), r.height().max(1.0), (r.width() as i64, r.height() as i64), |cx| {
                let t = cx.theme();
                let b = cx.bounds();
                cx.canvas.fill_rect(b, t.selection().with_alpha(0.22));
                aizome_kit::draw::border(cx.canvas, b, t.selection(), 1.0);
            })
            .into(),
        ));
    }
    aizome_kit::stack(vec![ground(wallpaper, size), free(placed)]).width(size.width).height(size.height).into()
}

/// Icons a rubber band from `a` to `b` takes in.
pub fn in_band(
    icons: &[DeskIcon],
    positions: &HashMap<String, Point>,
    height: f64,
    left: f64,
    a: Point,
    b: Point,
) -> Vec<String> {
    let band = Rect::from_points(a, b);
    icons
        .iter()
        .zip(layout(icons, positions, height, left))
        .filter(|(_, p)| band.intersect(Rect::from_origin_size(*p, (ICON_W, ICON_H))).area() > 0.0)
        .map(|(i, _)| i.key.clone())
        .collect()
}

/// The icon under a point.
pub fn icon_at(
    icons: &[DeskIcon],
    positions: &HashMap<String, Point>,
    height: f64,
    left: f64,
    at: Point,
) -> Option<String> {
    icons
        .iter()
        .zip(layout(icons, positions, height, left))
        .find(|(_, p)| Rect::from_origin_size(*p, (ICON_W, ICON_H)).contains(at))
        .map(|(i, _)| i.key.clone())
}

/// The wallpaper for a theme: the settings', else `AIZOME_WALLPAPER_NIGHT`
/// or `_DAY`. An explicit empty setting means no picture.
pub fn wallpaper_path(settings: &aizome_config::Settings, day: bool) -> Option<PathBuf> {
    settings.appearance.wallpaper_path(day).filter(|p| p.is_file())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn icon(key: &str) -> DeskIcon {
        DeskIcon {
            key: key.into(),
            label: key.into(),
            glyph: Icon::File,
            hue: Hue::Text,
            kind: IconKind::Trash { full: false },
        }
    }

    #[test]
    fn icons_line_up_down_the_left_and_wrap() {
        let icons: Vec<DeskIcon> = (0..5).map(|i| icon(&format!("i{i}"))).collect();
        let pts = layout(&icons, &HashMap::new(), TOP + 3.0 * STEP + 8.0, 0.0);
        assert_eq!(pts[0], Point::new(LEFT, TOP));
        assert_eq!(pts[1], Point::new(LEFT, TOP + STEP));
        assert_eq!(pts[3], Point::new(LEFT + ICON_W + 8.0, TOP), "a new column");
        let mut put = HashMap::new();
        put.insert("i0".to_owned(), Point::new(500.0, 300.0));
        let pts = layout(&icons, &put, 900.0, 0.0);
        assert_eq!(pts[0], Point::new(500.0, 300.0));
        assert_eq!(pts[1], Point::new(LEFT, TOP), "the others close up");
        assert_eq!(in_band(&icons, &put, 900.0, 0.0, Point::new(0.0, 0.0), Point::new(60.0, 140.0)), ["i1", "i2"]);
        assert_eq!(icon_at(&icons, &put, 900.0, 0.0, Point::new(520.0, 310.0)).as_deref(), Some("i0"));
    }

    #[test]
    fn the_board_icons() {
        let v = aizome_sys::disks::Volume { block: "/b/nvme0".into(), name: "nvme0".into(), ..Default::default() };
        let img = aizome_sys::disks::Volume {
            block: "/b/loop0".into(),
            name: "x".into(),
            image_file: Some("/home/u/sekka-nightly.iso".into()),
            ..Default::default()
        };
        let icons = icons("callen", &[v, img], false, &[]);
        let labels: Vec<&str> = icons.iter().map(|i| i.label.as_str()).collect();
        assert_eq!(labels, ["callen", "nvme0", "sekka-nightly.iso", "Trash"]);
        assert_eq!(icons[1].glyph, Icon::HardDrive);
        assert_eq!(icons[2].glyph, Icon::Disc);
    }
}

//! What Shashin's window shows: the toolbar, the stage (the picture on
//! `sunk`), the filmstrip, Get Info, and the menus.

use std::cell::Cell;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::SystemTime;

use aizome_kit::MsgSender;
use aizome_kit::column;
use aizome_kit::prelude::*;
use aizome_kit::sidestep_ui::{BlendMode, Canvas, ImageOptions, Key, NamedKey, PointerButton};
use aizome_kit::widget::Event;

use crate::app::{Body, Msg, Seen, Shashin, Thumb, Tile, Wallpaper, same_zoom};
use crate::folder;
use crate::view::{self, Zoom};

/// The zoom pop-up's choices: Fit, then fixed zooms.
pub const ZOOM_CHOICES: [Option<f64>; 7] = [None, Some(0.25), Some(0.5), Some(1.0), Some(2.0), Some(4.0), Some(8.0)];

pub fn window(app: &Shashin) -> Element<Msg> {
    // The stage keeps its key (and its focus) in and out of full screen.
    let stage = stage_area(app);
    if app.fullscreen {
        return aizome_kit::widgets::layout::column(vec![stage]).into();
    }
    let mut parts = vec![top_bar(app), stage];
    if app.filmstrip {
        parts.push(filmstrip(app));
    }
    aizome_kit::widgets::layout::column(parts).into()
}

fn tool(icon: Icon, tip: &str, msg: Msg, enabled: bool) -> Element<Msg> {
    icon_button(icon).toolbar().on_press(msg).enabled(enabled).tooltip(tip).into()
}

fn top_bar(app: &Shashin) -> Element<Msg> {
    let have = app.pic.as_ref().is_some_and(|p| p.info.error.is_none());
    let many = app.folder.len() > 1;
    let z = app.z();
    // The zoom pop-up: Fit, the fixed zooms, and the zoom now if it's
    // none of them.
    let mut labels: Vec<String> =
        ZOOM_CHOICES.iter().map(|c| c.map_or("Fit".to_owned(), |z| format!("{:.0}%", z * 100.0))).collect();
    let selected = match app.zoom {
        Zoom::Fit => Some(0),
        Zoom::Fixed(f) => ZOOM_CHOICES.iter().position(|c| c.is_some_and(|c| same_zoom(c, f))),
    };
    let (selected, offset) = match selected {
        Some(i) => (i, 0usize),
        None => {
            labels.insert(0, app.percent());
            (0, 1)
        }
    };
    let status: Element<Msg> = match &app.note {
        Some(n) => text(n.text.clone()).role(if n.error { Role::Danger } else { Role::Text2 }).into(),
        None => status_text(status_line(app, z)),
    };
    toolbar(vec![
        tool(Icon::ChevronLeft, "Previous Picture  ←", Msg::Step(-1), many),
        tool(Icon::ChevronRight, "Next Picture  →", Msg::Step(1), many),
        toolbar_separator(),
        tool(Icon::ZoomOut, "Zoom Out  ❖−", Msg::ZoomOut, have),
        dropdown(labels, selected)
            .compact()
            .enabled(have)
            .on_select(move |i| Msg::ZoomPick(i.wrapping_sub(offset)))
            .width(76.0)
            .into(),
        tool(Icon::ZoomIn, "Zoom In  ❖+", Msg::ZoomIn, have),
        toolbar_separator(),
        tool(Icon::RotateCcw, "Rotate Left  ❖L", Msg::Rotate(-1), have),
        tool(Icon::RotateCw, "Rotate Right  ❖R", Msg::Rotate(1), have),
        toolbar_separator(),
        icon_button(Icon::Film)
            .toolbar()
            .selected(app.filmstrip)
            .on_press(Msg::Filmstrip)
            .tooltip("Filmstrip  F")
            .into(),
        tool(Icon::Play, "Slideshow  ⇧❖F", Msg::Slideshow, have),
        tool(Icon::Info, "Get Info  ❖I", Msg::Info, have),
        spacer().into(),
        status,
        space(4.0),
    ])
}

/// `4032 × 3024 · 37% · 12 of 48`.
pub fn status_line(app: &Shashin, z: f64) -> String {
    let mut parts = Vec::new();
    if let Some(p) = &app.pic
        && p.info.dims.0 > 0
    {
        let (w, h) = app.shown_dims();
        parts.push(format!("{w:.0} × {h:.0}"));
        if p.info.error.is_none() {
            let pct = z * 100.0;
            parts.push(if pct >= 10.0 { format!("{pct:.0}%") } else { format!("{pct:.1}%") });
        }
    }
    if let Some(i) = app.index
        && app.folder.len() > 1
    {
        parts.push(format!("{} of {}", i + 1, app.folder.len()));
    }
    parts.join(" · ")
}

// The stage.

/// Reports what the stage drew to the program.
#[derive(Clone)]
pub struct Reporter {
    pub tx: MsgSender<Msg>,
    pub last: Rc<Cell<u64>>,
}

/// What the stage draws: the picture's drawings, where it sits, and how
/// large.
#[derive(Clone)]
pub struct Stage {
    pub serial: u64,
    /// The picture's pixels, turned.
    pub dims: (f64, f64),
    pub turns: u8,
    pub zoom: Zoom,
    pub center: (f64, f64),
    pub tiles: Rc<Vec<Tile>>,
    /// An animation's frame (unturned), drawn in place of tiles.
    pub frame: Option<(Image, usize)>,
    pub raster: bool,
    /// The view and scale the program last knew.
    pub assumed: (Size, f64),
    /// Not hashed: they don't change the drawing.
    pub panning: bool,
    pub report: Option<Reporter>,
}

impl Hash for Stage {
    fn hash<H: Hasher>(&self, h: &mut H) {
        self.serial.hash(h);
        self.dims.0.to_bits().hash(h);
        self.dims.1.to_bits().hash(h);
        self.turns.hash(h);
        self.zoom.bits().hash(h);
        self.center.0.to_bits().hash(h);
        self.center.1.to_bits().hash(h);
        for t in self.tiles.iter() {
            t.id.hash(h);
        }
        self.frame.as_ref().map(|f| f.1).hash(h);
    }
}

pub fn stage_of(app: &Shashin) -> Stage {
    let pic = app.pic.as_ref();
    let frame = pic.and_then(|p| match &p.info.body {
        Some(Body::Frames(frames, _)) => frames.get(p.frame).map(|f| (f.0.clone(), p.frame)),
        _ => None,
    });
    Stage {
        serial: pic.map_or(0, |p| p.serial),
        dims: app.shown_dims(),
        turns: app.current_turns(),
        zoom: app.zoom,
        center: app.center,
        tiles: Rc::new(pic.map(|p| p.tiles.clone()).unwrap_or_default()),
        frame,
        raster: matches!(pic.and_then(|p| p.info.body.as_ref()), Some(Body::Raster(..))),
        assumed: (app.view, app.scale),
        panning: app.panning.is_some(),
        report: app.stream.clone().map(|tx| Reporter { tx, last: app.reported.clone() }),
    }
}

fn stage_area(app: &Shashin) -> Element<Msg> {
    let st = stage_of(app);
    let canvas = canvas(st, paint_stage)
        .on_event(stage_event)
        .focusable()
        .focus_ring(aizome_kit::widget::FocusRing::Never)
        .id("stage")
        .label("Picture");
    let mut layers: Vec<Element<Msg>> = vec![canvas.into()];
    match &app.pic {
        None => layers.push(
            container(
                column![
                    icon(Icon::Image).size(48.0).role(Role::Faint),
                    text("No picture").size(15.0).semibold().role(Role::Text2),
                    text("Open one with ❖O, or drop it here.").role(Role::Text3),
                ]
                .spacing(8.0)
                .align(Align::Center),
            )
            .center()
            .width(Fill)
            .height(Fill)
            .into(),
        ),
        Some(p) if p.info.error.is_some() => layers.push(
            container(
                column![
                    icon(Icon::CircleAlert).size(40.0).role(Role::Faint),
                    text(format!("Shashin can't open “{}”", folder::display_name(&p.info.path)))
                        .size(15.0)
                        .semibold()
                        .role(Role::Text2),
                    text(p.info.error.clone().unwrap_or_default()).role(Role::Text3),
                ]
                .spacing(8.0)
                .align(Align::Center),
            )
            .center()
            .width(Fill)
            .height(Fill)
            .into(),
        ),
        _ => {}
    }
    mouse_area(aizome_kit::widgets::layout::stack(layers).width(Fill).height(Fill))
        .on_drop(|d| Msg::Open(d.urls.iter().filter_map(|u| file_path(u)).collect()))
        .key("stage")
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

fn stage_event(ev: &Event, size: Size, st: &Stage) -> Option<Msg> {
    match ev {
        Event::PointerDown { pos, button: PointerButton::Left, clicks, .. } => Some(if *clicks >= 2 {
            Msg::DoubleClick { at: *pos, view: size }
        } else {
            Msg::PanStart { at: *pos, view: size }
        }),
        Event::PointerMove { pos, .. } if st.panning => Some(Msg::PanTo(*pos)),
        Event::PointerUp { button: PointerButton::Left, .. } if st.panning => Some(Msg::PanEnd),
        Event::Scroll { pos, delta, mods, .. } => {
            if mods.super_key() || mods.control() {
                // A wheel's notch (40pt) zooms by a quarter.
                Some(Msg::ZoomBy { factor: 1.25f64.powf(-delta.y / aizome_kit::LINE), at: *pos, view: size })
            } else {
                Some(Msg::Scroll { by: *delta, view: size })
            }
        }
        Event::Pinch { pos, scale, .. } => Some(Msg::ZoomBy { factor: *scale, at: *pos, view: size }),
        Event::Key(k) if k.pressed => {
            let m = k.modifiers;
            match &k.key {
                // ❖= for ❖+ on keyboards where + takes Shift.
                Key::Character(s) if m.super_key() && s == "=" => Some(Msg::ZoomIn),
                // The menus' keys without a modifier (the kit matches only
                // those with ❖ or ⌃).
                _ if m.super_key() || m.control() || m.alt() => None,
                Key::Named(NamedKey::Escape) => Some(Msg::Escape),
                Key::Named(NamedKey::ArrowLeft) => Some(Msg::Step(-1)),
                Key::Named(NamedKey::ArrowRight) => Some(Msg::Step(1)),
                Key::Named(NamedKey::Home) => Some(Msg::First),
                Key::Named(NamedKey::End) => Some(Msg::Last),
                Key::Character(s) if s == " " => Some(Msg::Step(if m.shift() { -1 } else { 1 })),
                Key::Character(s) if s.eq_ignore_ascii_case("f") => Some(Msg::Filmstrip),
                _ => None,
            }
        }
        _ => None,
    }
}

/// Draw the picture: an exact drawing for this zoom one to one where
/// there is one, else the nearest-pixel best of what there is, and ask
/// the program for the exact one.
pub fn paint_stage(c: &mut Canvas, bounds: Rect, st: &Stage, theme: &Theme) {
    if st.dims.0 <= 0.0 || st.dims.1 <= 0.0 {
        c.fill_rect(bounds, theme.sunk());
        return;
    }
    let s = c.scale();
    let view = (bounds.width() * s, bounds.height() * s);
    let z = st.zoom.resolve(st.dims, view);
    let center = view::clamp_center(st.dims, view, z, st.center);
    let o = view::origin(st.dims, view, z, center);
    let vis = view::visible(st.dims, view, z, center);
    let to_pt = |x: f64, y: f64| Point::new(bounds.x0 + x / s, bounds.y0 + y / s);
    // What to draw: a drawing made for this zoom where there is one.
    let mine = |t: &&Tile| t.turns == st.turns;
    let full = st.tiles.iter().filter(mine).find(|t| same_zoom(t.z, 1.0) && t.whole(st.dims));
    let (plan, exact): (Option<&Tile>, bool) = if st.frame.is_some() {
        (None, true)
    } else if let Some(t) = st.tiles.iter().filter(mine).rev().find(|t| t.covers(z, vis)) {
        (Some(t), true)
    } else if let (Some(t), true) = (full, st.raster && z > 1.0) {
        // Over 200% the full picture's nearest pixels are the drawing.
        (Some(t), z > 2.0 + 1e-9)
    } else {
        (best(st, z), false)
    };
    // The ground: only round the picture when an opaque drawing covers it
    // (a photo's pixels needn't be filled twice).
    let covered = plan.is_some_and(|t| t.opaque && (t.whole(st.dims) || t.covers(z, vis)));
    if covered {
        let (w, h) = (st.dims.0 * z, st.dims.1 * z);
        let (x0, y0) = (o.0.max(0.0), o.1.max(0.0));
        let (x1, y1) = ((o.0 + w).min(view.0), (o.1 + h).min(view.1));
        for (a, b, cc, d) in
            [(0.0, 0.0, view.0, y0), (0.0, y1, view.0, view.1), (0.0, y0, x0, y1), (x1, y0, view.0, y1)]
        {
            if cc > a && d > b {
                c.fill_rect(Rect::from_points(to_pt(a, b), to_pt(cc, d)), theme.sunk());
            }
        }
    } else {
        c.fill_rect(bounds, theme.sunk());
    }
    if let Some((frame, _)) = &st.frame {
        // Animations are small: drawn as they are, turned by the canvas.
        let (fw, fh) = {
            let (w, h) = frame.pixel_size();
            (f64::from(w), f64::from(h))
        };
        draw_turned(c, frame, (fw, fh), st.turns, to_pt(o.0, o.1), z / s, st.dims, z <= 2.0);
    } else if let Some(t) = plan {
        draw_tile(c, t, z, o, s, bounds, false);
    } else if let Some(t) = st.tiles.iter().rev().find(|t| t.at == (0.0, 0.0)) {
        // Drawn for another turn: turned on the canvas meanwhile.
        let delta = (st.turns + 4 - t.turns % 4) % 4;
        let (w, h) = t.size();
        let scale = z / t.z / s;
        draw_turned(c, &t.image, (w, h), delta, to_pt(o.0, o.1), scale, st.dims, false);
    }
    if let Some(r) = &st.report {
        let changed_view = (bounds.size(), s) != st.assumed;
        if !exact || changed_view {
            let seen =
                Seen { serial: st.serial, turns: st.turns, view: bounds.size(), scale: s, z, visible: vis, exact };
            let key = {
                let mut h = std::collections::hash_map::DefaultHasher::new();
                (
                    seen.serial,
                    seen.turns,
                    z.to_bits(),
                    seen.view.width.to_bits(),
                    seen.view.height.to_bits(),
                    s.to_bits(),
                    exact,
                )
                    .hash(&mut h);
                for v in [vis.0, vis.1, vis.2, vis.3] {
                    v.round().to_bits().hash(&mut h);
                }
                h.finish()
            };
            if r.last.get() != key {
                r.last.set(key);
                r.tx.send(Msg::Seen(seen));
            }
        }
    }
}

/// The tile closest to `z` that covers the most: one made larger first
/// (drawn smaller it loses least), else the largest.
fn best(st: &Stage, z: f64) -> Option<&Tile> {
    let mine: Vec<&Tile> = st.tiles.iter().filter(|t| t.turns == st.turns && t.whole(st.dims)).collect();
    mine.iter()
        .filter(|t| t.z >= z)
        .min_by(|a, b| a.z.total_cmp(&b.z))
        .or_else(|| mine.iter().max_by(|a, b| a.z.total_cmp(&b.z)))
        .copied()
        .or_else(|| st.tiles.iter().rev().find(|t| t.turns == st.turns))
}

/// Draw tile `t` for zoom `z`, the picture's corner at `o` (device pixels
/// in the view).
fn draw_tile(c: &mut Canvas, t: &Tile, z: f64, o: (f64, f64), s: f64, bounds: Rect, smooth: bool) {
    let k = z / t.z;
    let (w, h) = t.size();
    let x = o.0 + (t.at.0 * k).round();
    let y = o.1 + (t.at.1 * k).round();
    let dst = Rect::new(
        bounds.x0 + x / s,
        bounds.y0 + y / s,
        bounds.x0 + (x + (w * k).round()) / s,
        bounds.y0 + (y + (h * k).round()) / s,
    );
    if t.opaque {
        // In place of what's there: no blending, a quarter less work.
        c.save();
        c.set_blend_mode(BlendMode::Copy);
        c.draw_image_with(&t.image, None, dst, ImageOptions { smooth, ..ImageOptions::default() });
        c.restore();
    } else {
        c.draw_image_with(&t.image, None, dst, ImageOptions { smooth, ..ImageOptions::default() });
    }
}

/// Draw `image` (of `size` pixels) turned `turns` quarter turns, scaled by
/// `scale` points a pixel, filling the picture's place: its corner at
/// `at`, `dims` (device pixels at zoom 1) its size turned.
#[allow(clippy::too_many_arguments)]
fn draw_turned(
    c: &mut Canvas,
    image: &Image,
    size: (f64, f64),
    turns: u8,
    at: Point,
    scale: f64,
    dims: (f64, f64),
    smooth: bool,
) {
    let (w, h) = (size.0 * scale, size.1 * scale);
    let shown = if turns % 2 == 1 { (h, w) } else { (w, h) };
    let _ = dims;
    let center = Point::new(at.x + shown.0 / 2.0, at.y + shown.1 / 2.0);
    c.save();
    c.translate(center.to_vec2());
    c.rotate(f64::from(turns % 4) * std::f64::consts::FRAC_PI_2);
    c.draw_image_with(
        image,
        None,
        Rect::new(-w / 2.0, -h / 2.0, w / 2.0, h / 2.0),
        ImageOptions { smooth, ..ImageOptions::default() },
    );
    c.restore();
}

// The filmstrip.

#[derive(Clone)]
struct Strip {
    index: usize,
    count: usize,
    /// The thumbnails from `first` on, as many as might show.
    first: usize,
    thumbs: Rc<Vec<Option<Image>>>,
    ids: u64,
}

impl Hash for Strip {
    fn hash<H: Hasher>(&self, h: &mut H) {
        (self.index, self.count, self.first, self.ids).hash(h);
    }
}

/// A slot's width, and the thumbnail's box in it.
const SLOT: f64 = 72.0;
const THUMB: f64 = 56.0;
/// The most slots a filmstrip shows (wide windows centre the rest).
const SLOTS: usize = 41;

fn strip_of(app: &Shashin) -> Strip {
    let index = app.index.unwrap_or(0);
    let count = app.folder.len();
    let first = index.saturating_sub(SLOTS / 2);
    let last = (first + SLOTS).min(count);
    let mut ids: u64 = 0;
    let thumbs: Vec<Option<Image>> = app.folder[first..last]
        .iter()
        .enumerate()
        .map(|(i, p)| match app.thumbs.get(p) {
            Some(Thumb::Ready(img)) => {
                ids = ids.wrapping_mul(31).wrapping_add(i as u64 + 1);
                Some(img.clone())
            }
            _ => None,
        })
        .collect();
    Strip { index, count, first, thumbs: Rc::new(thumbs), ids }
}

fn filmstrip(app: &Shashin) -> Element<Msg> {
    let strip = strip_of(app);
    container(
        canvas(strip, paint_strip)
            .on_event(|ev, size, st: &Strip| match ev {
                Event::PointerDown { pos, button: PointerButton::Left, .. } => {
                    let x0 = strip_x0(st, size.width);
                    let slot = ((pos.x - x0) / SLOT).floor();
                    if slot < 0.0 {
                        return None;
                    }
                    let i = st.index as i64 + slot as i64 - (st.index - st.first) as i64;
                    (0..st.count as i64).contains(&i).then_some(Msg::Go(i as usize))
                }
                Event::Scroll { delta, .. } => {
                    let d = if delta.x.abs() > delta.y.abs() { delta.x } else { delta.y };
                    (d.abs() >= 1.0).then_some(Msg::Step(d.signum() as i32))
                }
                _ => None,
            })
            .height(FILMSTRIP_HEIGHT)
            .label("Filmstrip"),
    )
    .fill(Tone::Chrome)
    .edges(Edge::BevelLo, Sides::TOP)
    .height(FILMSTRIP_HEIGHT)
    .into()
}

const FILMSTRIP_HEIGHT: f64 = crate::app::FILMSTRIP;

/// Where the strip's first slot starts: the current one centred.
fn strip_x0(st: &Strip, width: f64) -> f64 {
    let current = (st.index - st.first) as f64;
    (width / 2.0 - SLOT / 2.0 - current * SLOT).round()
}

fn paint_strip(c: &mut Canvas, b: Rect, st: &Strip, theme: &Theme) {
    let x0 = b.x0 + strip_x0(st, b.width());
    let y = b.y0 + ((b.height() - THUMB) / 2.0).round();
    for (k, thumb) in st.thumbs.iter().enumerate() {
        let x = x0 + k as f64 * SLOT + (SLOT - THUMB) / 2.0;
        if x + THUMB < b.x0 || x > b.x1 {
            continue;
        }
        let cell = Rect::new(x, y, x + THUMB, y + THUMB);
        let current = st.first + k == st.index;
        if current {
            // The current picture: a 2pt focus edge round its box.
            c.fill_rect(cell.inflate(4.0, 4.0), theme.focus());
            c.fill_rect(cell.inflate(2.0, 2.0), theme.chrome());
        }
        match thumb {
            Some(img) => {
                let (w, h) = img.pixel_size();
                let (w, h) = (f64::from(w), f64::from(h));
                let k = (THUMB / w).min(THUMB / h).min(1.0);
                let (dw, dh) = ((w * k).round(), (h * k).round());
                let dst = Rect::new(x + ((THUMB - dw) / 2.0).round(), y + ((THUMB - dh) / 2.0).round(), 0.0, 0.0);
                let dst = Rect::new(dst.x0, dst.y0, dst.x0 + dw, dst.y0 + dh);
                c.draw_image_with(img, None, dst, ImageOptions::default());
            }
            None => c.fill_rect(cell.inflate(-6.0, -6.0), theme.sunk()),
        }
    }
}

// Get Info.

pub fn info(app: &Shashin) -> Element<Msg> {
    let Some(pic) = &app.pic else {
        return column![button("Done").default().on_press(Msg::CloseInfo).id("info-done")].padding(20.0).into();
    };
    let i = &pic.info;
    let value = |s: String| -> Element<Msg> { text(s).role(Role::Text2).selectable().into() };
    let (w, h) = app.shown_dims();
    let mut rows: Vec<Element<Msg>> = vec![
        form_row("Kind", value(format!("{} picture", i.kind.name()))).into(),
        form_row("Dimensions", value(format!("{w:.0} × {h:.0} pixels"))).into(),
        form_row("Size", value(format!("{} ({} bytes)", human_size(i.file_size), group(i.file_size)))).into(),
    ];
    if let Some(t) = i.created {
        rows.push(form_row("Created", value(when(t))).into());
    }
    if let Some(t) = i.modified {
        rows.push(form_row("Modified", value(when(t))).into());
    }
    rows.push(form_row("Where", value(abbreviate(i.path.parent().unwrap_or(Path::new("/"))))).into());
    let mut parts: Vec<Element<Msg>> = vec![
        aizome_kit::row![
            icon(Icon::FileImage).size(32.0).role(Role::Accent),
            text(folder::display_name(&i.path)).size(15.0).semibold().max_lines(2).wrap()
        ]
        .spacing(12.0)
        .into(),
        form_section("", rows),
    ];
    if !i.exif.is_empty() {
        let rows = i.exif.iter().map(|(k, v)| form_row(k.clone(), value(v.clone())).into()).collect();
        parts.push(form_section("Photo", rows));
    }
    if app.current_turns() != 0 {
        parts.push(text("Turned, not yet saved (❖S saves it).").role(Role::Text3).into());
    }
    parts.push(aizome_kit::row![spacer(), button("Done").default().on_press(Msg::CloseInfo).id("info-done")].into());
    aizome_kit::widgets::layout::column(parts).spacing(14.0).padding(20.0).width(460.0).into()
}

/// `8.4 MB`: decimal, as the desktop counts.
pub fn human_size(n: u64) -> String {
    let n = n as f64;
    if n < 1000.0 {
        format!("{n:.0} bytes")
    } else if n < 1e6 {
        format!("{:.0} KB", n / 1e3)
    } else if n < 1e9 {
        format!("{:.1} MB", n / 1e6)
    } else {
        format!("{:.2} GB", n / 1e9)
    }
}

/// `8,412,035`.
fn group(n: u64) -> String {
    let s = n.to_string();
    let mut out = String::new();
    for (i, c) in s.chars().enumerate() {
        if i > 0 && (s.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    out
}

/// `~/Pictures/Trips`.
fn abbreviate(p: &Path) -> String {
    let home = std::env::var_os("HOME").map(PathBuf::from);
    match home.and_then(|h| p.strip_prefix(&h).ok().map(Path::to_path_buf)) {
        Some(rest) if rest.as_os_str().is_empty() => "~".into(),
        Some(rest) => format!("~/{}", rest.display()),
        None => p.display().to_string(),
    }
}

/// `17 October 2026, 21:42`, in local time.
pub fn when(t: SystemTime) -> String {
    let secs = t.duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0);
    let local = secs + crate::trash::local_offset(secs);
    let (y, m, d) = crate::trash::civil(local.div_euclid(86_400));
    let rem = local.rem_euclid(86_400);
    const MONTHS: [&str; 12] = [
        "January",
        "February",
        "March",
        "April",
        "May",
        "June",
        "July",
        "August",
        "September",
        "October",
        "November",
        "December",
    ];
    format!("{d} {} {y}, {:02}:{:02}", MONTHS[(m as usize).clamp(1, 12) - 1], rem / 3600, rem % 3600 / 60)
}

// Menus.

pub fn menus(app: &Shashin) -> MenuBar<Msg> {
    let have = app.pic.as_ref().is_some_and(|p| p.info.error.is_none() && p.decoded);
    let any = app.current_path().is_some();
    let turned = app.current_turns() != 0;
    let many = app.folder.len() > 1;
    let file = Menu::new("File")
        .item(MenuItem::new("Open…", Msg::OpenPanel).shortcut(Shortcut::cmd('O')))
        .separator()
        .item(MenuItem::new("Save", Msg::Save).shortcut(Shortcut::cmd('S')).enabled(turned))
        .item(MenuItem::new("Revert to Saved", Msg::Revert).enabled(turned))
        .separator()
        .item(MenuItem::new("Show in Hako", Msg::ShowInHako).shortcut(Shortcut::cmd('R').alt()).enabled(any))
        .item(MenuItem::new("Get Info", Msg::Info).shortcut(Shortcut::cmd('I')).enabled(any))
        .separator()
        .item(
            MenuItem::new("Move to Trash", Msg::Trash).shortcut(Shortcut::cmd_key(ShortcutKey::Backspace)).enabled(any),
        )
        .separator()
        .item(MenuItem::role_item("Close Window", MenuRole::CloseWindow).shortcut(Shortcut::cmd('W')));
    let undo_label = if app.undo.is_empty() { "Undo".to_owned() } else { "Undo Move to Trash".to_owned() };
    let redo_label = if app.redo.is_empty() { "Redo".to_owned() } else { "Redo Move to Trash".to_owned() };
    let edit = Menu::new("Edit")
        .item(MenuItem::new(undo_label, Msg::Undo).shortcut(Shortcut::cmd('Z')).enabled(!app.undo.is_empty()))
        .item(MenuItem::new(redo_label, Msg::Redo).shortcut(Shortcut::cmd('Z').shift()).enabled(!app.redo.is_empty()))
        .separator()
        .item(MenuItem::new("Copy", Msg::Copy).shortcut(Shortcut::cmd('C')).enabled(have))
        .separator()
        .submenu(
            Menu::new("Set as Wallpaper")
                .item(MenuItem::new("Night", Msg::SetWallpaper(Wallpaper::Night)).enabled(any))
                .item(MenuItem::new("Day", Msg::SetWallpaper(Wallpaper::Day)).enabled(any))
                .item(MenuItem::new("Both", Msg::SetWallpaper(Wallpaper::Both)).enabled(any)),
        );
    let view = Menu::new("View")
        .item(MenuItem::new("Zoom In", Msg::ZoomIn).shortcut(Shortcut::cmd('+')).enabled(have))
        .item(MenuItem::new("Zoom Out", Msg::ZoomOut).shortcut(Shortcut::cmd('-')).enabled(have))
        .item(
            MenuItem::new("Actual Size", Msg::ActualSize)
                .shortcut(Shortcut::cmd('0'))
                .enabled(have)
                .checked(app.zoom == Zoom::Fixed(1.0)),
        )
        .item(
            MenuItem::new("Zoom to Fit", Msg::Fit)
                .shortcut(Shortcut::cmd('9'))
                .enabled(have)
                .checked(app.zoom == Zoom::Fit),
        )
        .separator()
        .item(
            MenuItem::new("Show Filmstrip", Msg::Filmstrip)
                .shortcut(Shortcut::key(ShortcutKey::Char('f')))
                .checked(app.filmstrip),
        )
        .separator()
        .item(
            MenuItem::new(if app.slideshow.is_some() { "Stop Slideshow" } else { "Slideshow" }, Msg::Slideshow)
                .shortcut(Shortcut::cmd('F').shift())
                .enabled(have),
        )
        .item(
            MenuItem::new(if app.fullscreen { "Exit Full Screen" } else { "Enter Full Screen" }, Msg::FullScreen)
                .shortcut(Shortcut::cmd('F').ctrl()),
        );
    let image = Menu::new("Image")
        .item(MenuItem::new("Rotate Left", Msg::Rotate(-1)).shortcut(Shortcut::cmd('L')).enabled(have))
        .item(MenuItem::new("Rotate Right", Msg::Rotate(1)).shortcut(Shortcut::cmd('R')).enabled(have));
    let go = Menu::new("Go")
        .item(MenuItem::new("Previous Picture", Msg::Step(-1)).shortcut(Shortcut::key(ShortcutKey::Left)).enabled(many))
        .item(MenuItem::new("Next Picture", Msg::Step(1)).shortcut(Shortcut::key(ShortcutKey::Right)).enabled(many))
        .separator()
        .item(MenuItem::new("First Picture", Msg::First).shortcut(Shortcut::key(ShortcutKey::Home)).enabled(many))
        .item(MenuItem::new("Last Picture", Msg::Last).shortcut(Shortcut::key(ShortcutKey::End)).enabled(many));
    MenuBar::standard(&aizome_kit::AppInfo::new(crate::app::APP_ID, "Shashin", "Images"))
        .replace("Edit", edit)
        .with_file_menu(file)
        .with_menu(view)
        .with_menu(image)
        .with_menu(go)
}

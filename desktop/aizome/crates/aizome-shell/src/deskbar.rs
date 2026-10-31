//! The Deskbar (C2-Desktop's aside): at the top right under the bar (or the
//! top left, `[deskbar] side`), each running app with its windows indented
//! beneath, unread notifications filed under their app, and the Shelf of
//! parked drags.

use aizome_config::DeskbarSide;
use aizome_ipc::WindowId;
use aizome_kit::draw;
use aizome_kit::element::Element;
use aizome_kit::layout::{Align, Point, Rect, Size};
use aizome_kit::widget::SemanticRole;
use aizome_kit::{Icon, Layoutable, column};
use aizome_theme::typeface;

use crate::look::{self, Hue};
use crate::widgets::drawn::{DragPhase, Press, drawn};
use crate::wm::WindowNote;

/// The Deskbar's default width, and its shadow's (away from the edge it
/// stands against, and under it).
pub const WIDTH: f64 = aizome_theme::metrics::DESKBAR_WIDTH;
pub const SHADOW: f64 = aizome_theme::metrics::SHADOW;
/// The full Deskbar's widths: narrower cuts its rows' text to nothing,
/// wider takes too much of a small screen.
pub const MIN_WIDTH: f64 = 200.0;
pub const MAX_WIDTH: f64 = 400.0;
/// Narrower than the full Deskbar: Slim from here, and the Rail (icons
/// alone) below it, at its one width.
pub const SLIM_WIDTH: f64 = 120.0;
pub const RAIL_WIDTH: f64 = 48.0;
/// The strip at the top to drag the Deskbar by.
pub const GRIP: f64 = 10.0;
/// The inner edge that takes a drag to resize.
const EDGE: f64 = 5.0;

/// How much the Deskbar shows, by its width.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Density {
    /// Icons alone: each app, a mark per window, unread counts.
    Rail,
    /// Everything, a size smaller.
    Slim,
    Full,
}

pub fn density(width: f64) -> Density {
    if width < SLIM_WIDTH {
        Density::Rail
    } else if width < MIN_WIDTH {
        Density::Slim
    } else {
        Density::Full
    }
}

/// The width to draw for a configured one: whole points, in range, the
/// Rail's below Slim's (half way down snaps to it).
pub fn clamp_width(w: f64) -> f64 {
    if !w.is_finite() {
        return WIDTH;
    }
    let w = w.round();
    if w < (SLIM_WIDTH + RAIL_WIDTH) / 2.0 { RAIL_WIDTH } else { w.clamp(SLIM_WIDTH, MAX_WIDTH) }
}

/// The most of a screen's width the Deskbar takes. Windows filled or split
/// beside it keep the rest, and the compositor sets them beside a Deskbar
/// only while it stands in one half of the screen.
pub const MAX_SHARE: f64 = 0.45;

/// The widest the Deskbar is on a screen `screen` points wide: its share,
/// in whole points, and never more than [`MAX_WIDTH`].
pub fn most_width(screen: f64) -> f64 {
    if screen.is_finite() && screen > 0.0 { (screen * MAX_SHARE).floor().min(MAX_WIDTH) } else { MAX_WIDTH }
}

/// [`clamp_width`], and no wider than `most` (a narrow screen's share):
/// `most` itself when the width asked for is more, and the Rail when not
/// even Slim fits.
pub fn fit_width(w: f64, most: f64) -> f64 {
    let w = clamp_width(w);
    if w <= most {
        return w;
    }
    let w = clamp_width(most);
    if w <= most { w } else { RAIL_WIDTH }
}
const APP_ROW: f64 = 26.0;
const WINDOW_ROW: f64 = 22.0;
const CARD: f64 = 62.0;
const CARD_GAP: f64 = 6.0;
/// The row of a card's action buttons.
const BUTTONS: f64 = 26.0;
const SHELF_ROW: f64 = 26.0;
const SHELF_GAP: f64 = 4.0;
const CAPTION: f64 = 27.0;

/// What the Deskbar's elements say.
#[derive(Clone, Debug, PartialEq)]
pub enum DeskbarMsg {
    FocusApp(String),
    AppMenu(String, Point),
    /// Rearrange app groups: row center in content coordinates, pointer in
    /// the row's current local coordinates (including scroll displacement).
    AppDrag(String, f64, DragPhase),
    AutoHide(u64),
    MenuClosed(u64),
    /// Fold or unfold an app's windows.
    Fold(String),
    FocusWindow(WindowId),
    CloseWindow(WindowId),
    WindowDrag(WindowId, DragPhase),
    /// A notification's default action (or show it).
    OpenNote(u32),
    DismissNote(u32),
    NoteAction(u32, String),
    /// Show every unread notification of an app.
    ShowNotes(String),
    ShelfOpen(usize),
    ShelfMenu(usize, Point),
    /// Something dropped on "Drop to park": URLs and text.
    Park(Vec<String>, Option<String>),
    /// The grip dragged: let go over the other half of the screen, the
    /// Deskbar moves to that edge.
    Grip(DragPhase),
    /// The inner edge dragged: the width follows the pointer.
    Edge(DragPhase),
    /// The grip's menu: which edge, how wide.
    PlaceMenu(Point),
}

/// A notification's card under its app.
#[derive(Clone, Debug, PartialEq)]
pub struct NoteCard {
    pub id: u32,
    pub summary: String,
    pub body: String,
    pub buttons: Vec<(String, String)>,
}

/// An app's row and what's under it.
#[derive(Clone, Debug, PartialEq)]
pub struct AppRow {
    pub app_id: String,
    pub name: String,
    pub glyph: Icon,
    pub hue: Hue,
    /// The app's own icon, when it has no glyph here.
    pub image: Option<crate::look::Pic>,
    pub windows: Vec<WindowRow>,
    /// Its windows are folded under "N ▸".
    pub folded: bool,
    pub focused: bool,
    pub unread: usize,
    /// The cards shown: the newest, or all of them when asked.
    pub cards: Vec<NoteCard>,
}

impl AppRow {
    fn height(&self) -> f64 {
        APP_ROW
            + if self.folded { 0.0 } else { self.windows.len() as f64 * WINDOW_ROW }
            + self.cards.iter().map(|c| CARD + CARD_GAP + if c.buttons.is_empty() { 0.0 } else { BUTTONS }).sum::<f64>()
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct WindowRow {
    pub id: WindowId,
    pub title: String,
    pub focused: bool,
    pub note: WindowNote,
    pub urgent: bool,
}

/// A parked thing's row.
#[derive(Clone, Debug, PartialEq)]
pub struct ShelfRow {
    pub label: String,
    pub glyph: Icon,
    pub hue: Hue,
    pub dim: bool,
    /// What dragging it out carries: its URL (files, links) and text.
    pub uri: Option<String>,
    pub drag_text: String,
}

/// Where a drag of the grip stands.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Moving {
    #[default]
    No,
    /// Held over its own half of the screen: let go, it stays.
    Held,
    /// Over the other half: let go, it moves to that edge.
    Leaving,
}

#[derive(Clone, Debug, PartialEq)]
pub struct DeskbarModel {
    pub apps: Vec<AppRow>,
    pub shelf: Vec<ShelfRow>,
    /// `[deskbar] show_shelf = false`: no Shelf under the apps.
    pub hide_shelf: bool,
    /// Room below the menu bar on the owning output, including the shadow.
    pub max_height: Option<f64>,
    /// `[deskbar] side`: the edge it stands against.
    pub side: DeskbarSide,
    /// `[deskbar] width`, clamped: the face's, without the shadow.
    pub width: f64,
    /// Its grip dragged: held, then leaving once letting go would move it.
    pub moving: Moving,
}

impl Default for DeskbarModel {
    fn default() -> Self {
        DeskbarModel {
            apps: Vec::new(),
            shelf: Vec::new(),
            hide_shelf: false,
            max_height: None,
            side: DeskbarSide::Right,
            width: WIDTH,
            moving: Moving::No,
        }
    }
}

impl DeskbarModel {
    /// Where the face starts across the surface: after the shadow when it
    /// falls to the left (hanging from the right edge), at 0 otherwise.
    pub fn face_x(&self) -> f64 {
        match self.side {
            DeskbarSide::Right => SHADOW,
            DeskbarSide::Left => 0.0,
        }
    }

    pub fn density(&self) -> Density {
        density(self.width)
    }

    /// Its height, shadow included: the surface's.
    pub fn height(&self) -> f64 {
        self.max_height.map_or(self.content_height(), |h| self.content_height().min(h.max(SHADOW + 1.0)))
    }

    /// Unclipped height: overflowing content stays reachable by scrolling.
    pub fn content_height(&self) -> f64 {
        match self.density() {
            Density::Full => GRIP + self.full_height(),
            d => (GRIP + crate::deskbar_compact::height(self, d == Density::Rail)).ceil() + SHADOW,
        }
    }

    fn full_height(&self) -> f64 {
        let mut h = 4.0 + 2.0;
        for a in &self.apps {
            h += a.height();
        }
        if self.hide_shelf {
            return (h + 4.0).ceil() + SHADOW;
        }
        h += 1.0 + CAPTION;
        h += self.shelf.len() as f64 * (SHELF_ROW + SHELF_GAP) + SHELF_ROW + 10.0;
        (h + 1.0).ceil() + SHADOW
    }

    /// The app slot before which a dragged header should land.
    pub fn insertion_at(&self, y: f64) -> usize {
        let mut top = 4.0;
        for (i, app) in self.apps.iter().enumerate() {
            if y < top + APP_ROW / 2.0 {
                return i;
            }
            top += app.height();
        }
        self.apps.len()
    }
}

fn ui(t: &aizome_kit::Theme, size: f64, weight: f32, c: aizome_kit::Color) -> aizome_kit::text::TextSpec {
    look::ui(size * t.metrics.factor, weight, c)
}

fn app_row(a: &AppRow, top: f64, width: f64) -> Element<DeskbarMsg> {
    let (name, glyph, hue, n, folded, unread) = (a.name.clone(), a.glyph, a.hue, a.windows.len(), a.folded, a.unread);
    let image = a.image.clone();
    let id = a.app_id.clone();
    let id2 = a.app_id.clone();
    let id3 = a.app_id.clone();
    let drag_id = a.app_id.clone();
    let right = if unread > 0 {
        format!("{unread} new")
    } else if folded && n > 0 {
        format!("{n} ▸")
    } else if n > 0 {
        n.to_string()
    } else {
        String::new()
    };
    let label = name.clone();
    drawn(
        width,
        APP_ROW,
        (name.clone(), glyph, hue, right.clone(), unread > 0, image.as_ref().map(|i| i.0.pixel_size())),
        move |cx| {
            let t = cx.theme();
            let b = cx.bounds();
            if cx.is_hovered() {
                cx.canvas.fill_rect(b, t.hover(t.chrome()));
            }
            match &image {
                Some(img) => cx.canvas.draw_image(&img.0, Rect::new(8.0, 5.0, 24.0, 21.0)),
                None => glyph.draw(cx.canvas, Point::new(8.0, ((APP_ROW - 16.0) / 2.0).round()), 16.0, hue.color(t)),
            }
            let s = ui(t, typeface::BODY, typeface::WEIGHT_TAB, t.text());
            cx.draw_text_in(&name, &s, Rect::new(32.0, 0.0, b.x1 - 48.0, APP_ROW), Align::Start);
            let (spec, x1) = if unread > 0 {
                (look::digits(11.0 * t.metrics.factor, typeface::WEIGHT_SEMIBOLD, Hue::Red.color(t)), b.x1 - 10.0)
            } else {
                (look::digits(11.0 * t.metrics.factor, typeface::WEIGHT_REGULAR, t.text3()), b.x1 - 10.0)
            };
            cx.draw_text_in(&right, &spec, Rect::new(b.x1 - 60.0, 0.0, x1, APP_ROW), Align::End);
        },
    )
    .on_click(DeskbarMsg::FocusApp(id))
    .on_right_click(move |p: Press| DeskbarMsg::AppMenu(id2.clone(), p.at))
    .on_middle_click(DeskbarMsg::Fold(id3))
    .on_drag_local(move |phase| DeskbarMsg::AppDrag(drag_id.clone(), top + APP_ROW / 2.0, phase))
    .text(label, SemanticRole::ListItem)
    .tooltip("Drag to reorder; right-click for more options")
    .key(("app", a.app_id.as_str()))
    .into()
}

/// The count at an app's right, a target of its own: it folds the windows
/// (or, with notifications, shows them all).
fn count_hit(a: &AppRow) -> Element<DeskbarMsg> {
    let msg = if a.unread > 0 { DeskbarMsg::ShowNotes(a.app_id.clone()) } else { DeskbarMsg::Fold(a.app_id.clone()) };
    drawn(56.0, APP_ROW, "count", |_cx| {}).on_click(msg).key(("count", a.app_id.as_str())).into()
}

fn window_row(w: &WindowRow, width: f64) -> Element<DeskbarMsg> {
    let (title, focused, note, urgent) = (w.title.clone(), w.focused, w.note, w.urgent);
    let label = title.clone();
    drawn(width, WINDOW_ROW, (title.clone(), focused, note, urgent), move |cx| {
        let t = cx.theme();
        let b = cx.bounds();
        if focused {
            cx.canvas.fill_rect(b, look::mix(t.chrome(), t.focus(), 0.16));
            // The focus bar at the left, as the focused frame's rail.
            cx.canvas.fill_rect(Rect::new(18.0, 4.0, 21.0, 18.0), t.focus());
        } else if cx.is_hovered() {
            cx.canvas.fill_rect(b, t.hover(t.chrome()));
        }
        let hidden = note == WindowNote::Hidden;
        let color = if focused {
            t.text()
        } else if hidden {
            t.text3()
        } else {
            t.text2()
        };
        let mut s = ui(t, typeface::BODY, typeface::WEIGHT_REGULAR, color);
        s.font.italic = hidden;
        let note_w = if note == WindowNote::None { 0.0 } else { 52.0 };
        cx.draw_text_in(&title, &s, Rect::new(32.0, 0.0, b.x1 - 10.0 - note_w, WINDOW_ROW), Align::Start);
        if let Some(n) = note.label() {
            let mut ns = ui(t, typeface::SMALL, typeface::WEIGHT_REGULAR, t.text3());
            ns.font.italic = hidden;
            cx.draw_text_in(&n, &ns, Rect::new(b.x1 - 10.0 - note_w, 0.0, b.x1 - 10.0, WINDOW_ROW), Align::End);
        }
        if urgent {
            let c = Hue::Red.color(t);
            let dot = Rect::new(24.0, 9.0, 28.0, 13.0);
            cx.canvas.fill(&aizome_kit::sidestep_ui::kurbo::Circle::new(dot.center(), 2.5), c);
        }
    })
    .on_click(DeskbarMsg::FocusWindow(w.id))
    .on_middle_click(DeskbarMsg::CloseWindow(w.id))
    .on_drag({
        let id = w.id;
        move |p| DeskbarMsg::WindowDrag(id, p)
    })
    .text(label, SemanticRole::ListItem)
    .key(("window", w.id))
    .into()
}

/// A notification's card: the summary in 650, the body in two lines, a ×.
fn card(c: &NoteCard, width: f64) -> Element<DeskbarMsg> {
    let (summary, body) = (c.summary.clone(), c.body.clone());
    let label = format!("{} {}", c.summary, c.body);
    let card = drawn(width - 42.0, CARD, (summary.clone(), body.clone()), move |cx| {
        let t = cx.theme();
        let b = cx.bounds();
        cx.canvas.fill_rect(b, t.sunk());
        draw::border(cx.canvas, b, t.line(), 1.0);
        let s = ui(t, 12.0, typeface::WEIGHT_TAB, t.text());
        cx.draw_text_in(&summary, &s, Rect::new(9.0, 7.0, b.x1 - 20.0, 23.0), Align::Start);
        let mut bs = ui(t, 12.0, typeface::WEIGHT_REGULAR, t.text3());
        bs.wrap = aizome_kit::sidestep_ui::Wrap::Words;
        bs.max_lines = 2;
        let l = cx.layout_text(&body, &bs, Some(b.width() - 18.0));
        cx.canvas.draw_text(&l, Point::new(9.0, 24.0));
        // The ×.
        let x = Rect::new(b.x1 - 16.0, 6.0, b.x1 - 6.0, 16.0);
        let c = if cx.is_hovered() { t.text2() } else { t.faint() };
        Icon::X.draw(cx.canvas, x.origin(), 10.0, c);
    })
    .on_click(DeskbarMsg::OpenNote(c.id))
    .text(label, SemanticRole::Group);
    let close =
        drawn(18.0, 18.0, "x", |_| {}).on_click(DeskbarMsg::DismissNote(c.id)).text("Dismiss", SemanticRole::Button);
    let stacked: Element<DeskbarMsg> = aizome_kit::stack(vec![
        card.into(),
        aizome_kit::container(close).padding([2.0, 2.0, 0.0, width - 42.0 - 20.0]).into(),
    ])
    .into();
    let mut parts = vec![stacked];
    if !c.buttons.is_empty() {
        // The notification's actions, as buttons under it.
        let mut row: Vec<Element<DeskbarMsg>> = Vec::new();
        for (key, label) in c.buttons.iter().take(2) {
            row.push(
                aizome_kit::button(label.clone()).compact().on_press(DeskbarMsg::NoteAction(c.id, key.clone())).into(),
            );
        }
        parts.push(aizome_kit::row(row).spacing(6.0).height(BUTTONS).align(Align::Center).into());
    }
    aizome_kit::container(column(parts)).padding([0.0, 10.0, CARD_GAP, 32.0]).key(("note", c.id)).into()
}

fn caption(width: f64) -> Element<DeskbarMsg> {
    drawn(width, 1.0 + CAPTION, "shelf-caption", |cx| {
        let t = cx.theme();
        let b = cx.bounds();
        cx.canvas.fill_rect(Rect::new(0.0, 0.0, b.x1, 1.0), t.bevel_lo());
        cx.canvas.fill_rect(Rect::new(0.0, 1.0, b.x1, 2.0), t.bevel_hi());
        let s = look::caption(t);
        cx.draw_text_in("SHELF", &s, Rect::new(10.0, 11.0, b.x1, 1.0 + CAPTION - 3.0), Align::Start);
    })
    .text("Shelf", SemanticRole::Label)
    .into()
}

fn shelf_row(i: usize, r: &ShelfRow, width: f64) -> Element<DeskbarMsg> {
    let (label, glyph, hue, dim) = (r.label.clone(), r.glyph, r.hue, r.dim);
    let (uri, drag_text) = (r.uri.clone(), r.drag_text.clone());
    let text = label.clone();
    drawn(width - 20.0, SHELF_ROW, (label.clone(), glyph, hue, dim), move |cx| {
        let t = cx.theme();
        let b = cx.bounds();
        let fill = if cx.is_hovered() { t.hover(t.surface()) } else { t.surface() };
        cx.canvas.fill_rect(b, fill);
        draw::border(cx.canvas, b, t.line(), 1.0);
        glyph.draw(cx.canvas, Point::new(9.0, ((SHELF_ROW - 14.0) / 2.0).round()), 14.0, hue.color(t));
        let s = ui(t, 12.0, typeface::WEIGHT_REGULAR, if dim { t.text2() } else { t.text() });
        cx.draw_text_in(&label, &s, Rect::new(31.0, 0.0, b.x1 - 8.0, SHELF_ROW), Align::Start);
    })
    .on_double_click(DeskbarMsg::ShelfOpen(i))
    // Out to Hako, an editor, a mail: a copy (it stays on the Shelf).
    .drag_out(move || crate::shelf::drag_source(uri.as_deref(), &drag_text))
    .on_right_click(move |p: Press| DeskbarMsg::ShelfMenu(i, p.at))
    .text(text, SemanticRole::ListItem)
    .key(("shelf", i))
    .into()
}

fn drop_row(width: f64) -> Element<DeskbarMsg> {
    drawn(width - 20.0, SHELF_ROW, "drop", |cx| {
        let t = cx.theme();
        let b = cx.bounds();
        let style = aizome_kit::sidestep_ui::StrokeStyle::new(1.0).with_dash(vec![3.0, 2.0], 0.0);
        cx.canvas.stroke(&b.inset(-0.5), &style, t.line2());
        let s = ui(t, 11.5, typeface::WEIGHT_REGULAR, t.text3());
        cx.draw_text_in("Drop to park", &s, b, Align::Center);
    })
    .on_drop(DeskbarMsg::Park)
    .text("Drop to park", SemanticRole::Group)
    .key("drop")
    .into()
}

/// The grip's dots, across the top of `b`: three when it's the Rail's.
fn grip_dots(canvas: &mut aizome_kit::sidestep_ui::Canvas, b: Rect, c: aizome_kit::Color) {
    let n = if b.width() < SLIM_WIDTH { 3 } else { 5 };
    let x0 = b.x0 + (b.width() - (n as f64 * 4.0 - 2.0)) / 2.0;
    for i in 0..n {
        let x = (x0 + i as f64 * 4.0).round();
        canvas.fill_rect(Rect::new(x, b.y0 + 4.0, x + 2.0, b.y0 + 6.0), c);
    }
}

/// The strip to drag the Deskbar by: a row of dots, as Haiku's Deskbar
/// knob, darker under the pointer and pressed in, its dots in the focus
/// colour, while it's dragged. Right-click for where and how wide.
fn grip(width: f64, moving: Moving) -> Element<DeskbarMsg> {
    let held = moving != Moving::No;
    let el = drawn(width, GRIP, ("grip", width.to_bits(), held), move |cx| {
        let t = cx.theme();
        let b = cx.bounds();
        let hover = cx.is_hovered();
        if held {
            cx.canvas.fill_rect(b, t.pressed(t.chrome()));
        } else if hover {
            cx.canvas.fill_rect(b, t.hover(t.chrome()));
        }
        let c = if held {
            t.focus()
        } else if hover {
            t.text2()
        } else {
            t.faint()
        };
        grip_dots(cx.canvas, b, c);
    })
    .on_drag(DeskbarMsg::Grip)
    .on_right_click(|p: Press| DeskbarMsg::PlaceMenu(p.at))
    .cursor(aizome_kit::sidestep_ui::Cursor::Move)
    .text("Move the Deskbar", SemanticRole::Button)
    .key("grip");
    aizome_kit::tooltip(el, "Drag to move the Deskbar; drag its edge to resize; right-click for sizes")
}

/// The inner edge, a thin target over the rows: the resize cursor and the
/// focus colour while the pointer's on it. It takes only drags: a click
/// there (any button, or the wheel) is the row's under it.
fn edge(height: f64) -> Element<DeskbarMsg> {
    drawn(EDGE, height, ("edge", height.to_bits()), |cx| {
        if cx.is_hovered() {
            let t = cx.theme();
            let b = cx.bounds();
            cx.canvas.fill_rect(Rect::new(1.0, 0.0, 3.0, b.y1), t.focus());
        }
    })
    .on_drag(DeskbarMsg::Edge)
    .drags_only()
    .cursor(aizome_kit::sidestep_ui::Cursor::EwResize)
    .text("Resize the Deskbar", SemanticRole::Button)
    .key("edge")
    .into()
}

/// The full Deskbar's rows, under the grip.
fn full_rows(m: &DeskbarModel) -> Vec<Element<DeskbarMsg>> {
    let width = m.width;
    let mut rows: Vec<Element<DeskbarMsg>> = vec![aizome_kit::space(4.0)];
    let mut top = 4.0;
    for a in &m.apps {
        rows.push(
            aizome_kit::stack(vec![
                app_row(a, top, width),
                aizome_kit::container(count_hit(a)).padding([0.0, 0.0, 0.0, width - 56.0]).into(),
            ])
            .into(),
        );
        if !a.folded {
            for w in &a.windows {
                rows.push(window_row(w, width));
            }
        }
        for c in &a.cards {
            rows.push(card(c, width));
        }
        top += a.height();
    }
    if m.hide_shelf {
        rows.push(aizome_kit::space(4.0));
    } else {
        rows.push(aizome_kit::space(2.0));
        rows.push(caption(width));
        let mut shelf: Vec<Element<DeskbarMsg>> =
            m.shelf.iter().enumerate().map(|(i, r)| shelf_row(i, r, width)).collect();
        shelf.push(drop_row(width));
        rows.push(aizome_kit::container(column(shelf).spacing(SHELF_GAP)).padding([0.0, 10.0, 10.0, 10.0]).into());
    }
    rows
}

/// The Deskbar: the grip, and under it the rows, scrolling when they
/// overflow the room below the bar.
pub fn view(m: &DeskbarModel) -> Element<DeskbarMsg> {
    let width = m.width;
    let rows = match m.density() {
        Density::Full => full_rows(m),
        d => crate::deskbar_compact::rows(m, d == Density::Rail),
    };
    let h = m.height();
    let list = aizome_kit::scroll(column(rows).width(width)).width(width).height((h - SHADOW - GRIP).max(1.0));
    let content = column(vec![grip(width, m.moving), list.into()]).width(width);
    let side = m.side;
    let face_x = m.face_x();
    // The ground: `chrome`, its edges toward the screen and the bottom, the
    // shadow away from the edge it hangs from, and down.
    let ground = drawn(width + SHADOW, h, ("deskbar-ground", h.to_bits(), width.to_bits(), side), move |cx| {
        let t = cx.theme();
        let s = SHADOW;
        let face = Rect::new(face_x, 0.0, face_x + width, h - s);
        let inner = match side {
            DeskbarSide::Right => {
                look::shadow_left(cx.canvas, t, face);
                face.x0
            }
            // Hanging from the left edge, it casts the shadow every other
            // floating thing does: right and down.
            DeskbarSide::Left => {
                look::shadow(cx.canvas, t, face);
                face.x1 - 2.0
            }
        };
        aizome_kit::draw::frost(cx.canvas, t, face, t.chrome());
        let (lo, hi) = match side {
            DeskbarSide::Right => (inner, inner + 1.0),
            DeskbarSide::Left => (inner + 1.0, inner),
        };
        cx.canvas.fill_rect(Rect::new(lo, 0.0, lo + 1.0, face.y1), t.bevel_lo());
        cx.canvas.fill_rect(Rect::new(hi, 0.0, hi + 1.0, face.y1 - 1.0), t.bevel_hi());
        cx.canvas.fill_rect(Rect::new(face.x0, face.y1 - 1.0, face.x1, face.y1), t.bevel_lo());
    });
    // The edge toward the screen, under the grip.
    let edge_x = match side {
        DeskbarSide::Right => face_x,
        DeskbarSide::Left => face_x + width - EDGE,
    };
    let mut layers = vec![
        ground.into(),
        aizome_kit::container(content).padding([0.0, 0.0, 0.0, face_x]).into(),
        aizome_kit::container(edge((h - SHADOW - GRIP).max(1.0))).padding([GRIP, 0.0, 0.0, edge_x]).into(),
    ];
    // Let go now and it leaves: its rows fade under a veil of `chrome`,
    // the grip still lit above them.
    if m.moving == Moving::Leaving {
        let veil = drawn(width, (h - SHADOW - GRIP).max(1.0), "deskbar-leaving", |cx| {
            let t = cx.theme();
            cx.canvas.fill_rect(cx.bounds(), t.chrome().with_alpha(0.6));
        });
        layers.push(aizome_kit::container(veil).padding([GRIP, 0.0, 0.0, face_x]).into());
    }
    aizome_kit::stack(layers).width(width + SHADOW).height(h).into()
}

/// Where the Deskbar takes the pointer: not its shadow.
pub fn input_region(m: &DeskbarModel) -> Vec<Rect> {
    let x = m.face_x();
    vec![Rect::new(x, 0.0, x + m.width, m.height() - SHADOW)]
}

pub fn size(m: &DeskbarModel) -> Size {
    Size::new(m.width + SHADOW, m.height())
}

/// What an edge drag would make the Deskbar, `m` at the width it would
/// take: its face outlined in the focus colour, against the same edge, on
/// a surface of its own over it that takes no input. The Deskbar itself
/// keeps its size until the drag ends, so the windows beside it are fitted
/// once, not at every step.
pub fn preview<Msg: 'static>(m: &DeskbarModel) -> Element<Msg> {
    let (width, side) = (m.width, m.side);
    let height = (m.height() - SHADOW).max(1.0);
    drawn(0.0, 0.0, ("deskbar-preview", width.to_bits(), height.to_bits(), side), move |cx| {
        let t = cx.theme();
        let b = cx.bounds();
        let x0 = match side {
            DeskbarSide::Right => b.x1 - width,
            DeskbarSide::Left => 0.0,
        };
        let face = Rect::new(x0, 0.0, x0 + width, height.min(b.y1));
        cx.canvas.fill_rect(face, t.focus().with_alpha(0.16));
        draw::border(cx.canvas, face, t.focus(), 2.0);
    })
    .into()
}

/// Where a drag of the grip would put the Deskbar: its face against the
/// other edge (`side`), on a surface of its own of the face's size that
/// takes no input. While the pointer's over the Deskbar's own half it's a
/// dashed outline whose badge says to drag there; over this half, where
/// letting go moves it, the outline is solid in the focus colour, as the
/// edge drag's is, and the badge says to let go. The badge sits near the
/// top, by the grip's path, with an arrow toward the edge and as much of
/// its line as fits (none at the Rail's width).
pub fn landing<Msg: 'static>(side: DeskbarSide, armed: bool) -> Element<Msg> {
    drawn(0.0, 0.0, ("deskbar-landing", side, armed), move |cx| {
        let t = cx.theme();
        let b = cx.bounds();
        let focus = t.focus();
        if armed {
            cx.canvas.fill_rect(b, focus.with_alpha(0.16));
            draw::border(cx.canvas, b, focus, 2.0);
        } else {
            cx.canvas.fill_rect(b, focus.with_alpha(0.06));
            let dashes = aizome_kit::sidestep_ui::StrokeStyle::new(2.0).with_dash(vec![6.0, 4.0], 0.0);
            cx.canvas.stroke(&b.inset(-1.0), &dashes, focus.with_alpha(0.8));
        }
        grip_dots(cx.canvas, b, focus);

        let (pad, icon, gap) = (10.0, 14.0, 6.0);
        let ink = if armed { t.on_focus() } else { t.text() };
        let s = ui(t, typeface::BODY, typeface::WEIGHT_TAB, ink);
        let lines = if armed { ["Release to move here", "Release"] } else { ["Drag here to move", "Drag here"] };
        let room = b.width() - 2.0 * pad - (2.0 * pad + icon + gap);
        let label = lines.iter().map(|l| cx.layout_text(l, &s, None)).find(|l| l.size().width <= room);
        let w = 2.0 * pad + icon + label.as_ref().map_or(0.0, |l| gap + l.size().width.ceil());
        let x0 = (b.x0 + (b.width() - w) / 2.0).round();
        let badge = Rect::new(x0, GRIP + 14.0, x0 + w, GRIP + 14.0 + 28.0);
        if armed {
            look::shadow(cx.canvas, t, badge);
            cx.canvas.fill_rect(badge, focus);
        } else {
            look::panel(cx.canvas, t, badge);
        }
        let arrow = match side {
            DeskbarSide::Left => Icon::ArrowLeft,
            DeskbarSide::Right => Icon::ArrowRight,
        };
        let top = badge.y0 + ((badge.height() - icon) / 2.0).round();
        arrow.draw(cx.canvas, Point::new(badge.x0 + pad, top), icon, if armed { ink } else { focus });
        if let Some(l) = label {
            let y = badge.y0 + ((badge.height() - l.size().height) / 2.0).round();
            cx.canvas.draw_text(&l, Point::new(badge.x0 + pad + icon + gap, y));
        }
    })
    .into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_height_counts_every_row() {
        let empty = DeskbarModel::default();
        let base = empty.height();
        let one = DeskbarModel {
            apps: vec![AppRow {
                app_id: "a".into(),
                name: "A".into(),
                glyph: Icon::Folder,
                hue: Hue::Blue,
                image: None,
                windows: vec![WindowRow {
                    id: 1,
                    title: "x".into(),
                    focused: false,
                    note: WindowNote::None,
                    urgent: false,
                }],
                folded: false,
                focused: false,
                unread: 0,
                cards: vec![],
            }],
            shelf: vec![],
            hide_shelf: false,
            ..DeskbarModel::default()
        };
        assert_eq!(one.height(), base + APP_ROW + WINDOW_ROW);
        let mut folded = one.clone();
        folded.apps[0].folded = true;
        assert_eq!(folded.height(), base + APP_ROW);
        // Without the Shelf, its caption and drop row go too.
        let bare = DeskbarModel { hide_shelf: true, ..one.clone() };
        assert!(bare.height() < one.height() - SHELF_ROW - CAPTION);
    }
}

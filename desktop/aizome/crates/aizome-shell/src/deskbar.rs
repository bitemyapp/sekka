//! The Deskbar (C2-Desktop's aside): at the top right under the bar, each
//! running app with its windows indented beneath, unread notifications
//! filed under their app, and the Shelf of parked drags.

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

/// The Deskbar's width, and its shadow's (to its left and under it).
pub const WIDTH: f64 = aizome_theme::metrics::DESKBAR_WIDTH;
pub const SHADOW: f64 = aizome_theme::metrics::SHADOW;
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

#[derive(Clone, Debug, Default, PartialEq)]
pub struct DeskbarModel {
    pub apps: Vec<AppRow>,
    pub shelf: Vec<ShelfRow>,
    /// `[deskbar] show_shelf = false`: no Shelf under the apps.
    pub hide_shelf: bool,
    /// Room below the menu bar on the owning output, including the shadow.
    pub max_height: Option<f64>,
}

impl DeskbarModel {
    /// Its height, shadow included: the surface's.
    pub fn height(&self) -> f64 {
        self.max_height.map_or(self.content_height(), |h| self.content_height().min(h.max(SHADOW + 1.0)))
    }

    /// Unclipped height: overflowing content stays reachable by scrolling.
    pub fn content_height(&self) -> f64 {
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

fn app_row(a: &AppRow, top: f64) -> Element<DeskbarMsg> {
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
        WIDTH,
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

fn window_row(w: &WindowRow) -> Element<DeskbarMsg> {
    let (title, focused, note, urgent) = (w.title.clone(), w.focused, w.note, w.urgent);
    let label = title.clone();
    drawn(WIDTH, WINDOW_ROW, (title.clone(), focused, note, urgent), move |cx| {
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
fn card(c: &NoteCard) -> Element<DeskbarMsg> {
    let (summary, body) = (c.summary.clone(), c.body.clone());
    let label = format!("{} {}", c.summary, c.body);
    let card = drawn(WIDTH - 42.0, CARD, (summary.clone(), body.clone()), move |cx| {
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
        aizome_kit::container(close).padding([2.0, 2.0, 0.0, WIDTH - 42.0 - 20.0]).into(),
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

fn caption() -> Element<DeskbarMsg> {
    drawn(WIDTH, 1.0 + CAPTION, "shelf-caption", |cx| {
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

fn shelf_row(i: usize, r: &ShelfRow) -> Element<DeskbarMsg> {
    let (label, glyph, hue, dim) = (r.label.clone(), r.glyph, r.hue, r.dim);
    let (uri, drag_text) = (r.uri.clone(), r.drag_text.clone());
    let text = label.clone();
    drawn(WIDTH - 20.0, SHELF_ROW, (label.clone(), glyph, hue, dim), move |cx| {
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

fn drop_row() -> Element<DeskbarMsg> {
    drawn(WIDTH - 20.0, SHELF_ROW, "drop", |cx| {
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

/// The Deskbar.
pub fn view(m: &DeskbarModel) -> Element<DeskbarMsg> {
    let mut rows: Vec<Element<DeskbarMsg>> = vec![aizome_kit::space(4.0)];
    let mut top = 4.0;
    for a in &m.apps {
        rows.push(
            aizome_kit::stack(vec![
                app_row(a, top),
                aizome_kit::container(count_hit(a)).padding([0.0, 0.0, 0.0, WIDTH - 56.0]).into(),
            ])
            .into(),
        );
        if !a.folded {
            for w in &a.windows {
                rows.push(window_row(w));
            }
        }
        for c in &a.cards {
            rows.push(card(c));
        }
        top += a.height();
    }
    if m.hide_shelf {
        rows.push(aizome_kit::space(4.0));
    } else {
        rows.push(aizome_kit::space(2.0));
        rows.push(caption());
        let mut shelf: Vec<Element<DeskbarMsg>> = m.shelf.iter().enumerate().map(|(i, r)| shelf_row(i, r)).collect();
        shelf.push(drop_row());
        rows.push(aizome_kit::container(column(shelf).spacing(SHELF_GAP)).padding([0.0, 10.0, 10.0, 10.0]).into());
    }
    let h = m.height();
    let content = aizome_kit::scroll(column(rows).width(WIDTH)).width(WIDTH).height((h - SHADOW).max(1.0));
    // The ground: `chrome`, its left and bottom edges, the shadow left and
    // down.
    let ground = drawn(WIDTH + SHADOW, h, ("deskbar-ground", h.to_bits()), |cx| {
        let t = cx.theme();
        let b = cx.bounds();
        let s = SHADOW;
        let face = Rect::new(s, 0.0, b.x1, b.y1 - s);
        look::shadow_left(cx.canvas, t, face);
        aizome_kit::draw::frost(cx.canvas, t, face, t.chrome());
        cx.canvas.fill_rect(Rect::new(face.x0, 0.0, face.x0 + 1.0, face.y1), t.bevel_lo());
        cx.canvas.fill_rect(Rect::new(face.x0 + 1.0, 0.0, face.x0 + 2.0, face.y1 - 1.0), t.bevel_hi());
        cx.canvas.fill_rect(Rect::new(face.x0, face.y1 - 1.0, face.x1, face.y1), t.bevel_lo());
    });
    aizome_kit::stack(vec![ground.into(), aizome_kit::container(content).padding([0.0, 0.0, 0.0, SHADOW]).into()])
        .width(WIDTH + SHADOW)
        .height(h)
        .into()
}

/// Where the Deskbar takes the pointer: not its shadow.
pub fn input_region(m: &DeskbarModel) -> Vec<Rect> {
    vec![Rect::new(SHADOW, 0.0, SHADOW + WIDTH, m.height() - SHADOW)]
}

pub fn size(m: &DeskbarModel) -> Size {
    Size::new(WIDTH + SHADOW, m.height())
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
            max_height: None,
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

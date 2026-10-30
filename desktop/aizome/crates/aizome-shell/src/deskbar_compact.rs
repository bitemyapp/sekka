//! The Deskbar narrowed, as its width asks: Slim (everything, a size
//! smaller: names, window titles, a line per notification) and the Rail
//! (icons alone: each app, a mark per window, its unread count).

use aizome_kit::draw;
use aizome_kit::element::Element;
use aizome_kit::layout::{Align, Point, Rect};
use aizome_kit::sidestep_ui::StrokeStyle;
use aizome_kit::sidestep_ui::kurbo::{Circle, RoundedRect};
use aizome_kit::widget::SemanticRole;
use aizome_kit::{Icon, Layoutable, column};
use aizome_theme::typeface;

use crate::deskbar::{AppRow, DeskbarModel, DeskbarMsg, ShelfRow, WindowRow};
use crate::look::{self, Hue};
use crate::widgets::drawn::{Press, drawn};
use crate::wm::WindowNote;

const SLIM_APP: f64 = 24.0;
const SLIM_WINDOW: f64 = 19.0;
const SLIM_NOTE: f64 = 20.0;
const SLIM_SHELF: f64 = 22.0;
const RAIL_APP: f64 = 34.0;
const RAIL_WINDOW: f64 = 16.0;
const RAIL_TILE: f64 = 30.0;
const GAP: f64 = 4.0;
const CAPTION: f64 = 9.0;

fn ui(t: &aizome_kit::Theme, size: f64, weight: f32, c: aizome_kit::Color) -> aizome_kit::text::TextSpec {
    look::ui(size * t.metrics.factor, weight, c)
}

/// The app's picture: its own icon, else its glyph, `size` square at `at`.
fn app_icon(cx: &mut aizome_kit::widget::PaintCx, a: &(Icon, Hue, Option<look::Pic>), at: Point, size: f64) {
    let t = cx.theme();
    match &a.2 {
        Some(img) => cx.canvas.draw_image(&img.0, Rect::new(at.x, at.y, at.x + size, at.y + size)),
        None => a.0.draw(cx.canvas, at, size, a.1.color(t)),
    }
}

/// A red count, centred on `c`.
fn badge(cx: &mut aizome_kit::widget::PaintCx, c: Point, n: usize) {
    let t = cx.theme();
    let label = if n > 9 { "9+".to_owned() } else { n.to_string() };
    let r = if label.len() > 1 { 7.5 } else { 6.5 };
    cx.canvas.fill(&RoundedRect::new(c.x - r - 1.0, c.y - 6.5, c.x + r + 1.0, c.y + 6.5, 6.5), Hue::Red.color(t));
    let spec = look::digits(9.5 * t.metrics.factor, typeface::WEIGHT_SEMIBOLD, t.on_selection());
    cx.draw_text_in(&label, &spec, Rect::new(c.x - 10.0, c.y - 7.0, c.x + 10.0, c.y + 7.0), Align::Center);
}

/// Its height, shadow and grip excluded.
pub fn height(m: &DeskbarModel, rail: bool) -> f64 {
    let mut h = GAP;
    for a in &m.apps {
        if rail {
            h += RAIL_APP;
            if !a.folded {
                h += a.windows.len() as f64 * RAIL_WINDOW + if a.windows.is_empty() { 0.0 } else { 4.0 };
            }
        } else {
            h += SLIM_APP;
            if !a.folded {
                h += a.windows.len() as f64 * SLIM_WINDOW;
            }
            h += a.cards.len() as f64 * SLIM_NOTE;
        }
    }
    if m.hide_shelf {
        return h + GAP;
    }
    h += 2.0 + CAPTION;
    let (row, gap) = if rail { (RAIL_TILE, GAP) } else { (SLIM_SHELF, 3.0) };
    // The items and "Drop to park", a gap between each.
    let n = m.shelf.len() as f64;
    h + (n + 1.0) * row + n * gap + 6.0
}

/// The rows, top to bottom.
pub fn rows(m: &DeskbarModel, rail: bool) -> Vec<Element<DeskbarMsg>> {
    let w = m.width;
    let mut rows: Vec<Element<DeskbarMsg>> = vec![aizome_kit::space(GAP)];
    for a in &m.apps {
        if rail {
            rows.push(rail_app(a, w));
            if !a.folded && !a.windows.is_empty() {
                for win in &a.windows {
                    rows.push(rail_window(win, w));
                }
                rows.push(aizome_kit::space(4.0));
            }
        } else {
            rows.push(slim_app(a, w));
            if !a.folded {
                for win in &a.windows {
                    rows.push(slim_window(win, w));
                }
            }
            for c in &a.cards {
                rows.push(slim_note(c.id, &c.summary, w));
            }
        }
    }
    if m.hide_shelf {
        rows.push(aizome_kit::space(GAP));
        return rows;
    }
    rows.push(divider(w));
    let (row, gap) = if rail { (RAIL_TILE, GAP) } else { (SLIM_SHELF, 3.0) };
    let mut shelf: Vec<Element<DeskbarMsg>> =
        m.shelf.iter().enumerate().map(|(i, r)| if rail { rail_tile(i, r, w) } else { slim_shelf(i, r, w) }).collect();
    shelf.push(drop_tile(w, row, rail));
    rows.push(aizome_kit::container(column(shelf).spacing(gap)).padding([0.0, 0.0, 6.0, 0.0]).into());
    rows
}

fn rail_app(a: &AppRow, w: f64) -> Element<DeskbarMsg> {
    let look = (a.glyph, a.hue, a.image.clone());
    let (unread, n, folded, focused) = (a.unread, a.windows.len(), a.folded, a.focused);
    let (id, id2, id3) = (a.app_id.clone(), a.app_id.clone(), a.app_id.clone());
    let newest = a.cards.first().map(|c| c.id);
    let name = a.name.clone();
    let tip = if unread > 0 { format!("{} — {unread} new", a.name) } else { a.name.clone() };
    let el = drawn(w, RAIL_APP, (a.name.clone(), a.glyph, a.hue, unread, n, folded, focused), move |cx| {
        let t = cx.theme();
        let b = cx.bounds();
        if cx.is_hovered() {
            cx.canvas.fill_rect(b, t.hover(t.chrome()));
        }
        let at = Point::new(((b.width() - 20.0) / 2.0).round(), 7.0);
        app_icon(cx, &look, at, 20.0);
        if focused {
            cx.canvas.fill_rect(Rect::new(b.x0 + 3.0, 9.0, b.x0 + 5.0, 25.0), t.focus());
        }
        if unread > 0 {
            badge(cx, Point::new(at.x + 21.0, at.y + 1.0), unread);
        } else if folded && n > 0 {
            // Its windows folded: how many, under the icon.
            let s = look::digits(9.0 * t.metrics.factor, typeface::WEIGHT_REGULAR, t.text3());
            cx.draw_text_in(&format!("{n}▸"), &s, Rect::new(0.0, 26.0, b.x1, RAIL_APP), Align::Center);
        }
    });
    let el = match newest {
        Some(note) => el.on_click(DeskbarMsg::OpenNote(note)),
        None => el.on_click(DeskbarMsg::FocusApp(id)),
    };
    let el = el
        .on_right_click(move |p: Press| DeskbarMsg::AppMenu(id2.clone(), p.at))
        .on_middle_click(DeskbarMsg::Fold(id3));
    let el = el.text(name, SemanticRole::ListItem).key(("app", a.app_id.as_str()));
    aizome_kit::tooltip(el, tip)
}

/// A window as a mark under its app: a small frame, filled in the focus
/// colour when it's the focused one, dashed when hidden.
fn rail_window(win: &WindowRow, w: f64) -> Element<DeskbarMsg> {
    let (focused, note, urgent) = (win.focused, win.note, win.urgent);
    let title = win.title.clone();
    let el = drawn(w, RAIL_WINDOW, (win.id, focused, note, urgent), move |cx| {
        let t = cx.theme();
        let b = cx.bounds();
        if cx.is_hovered() {
            cx.canvas.fill_rect(b, t.hover(t.chrome()));
        }
        let x0 = ((b.width() - 20.0) / 2.0).round();
        let r = Rect::new(x0 + 0.5, 3.5, x0 + 19.5, 12.5);
        let shape = RoundedRect::new(r.x0, r.y0, r.x1, r.y1, 2.0);
        if focused {
            cx.canvas.fill(&shape, t.focus());
        } else if note == WindowNote::Hidden {
            let s = StrokeStyle::new(1.0).with_dash(vec![2.0, 1.5], 0.0);
            cx.canvas.stroke(&shape, &s, t.text3());
        } else {
            cx.canvas.fill(&shape, t.sunk());
            cx.canvas.stroke(&shape, &StrokeStyle::new(1.0), t.text3());
        }
        // The title bar's line, as the window's tab.
        let c = if focused { t.on_selection() } else { t.text3() };
        cx.canvas.fill_rect(Rect::new(r.x0 + 3.0, r.y0 + 2.0, r.x0 + 9.0, r.y0 + 3.0), c);
        if urgent {
            cx.canvas.fill(&Circle::new(Point::new(r.x1 + 1.0, r.y0), 2.5), Hue::Red.color(t));
        }
    })
    .on_click(DeskbarMsg::FocusWindow(win.id))
    .on_middle_click(DeskbarMsg::CloseWindow(win.id))
    .on_drag({
        let id = win.id;
        move |p| DeskbarMsg::WindowDrag(id, p)
    })
    .text(title.clone(), SemanticRole::ListItem)
    .key(("window", win.id));
    aizome_kit::tooltip(el, title)
}

fn slim_app(a: &AppRow, w: f64) -> Element<DeskbarMsg> {
    let look = (a.glyph, a.hue, a.image.clone());
    let (name, n, folded, unread) = (a.name.clone(), a.windows.len(), a.folded, a.unread);
    let (id, id2, id3) = (a.app_id.clone(), a.app_id.clone(), a.app_id.clone());
    let label = name.clone();
    drawn(w, SLIM_APP, (name.clone(), a.glyph, a.hue, n, folded, unread), move |cx| {
        let t = cx.theme();
        let b = cx.bounds();
        if cx.is_hovered() {
            cx.canvas.fill_rect(b, t.hover(t.chrome()));
        }
        app_icon(cx, &look, Point::new(7.0, 5.0), 14.0);
        let s = ui(t, 11.5, typeface::WEIGHT_TAB, t.text());
        cx.draw_text_in(&name, &s, Rect::new(27.0, 0.0, b.x1 - 30.0, SLIM_APP), Align::Start);
        if unread > 0 {
            badge(cx, Point::new(b.x1 - 16.0, SLIM_APP / 2.0), unread);
        } else if n > 0 {
            let right = if folded { format!("{n} ▸") } else { n.to_string() };
            let s = look::digits(10.0 * t.metrics.factor, typeface::WEIGHT_REGULAR, t.text3());
            cx.draw_text_in(&right, &s, Rect::new(b.x1 - 40.0, 0.0, b.x1 - 8.0, SLIM_APP), Align::End);
        }
    })
    .on_click(DeskbarMsg::FocusApp(id))
    .on_right_click(move |p: Press| DeskbarMsg::AppMenu(id2.clone(), p.at))
    .on_middle_click(DeskbarMsg::Fold(id3))
    .text(label, SemanticRole::ListItem)
    .key(("app", a.app_id.as_str()))
    .into()
}

fn slim_window(win: &WindowRow, w: f64) -> Element<DeskbarMsg> {
    let (title, focused, note, urgent) = (win.title.clone(), win.focused, win.note, win.urgent);
    let label = title.clone();
    drawn(w, SLIM_WINDOW, (title.clone(), focused, note, urgent), move |cx| {
        let t = cx.theme();
        let b = cx.bounds();
        if focused {
            cx.canvas.fill_rect(b, look::mix(t.chrome(), t.focus(), 0.16));
            cx.canvas.fill_rect(Rect::new(14.0, 4.0, 16.0, 15.0), t.focus());
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
        let mut s = ui(t, 11.0, typeface::WEIGHT_REGULAR, color);
        s.font.italic = hidden;
        cx.draw_text_in(&title, &s, Rect::new(27.0, 0.0, b.x1 - 8.0, SLIM_WINDOW), Align::Start);
        if urgent {
            cx.canvas.fill(&Circle::new(Point::new(21.0, SLIM_WINDOW / 2.0), 2.5), Hue::Red.color(t));
        }
    })
    .on_click(DeskbarMsg::FocusWindow(win.id))
    .on_middle_click(DeskbarMsg::CloseWindow(win.id))
    .on_drag({
        let id = win.id;
        move |p| DeskbarMsg::WindowDrag(id, p)
    })
    .text(label, SemanticRole::ListItem)
    .key(("window", win.id))
    .into()
}

/// A notification in one line: a red mark and its summary.
fn slim_note(id: u32, summary: &str, w: f64) -> Element<DeskbarMsg> {
    let summary = summary.to_owned();
    let label = summary.clone();
    drawn(w, SLIM_NOTE, summary.clone(), move |cx| {
        let t = cx.theme();
        let b = cx.bounds();
        let face = Rect::new(22.0, 2.0, b.x1 - 6.0, SLIM_NOTE - 2.0);
        cx.canvas.fill_rect(face, if cx.is_hovered() { t.hover(t.sunk()) } else { t.sunk() });
        cx.canvas.fill_rect(Rect::new(face.x0, face.y0, face.x0 + 2.0, face.y1), Hue::Red.color(t));
        let s = ui(t, 10.5, typeface::WEIGHT_TAB, t.text());
        cx.draw_text_in(&summary, &s, Rect::new(face.x0 + 7.0, 0.0, face.x1 - 4.0, SLIM_NOTE), Align::Start);
    })
    .on_click(DeskbarMsg::OpenNote(id))
    .on_middle_click(DeskbarMsg::DismissNote(id))
    .text(label, SemanticRole::Group)
    .key(("note", id))
    .into()
}

fn divider(w: f64) -> Element<DeskbarMsg> {
    drawn(w, 2.0 + CAPTION, "compact-divider", |cx| {
        let t = cx.theme();
        let b = cx.bounds();
        let y = (CAPTION / 2.0).round();
        cx.canvas.fill_rect(Rect::new(6.0, y, b.x1 - 6.0, y + 1.0), t.bevel_lo());
        cx.canvas.fill_rect(Rect::new(6.0, y + 1.0, b.x1 - 6.0, y + 2.0), t.bevel_hi());
    })
    .text("Shelf", SemanticRole::Label)
    .into()
}

fn rail_tile(i: usize, r: &ShelfRow, w: f64) -> Element<DeskbarMsg> {
    let (glyph, hue, label) = (r.glyph, r.hue, r.label.clone());
    let (uri, drag_text) = (r.uri.clone(), r.drag_text.clone());
    let x0 = ((w - RAIL_TILE) / 2.0).round();
    let tile = drawn(RAIL_TILE, RAIL_TILE, (label.clone(), glyph, hue), move |cx| {
        let t = cx.theme();
        let b = cx.bounds();
        cx.canvas.fill_rect(b, if cx.is_hovered() { t.hover(t.surface()) } else { t.surface() });
        draw::border(cx.canvas, b, t.line(), 1.0);
        glyph.draw(cx.canvas, Point::new(8.0, 8.0), 14.0, hue.color(t));
    })
    .on_double_click(DeskbarMsg::ShelfOpen(i))
    .drag_out(move || crate::shelf::drag_source(uri.as_deref(), &drag_text))
    .on_right_click(move |p: Press| DeskbarMsg::ShelfMenu(i, p.at))
    .text(label.clone(), SemanticRole::ListItem)
    .key(("shelf", i));
    let tile = aizome_kit::tooltip(tile, label);
    aizome_kit::container(tile).padding([0.0, 0.0, 0.0, x0]).into()
}

fn slim_shelf(i: usize, r: &ShelfRow, w: f64) -> Element<DeskbarMsg> {
    let (label, glyph, hue, dim) = (r.label.clone(), r.glyph, r.hue, r.dim);
    let (uri, drag_text) = (r.uri.clone(), r.drag_text.clone());
    let text = label.clone();
    let row = drawn(w - 12.0, SLIM_SHELF, (label.clone(), glyph, hue, dim), move |cx| {
        let t = cx.theme();
        let b = cx.bounds();
        cx.canvas.fill_rect(b, if cx.is_hovered() { t.hover(t.surface()) } else { t.surface() });
        draw::border(cx.canvas, b, t.line(), 1.0);
        glyph.draw(cx.canvas, Point::new(6.0, ((SLIM_SHELF - 12.0) / 2.0).round()), 12.0, hue.color(t));
        let s = ui(t, 10.5, typeface::WEIGHT_REGULAR, if dim { t.text2() } else { t.text() });
        cx.draw_text_in(&label, &s, Rect::new(23.0, 0.0, b.x1 - 6.0, SLIM_SHELF), Align::Start);
    })
    .on_double_click(DeskbarMsg::ShelfOpen(i))
    .drag_out(move || crate::shelf::drag_source(uri.as_deref(), &drag_text))
    .on_right_click(move |p: Press| DeskbarMsg::ShelfMenu(i, p.at))
    .text(text, SemanticRole::ListItem)
    .key(("shelf", i));
    aizome_kit::container(row).padding([0.0, 6.0, 0.0, 6.0]).into()
}

fn drop_tile(w: f64, h: f64, rail: bool) -> Element<DeskbarMsg> {
    let width = if rail { h } else { w - 12.0 };
    let x0 = if rail { ((w - h) / 2.0).round() } else { 6.0 };
    let tile = drawn(width, h, ("drop", rail), move |cx| {
        let t = cx.theme();
        let b = cx.bounds();
        let style = StrokeStyle::new(1.0).with_dash(vec![3.0, 2.0], 0.0);
        cx.canvas.stroke(&b.inset(-0.5), &style, t.line2());
        if rail {
            Icon::Plus.draw(
                cx.canvas,
                Point::new(((h - 12.0) / 2.0).round(), ((h - 12.0) / 2.0).round()),
                12.0,
                t.text3(),
            );
        } else {
            let s = ui(t, 10.5, typeface::WEIGHT_REGULAR, t.text3());
            cx.draw_text_in("Drop to park", &s, b, Align::Center);
        }
    })
    .on_drop(DeskbarMsg::Park)
    .text("Drop to park", SemanticRole::Group)
    .key("drop");
    aizome_kit::container(tile).padding([0.0, 0.0, 0.0, x0]).into()
}

//! The browser's strip of columns, NeXT's way: each folder opened makes
//! a column to the right, and the strip scrolls sideways to keep the
//! newest in sight. Dividers between columns drag to resize them all.

use aizome_kit::element::{Element, MsgFn};
use aizome_kit::hash::PaintHasher;
use aizome_kit::sidestep_ui::{Cursor, PointerButton};
use aizome_kit::widget::{Event, EventCx, LayoutCx, PaintCx, Status, Widget};
use aizome_kit::{Constraints, Layoutable, Length, Point, Rect, Size};
use std::rc::Rc;
use std::time::{Duration, Instant};

/// A strip of `columns`, each `width` wide (the last as wide as given by
/// `last_width`). It shows column `reveal.0` whole (with the ones after
/// it) each time `reveal.1` changes.
pub fn strip<Msg: Clone + Send + 'static>(
    columns: Vec<Element<Msg>>,
    width: f64,
    last_width: f64,
    reveal: (usize, u64),
    on_resize: Rc<dyn Fn(f64) -> Msg>,
) -> Element<Msg> {
    let n = columns.len();
    let mut el = Element::new(Strip { width, last_width, n, reveal, on_resize, content: 0.0, viewport: 0.0 }, columns);
    el = el.width(Length::Fill).height(Length::Fill);
    el
}

struct Strip<Msg> {
    width: f64,
    last_width: f64,
    n: usize,
    reveal: (usize, u64),
    on_resize: Rc<dyn Fn(f64) -> Msg>,
    content: f64,
    viewport: f64,
}

#[derive(Default)]
struct StripState {
    offset: f64,
    reveal_gen: u64,
    follow: bool,
    /// A divider being dragged: how many columns are left of it.
    resizing: Option<f64>,
    near: bool,
    bar_until: Option<Instant>,
}

const LINGER: Duration = Duration::from_millis(900);
/// The height of the dividers' grips, at the foot of the strip.
pub const GRIP: f64 = 18.0;

impl<Msg: Clone + Send + 'static> Strip<Msg> {
    fn x_of(&self, i: usize) -> f64 {
        i as f64 * self.width
    }

    /// Whether `x` (content points) at `y` is on a divider's grip, at
    /// its foot.
    fn divider_at(&self, x: f64, y: f64, height: f64) -> bool {
        y >= height - GRIP && (1..=self.n.saturating_sub(1)).any(|i| (self.x_of(i) - x).abs() <= 5.0)
    }
}

impl<Msg: Clone + Send + 'static> Widget for Strip<Msg> {
    fn name(&self) -> &'static str {
        "hako-strip"
    }

    fn layout(&mut self, cx: &mut LayoutCx, bc: Constraints) -> Size {
        let size = bc.constrain(Size::new(
            if bc.max.width.is_finite() { bc.max.width } else { 800.0 },
            if bc.max.height.is_finite() { bc.max.height } else { 400.0 },
        ));
        let n = cx.child_count();
        self.n = n;
        let last = if n > 0 { self.last_width.max(self.width) } else { 0.0 };
        let content = self.width * n.saturating_sub(1) as f64 + last;
        self.content = content;
        self.viewport = size.width;
        let (reveal, generation) = self.reveal;
        let st = cx.state::<StripState>();
        if generation != st.reveal_gen {
            st.reveal_gen = generation;
            st.follow = true;
        }
        // Until the person scrolls it themselves, the strip follows the
        // columns as they come (a folder's listing, a preview).
        if st.follow {
            // Show the column asked for and everything after it, or as
            // much of it as fits from its left edge.
            // Whole columns only: the strip starts at a column's edge, and
            // the last column takes what's left over.
            let from = reveal.min(n.saturating_sub(1)) as f64 * self.width;
            let end = content;
            let need = if end > size.width { ((end - size.width) / self.width).ceil() * self.width } else { 0.0 };
            let max_whole = n.saturating_sub(1) as f64 * self.width;
            st.offset = need.min(from).min(max_whole);
        }
        // Keep the last column's right edge at most at the right edge
        // (columns going away pull the strip back).
        if !st.follow {
            st.offset = st.offset.clamp(0.0, (content - size.width).max(0.0)).round();
        }
        let off = st.offset;
        for i in 0..n {
            let w = if i + 1 == n { last.max(size.width - self.x_of(i) + off) } else { self.width };
            cx.layout_child(i, Constraints::tight(Size::new(w, size.height)));
            cx.place_child(i, Point::new(self.x_of(i) - off, 0.0));
        }
        size
    }

    fn paint(&self, cx: &mut PaintCx) {
        let t = cx.theme();
        cx.canvas.fill_rect(cx.bounds(), t.surface());
    }

    fn paint_over(&self, cx: &mut PaintCx) {
        let t = cx.theme();
        let b = cx.bounds();
        let st = cx.state::<StripState>();
        let off = st.map_or(0.0, |s| s.offset);
        let near = st.is_some_and(|s| s.near || s.resizing.is_some());
        for i in 1..self.n {
            let x = self.x_of(i) - off;
            if x <= 0.0 || x >= b.x1 {
                continue;
            }
            cx.canvas.fill_rect(Rect::new(x - 1.0, 0.0, x, b.y1), if near { t.focus() } else { t.line() });
            // The grip at its foot: two short rules either side.
            let g = if near { t.focus() } else { t.text3() };
            for dx in [-4.0, 3.0] {
                cx.canvas.fill_rect(Rect::new(x + dx, b.y1 - GRIP + 5.0, x + dx + 1.0, b.y1 - 5.0), g);
            }
        }
        // A thin bar while it moves.
        if self.content > self.viewport + 0.5 && st.is_some_and(|s| s.bar_until.is_some_and(|u| cx.now() < u)) {
            let track = Rect::new(2.0, b.y1 - 6.0, b.x1 - 2.0, b.y1 - 2.0);
            let len = (track.width() * self.viewport / self.content).max(24.0);
            let x = track.x0 + (track.width() - len) * (off / (self.content - self.viewport)).clamp(0.0, 1.0);
            cx.canvas
                .fill_rect(Rect::new(x.round(), track.y0, (x + len).round(), track.y1), t.line2().mix(t.text3(), 0.35));
        }
    }

    fn hash(&self, h: &mut PaintHasher) {
        h.add(&self.n).f64(self.width).f64(self.last_width);
    }

    fn clips(&self) -> bool {
        true
    }

    fn hit(&self) -> bool {
        true
    }

    fn hover_paints(&self) -> bool {
        false
    }

    fn event(&mut self, cx: &mut EventCx, event: &Event) -> Status {
        match event {
            Event::Scroll { delta, mods, .. } => {
                let dx = if mods.shift() && delta.x == 0.0 { delta.y } else { delta.x };
                if dx == 0.0 {
                    return Status::Ignored;
                }
                let max = (self.content - self.viewport).max(0.0);
                let now = cx.now();
                let st = cx.state::<StripState>();
                let before = st.offset;
                st.offset = (st.offset + dx).clamp(0.0, max).round();
                if st.offset == before {
                    return Status::Ignored;
                }
                st.follow = false;
                st.bar_until = Some(now + LINGER);
                cx.request_tick(LINGER + Duration::from_millis(20));
                cx.relayout();
                cx.invalidate();
                Status::Handled
            }
            Event::Tick => {
                cx.invalidate();
                Status::Ignored
            }
            Event::PointerMove { pos, .. } => {
                let off = cx.state::<StripState>().offset;
                if let Some(left) = cx.state::<StripState>().resizing {
                    // Every column takes the width that puts the divider
                    // under the pointer.
                    let w = ((pos.x + off) / left).clamp(140.0, 600.0).round();
                    if w != self.width {
                        cx.emit(&MsgFn::new((self.on_resize)(w)));
                    }
                    return Status::Handled;
                }
                let near = self.divider_at(pos.x + off, pos.y, cx.size().height);
                let st = cx.state::<StripState>();
                if near != st.near {
                    st.near = near;
                    cx.set_cursor(if near { Cursor::ColResize } else { Cursor::Default });
                    cx.invalidate();
                }
                Status::Ignored
            }
            Event::PointerLeave => {
                let st = cx.state::<StripState>();
                if st.near && st.resizing.is_none() {
                    st.near = false;
                    cx.set_cursor(Cursor::Default);
                    cx.invalidate();
                }
                Status::Ignored
            }
            Event::PointerDown { pos, button: PointerButton::Left, .. } => {
                let off = cx.state::<StripState>().offset;
                if self.divider_at(pos.x + off, pos.y, cx.size().height) {
                    // The divider dragged is at a column's right edge: its
                    // width grows with the pointer, by the share of the
                    // columns before it.
                    let left = ((pos.x + off) / self.width).round().max(1.0);
                    cx.state::<StripState>().resizing = Some(left);
                    cx.capture_pointer();
                    return Status::Handled;
                }
                Status::Ignored
            }
            Event::PointerUp { .. } => {
                if cx.state::<StripState>().resizing.take().is_some() {
                    cx.release_pointer();
                    cx.invalidate();
                    return Status::Handled;
                }
                Status::Ignored
            }
            _ => Status::Ignored,
        }
    }
}

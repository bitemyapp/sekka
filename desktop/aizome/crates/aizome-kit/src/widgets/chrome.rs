//! The kit's own frames: a window's root, the layer a sheet sits on, the
//! panel a popover floats in.

use crate::draw;
use crate::element::{Common, RawElement};
use crate::layout::{Constraints, Length, Point, Rect, Size};
use crate::widget::{Event, EventCx, LayoutCx, PaintCx, SemanticRole, Semantics, Status, Widget};
use crate::widgets::layout::stack_layout;

/// A window's root: the surface colour under everything, its layers (the
/// content, then sheets) stacked to its size.
pub(crate) struct Root {
    /// Painted with the surface colour (else see-through: a desktop
    /// surface draws its own ground).
    pub opaque: bool,
}

impl Widget for Root {
    fn name(&self) -> &'static str {
        "root"
    }

    fn layout(&mut self, cx: &mut LayoutCx, bc: Constraints) -> Size {
        stack_layout(cx, bc, crate::layout::Align::Start, crate::layout::Align::Start)
    }

    fn paint(&self, cx: &mut PaintCx) {
        if self.opaque {
            let c = cx.theme().surface();
            let r = cx.bounds();
            cx.canvas.fill_rect(r, c);
        }
    }

    fn hash(&self, h: &mut crate::hash::PaintHasher) {
        h.add(&self.opaque);
    }

    fn solid(&self) -> bool {
        true
    }

    fn semantics(&self) -> Option<Semantics> {
        Some(Semantics { role: SemanticRole::Window, ..Default::default() })
    }
}

/// The layer a sheet sits on: it takes the whole window, dims it, keeps
/// the pointer and focus inside the sheet, and hangs the sheet from the
/// window's top, centred.
pub(crate) struct SheetLayer {
    /// Where the sheet hangs from (under the in-window menu bar).
    top: f64,
}

impl SheetLayer {
    pub fn element(content: RawElement, key: u64, top: f64) -> RawElement {
        let panel_common = Common { key: Some(1), ..Common::default() };
        let panel = RawElement::new(SheetPanel, panel_common, vec![content]);
        let mut c = Common::sized(Length::Fill, Length::Fill);
        c.key = Some(key);
        RawElement::new(SheetLayer { top }, c, vec![panel])
    }
}

impl Widget for SheetLayer {
    fn name(&self) -> &'static str {
        "sheet-layer"
    }

    fn layout(&mut self, cx: &mut LayoutCx, bc: Constraints) -> Size {
        let size = bc.max;
        let max_w = (size.width - 32.0).clamp(160.0, 640.0);
        let s = cx.layout_child(0, Constraints::loose(Size::new(max_w, (size.height - self.top - 16.0).max(80.0))));
        cx.place_child(0, Point::new(((size.width - s.width) / 2.0).round(), self.top));
        size
    }

    fn paint(&self, cx: &mut PaintCx) {
        let t = cx.theme();
        let dim = if t.is_dark() { t.slab().with_alpha(0.6) } else { t.slab().with_alpha(0.5) };
        let r = cx.bounds();
        cx.canvas.fill_rect(r, dim);
    }

    fn solid(&self) -> bool {
        true
    }

    fn modal(&self) -> bool {
        true
    }

    fn over_video(&self) -> bool {
        // The sheet and its full-window dimming must cover native video
        // surfaces too, including alerts and open/save panels.
        true
    }

    fn event(&mut self, _cx: &mut EventCx, event: &Event) -> Status {
        // Nothing under the sheet hears the pointer.
        match event {
            Event::PointerDown { .. } | Event::PointerUp { .. } | Event::Scroll { .. } => Status::Handled,
            _ => Status::Ignored,
        }
    }

    fn semantics(&self) -> Option<Semantics> {
        Some(Semantics { role: SemanticRole::Dialog, ..Default::default() })
    }
}

/// A sheet: raised, edged in `bevel_lo` but along the top (it hangs from
/// the window's edge), with the hard shadow.
struct SheetPanel;

impl Widget for SheetPanel {
    fn name(&self) -> &'static str {
        "sheet"
    }

    fn layout(&mut self, cx: &mut LayoutCx, bc: Constraints) -> Size {
        let s = cx.layout_child(0, bc.loosen());
        cx.place_child(0, Point::ZERO);
        bc.constrain(Size::new(s.width.max(320.0f64.min(bc.max.width)), s.height))
    }

    fn paint(&self, cx: &mut PaintCx) {
        let t = cx.theme();
        let r = cx.bounds();
        if t.shadows() {
            let s = t.metrics.shadow;
            cx.canvas.fill_rect(Rect::new(r.x0 + s, r.y1, r.x1 + s, r.y1 + s), t.slab());
            cx.canvas.fill_rect(Rect::new(r.x1, r.y0, r.x1 + s, r.y1), t.slab());
        }
        cx.canvas.fill_rect(r, t.raised());
        draw::left(cx.canvas, r, t.bevel_lo(), 1.0);
        draw::right(cx.canvas, r, t.bevel_lo(), 1.0);
        draw::bottom(cx.canvas, r, t.bevel_lo(), 1.0);
        // The focus colour along the top, as the focused frame's rail.
        draw::top(cx.canvas, r, t.focus(), 2.0);
    }

    fn solid(&self) -> bool {
        true
    }

    fn overflow(&self) -> f64 {
        3.0
    }
}

/// A popover's frame in its popup: the floating look and room for the
/// shadow.
pub(crate) struct FloatingPanel;

impl FloatingPanel {
    pub fn element(content: RawElement) -> RawElement {
        RawElement::new(FloatingPanel, Common::default(), vec![content])
    }
}

impl Widget for FloatingPanel {
    fn name(&self) -> &'static str {
        "floating"
    }

    fn layout(&mut self, cx: &mut LayoutCx, bc: Constraints) -> Size {
        let shadow = cx.theme().metrics.shadow;
        let inner = Constraints::loose(Size::new(bc.max.width - shadow - 2.0, bc.max.height - shadow - 2.0));
        let s = cx.layout_child(0, inner);
        cx.place_child(0, Point::new(1.0, 1.0));
        Size::new(s.width + 2.0 + shadow, s.height + 2.0 + shadow)
    }

    fn paint(&self, cx: &mut PaintCx) {
        let t = cx.theme();
        let s = t.metrics.shadow;
        let b = cx.bounds();
        let r = Rect::new(b.x0, b.y0, b.x1 - s, b.y1 - s);
        draw::floating(cx.canvas, t, r);
    }

    fn solid(&self) -> bool {
        true
    }
}

//! What shows under the pointer while files are dragged out of Hako: a
//! chip in the look of the Shelf's cards (an icon in its dye, the name or
//! how many items), drawn when the drag starts at the scale of the window
//! it leaves, so it's as sharp at 2× as the window is.
//!
//! And something to take hold of that isn't a row: a [`handle`], which
//! the preview's picture is.

use aizome_kit::element::{Element, MsgFn};
use aizome_kit::sidestep_ui::{Canvas, PointerButton, kurbo};
use aizome_kit::theme::FontSpec;
use aizome_kit::widget::{Event, EventCx, LayoutCx, Status, Widget};
use aizome_kit::{Constraints, Point, Rect, Size, Theme};

use super::{dnd, look};
use crate::model::entry::Entry;

/// The chip's height in points.
pub const HEIGHT: f64 = 26.0;

/// A chip for dragging `first` (and `count - 1` more): its size in points
/// and its drawing, for `DragSource::icon_drawn`.
pub fn chip(
    theme: &Theme,
    first: Option<&Entry>,
    count: usize,
) -> (Size, impl Fn(&mut Canvas) + Send + Sync + 'static) {
    let label = match (first, count) {
        (Some(e), 1) => e.display.clone(),
        (_, n) => format!("{n} items"),
    };
    let (icon, role) = match (first, count) {
        (Some(e), 1) => look::entry_icon(e),
        _ => (aizome_kit::Icon::Files, aizome_kit::Role::Accent),
    };
    let style = theme.style(FontSpec::ui(theme.metrics.small + 1.0), theme.text());
    let text_w = aizome_kit::sidestep_ui::measure(&label, &style).width.min(220.0);
    let width = (8.0 + 14.0 + 8.0 + text_w + 10.0).ceil();
    let size = Size::new(width, HEIGHT);
    let fill = theme.surface();
    let edge = theme.line();
    let icon_color = theme.role(role);
    let draw = move |c: &mut Canvas| {
        let r = Rect::from_origin_size(Point::ZERO, size);
        c.fill_rect(r, fill.with_alpha(0.94));
        aizome_kit::draw::border(c, r, edge, 1.0);
        icon.draw(c, Point::new(8.0, ((HEIGHT - 14.0) / 2.0).round()), 14.0, icon_color);
        let layout = aizome_kit::sidestep_ui::TextLayout::builder(&label, &style)
            .width(text_w)
            .wrap(aizome_kit::sidestep_ui::Wrap::None)
            .ellipsis(true)
            .build();
        let h = layout.size().height;
        c.draw_text(&layout, kurbo::Point::new(30.0, ((HEIGHT - h) / 2.0).round()));
    };
    (size, draw)
}

/// `child`, as something to drag out of the window by: pressed with the
/// left button and moved [`dnd::SLOP`] points, it sends `msg`, and the
/// program starts the drag (`Hako::drag_out`, as when a drag of rows
/// leaves the window). Where `child` shows a picture of `picture`'s size
/// as large as fits in its middle (the kit's `image`), only the picture
/// takes hold, not the room left beside it. It takes only drags: a
/// click, the other buttons, the wheel and drops are for what's under
/// it, as if it weren't there.
pub fn handle<Msg: Clone + Send + 'static>(child: Element<Msg>, picture: Option<Size>, msg: Msg) -> Element<Msg> {
    Element::new(Handle { picture, msg: MsgFn::new(msg) }, vec![child])
}

struct Handle {
    picture: Option<Size>,
    msg: MsgFn,
}

/// Where the button went down on a handle, until it drags or comes up.
#[derive(Default)]
struct Pressed(Option<Point>);

impl Handle {
    /// Whether `p` is on what takes hold: all of it, or its picture where
    /// the kit's `image` draws one in a room of `size`.
    fn holds(&self, p: Point, size: Size) -> bool {
        let Some(s) = self.picture.filter(|s| s.width > 0.0 && s.height > 0.0) else { return true };
        let k = (size.width / s.width).min(size.height / s.height);
        let (w, h) = (s.width * k, s.height * k);
        let (x, y) = ((size.width - w) / 2.0, (size.height - h) / 2.0);
        Rect::new(x, y, x + w, y + h).contains(p)
    }
}

impl Widget for Handle {
    fn name(&self) -> &'static str {
        "hako-drag-handle"
    }

    fn layout(&mut self, cx: &mut LayoutCx, bc: Constraints) -> Size {
        let s = cx.layout_child(0, bc);
        cx.place_child(0, Point::ZERO);
        bc.constrain(s)
    }

    fn hit(&self) -> bool {
        true
    }

    fn hover_paints(&self) -> bool {
        false
    }

    fn drags_only(&self) -> bool {
        true
    }

    fn event(&mut self, cx: &mut EventCx, event: &Event) -> Status {
        match event {
            Event::PointerDown { pos, button: PointerButton::Left, .. } => {
                // Beside the picture it's a press that can't drag: a
                // click on what's under, like any other that doesn't.
                cx.state::<Pressed>().0 = self.holds(*pos, cx.size()).then_some(*pos);
                Status::Handled
            }
            Event::PointerMove { pos, .. } => {
                // A press the kit has let go of (the compositor took the
                // pointer before the release) is over.
                if !cx.is_pressed() {
                    cx.state::<Pressed>().0 = None;
                }
                match cx.state::<Pressed>().0 {
                    Some(from) if (*pos - from).hypot() >= dnd::SLOP => {
                        // The desktop carries it on: the press is no
                        // longer the window's.
                        cx.state::<Pressed>().0 = None;
                        cx.release_pointer();
                        cx.emit(&self.msg);
                        Status::Handled
                    }
                    Some(_) => Status::Handled,
                    None => Status::Ignored,
                }
            }
            Event::PointerUp { button: PointerButton::Left, .. } => {
                // No drag came of it: the kit makes it a click on what's
                // under the handle.
                cx.state::<Pressed>().0 = None;
                Status::Ignored
            }
            _ => Status::Ignored,
        }
    }
}

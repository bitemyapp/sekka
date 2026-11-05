//! The transcript's scroll view: it keeps to the end while an answer
//! streams, but only for a reader who is at the end. Scrolled up, the
//! view stays where they left it however much arrives below; scrolled
//! back to the end, it follows again. Sending a message goes to the end
//! (a token that changes), and the position it was left at comes back
//! when the conversation is shown again ([`Reading`]).
//!
//! The kit's `scroll` has no such rule here (Omperor's kit adds
//! `follow_end`); this is that rule ([`Follow`]) in a view of its own,
//! with the kit's thin bars, wheel, keys and dragged thumb.

use std::marker::PhantomData;
use std::rc::Rc;
use std::time::{Duration, Instant};

use aizome_kit::Element;
use aizome_kit::element::MsgFn;
use aizome_kit::hash::PaintHasher;
use aizome_kit::layout::{Constraints, Point, Rect, Size, Vec2};
use aizome_kit::sidestep_ui::{Key, NamedKey, PointerButton};
use aizome_kit::widget::{
    Event, EventCx, FocusRing, Focusable, LayoutCx, PaintCx, SemanticAction, SemanticRole, Semantics, Status, Widget,
};

/// Where a reader is in a transcript.
#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Reading {
    pub offset: f64,
    pub at_end: bool,
}

impl Default for Reading {
    fn default() -> Reading {
        Reading { offset: 0.0, at_end: true }
    }
}

/// The rule, apart from any widget: the offset after a layout or a
/// scroll, and whether the view follows the end.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Follow {
    pub offset: f64,
    pub content: f64,
    pub viewport: f64,
    /// At the end: it follows what arrives.
    pub at_end: bool,
    laid: bool,
    token: Option<u64>,
}

impl Follow {
    pub fn restored(r: Reading) -> Follow {
        Follow { offset: r.offset, at_end: r.at_end, ..Follow::default() }
    }

    pub fn max(&self) -> f64 {
        (self.content - self.viewport).max(0.0)
    }

    pub fn reading(&self) -> Reading {
        Reading { offset: self.offset, at_end: self.at_end }
    }

    /// Laid out again, the content `content` tall in a view `viewport`
    /// tall: a reader at the end stays at it; one scrolled up stays put.
    /// A new `token` goes to the end.
    pub fn laid_out(&mut self, content: f64, viewport: f64, token: Option<u64>) {
        let asked = self.laid && token.is_some() && token != self.token;
        if !self.laid || asked {
            self.token = token;
        }
        self.content = content;
        self.viewport = viewport;
        if asked {
            self.at_end = true;
        }
        if self.at_end {
            self.offset = self.max();
        }
        self.offset = self.offset.clamp(0.0, self.max()).round();
        // Restored above an end that has since come nearer.
        if self.laid || asked {
            self.at_end = self.at_end || self.offset >= self.max() - 1.0;
        }
        self.laid = true;
    }

    /// The reader moved it by `dy`: whether it moved.
    pub fn scroll_by(&mut self, dy: f64) -> bool {
        let before = self.offset;
        self.offset = (self.offset + dy).clamp(0.0, self.max()).round();
        self.at_end = self.offset >= self.max() - 1.0;
        self.offset != before
    }
}

const LINGER: Duration = Duration::from_millis(900);
const BAR: f64 = 4.0;
const BAR_WIDE: f64 = 8.0;
const MIN_THUMB: f64 = 24.0;
/// Where it is, told once scrolling pauses this long.
const SETTLE: Duration = Duration::from_millis(160);
const SETTLE_TIMER: u32 = 1;

pub struct FollowScroll<Msg> {
    child: Element<Msg>,
    token: Option<u64>,
    restore: Reading,
    on_read: Option<Rc<dyn Fn(Reading) -> MsgFn>>,
    _m: PhantomData<fn() -> Msg>,
}

/// `child`, scrolled, following its end.
pub fn follow_scroll<Msg>(child: impl Into<Element<Msg>>) -> FollowScroll<Msg> {
    FollowScroll { child: child.into(), token: None, restore: Reading::default(), on_read: None, _m: PhantomData }
}

impl<Msg: Clone + Send + 'static> FollowScroll<Msg> {
    /// Go to the end whenever `token` changes.
    pub fn scroll_to_end(mut self, token: u64) -> Self {
        self.token = Some(token);
        self
    }

    /// Where to be when it first shows.
    pub fn restore(mut self, r: Reading) -> Self {
        self.restore = r;
        self
    }

    /// Told where the reader is once scrolling pauses, and at once when
    /// they reach or leave the end.
    pub fn on_read(mut self, f: impl Fn(Reading) -> Msg + 'static) -> Self {
        self.on_read = Some(Rc::new(move |r| MsgFn::new(f(r))));
        self
    }
}

impl<Msg: Clone + Send + 'static> From<FollowScroll<Msg>> for Element<Msg> {
    fn from(s: FollowScroll<Msg>) -> Element<Msg> {
        use aizome_kit::Layoutable;
        let w = FollowWidget { token: s.token, restore: s.restore, on_read: s.on_read };
        Element::new(w, vec![s.child]).width(aizome_kit::layout::Length::Fill).height(aizome_kit::layout::Length::Fill)
    }
}

struct FollowWidget {
    token: Option<u64>,
    restore: Reading,
    on_read: Option<Rc<dyn Fn(Reading) -> MsgFn>>,
}

#[derive(Default)]
struct FollowState {
    follow: Option<Follow>,
    /// The offset the content was last placed at.
    laid: Option<f64>,
    shown_until: Option<Instant>,
    /// The thumb being dragged, and where in it it was taken.
    drag: Option<f64>,
    hover_bar: bool,
    /// Whether it was at the end when it last told.
    told_end: Option<bool>,
    /// The view's width (the rule is only vertical).
    viewport_width: f64,
}

impl FollowState {
    fn f(&mut self) -> &mut Follow {
        self.follow.get_or_insert_with(Follow::default)
    }

    fn bar(&self, wide: bool) -> Option<(Rect, Rect)> {
        let f = self.follow?;
        let (c, v) = (f.content, f.viewport);
        if c <= v + 0.5 || v <= 0.0 {
            return None;
        }
        let w = if wide { BAR_WIDE } else { BAR };
        let width = self.viewport_width;
        let track = Rect::new(width - w - 2.0, 2.0, width - 2.0, v - 2.0);
        let len = (track.height() * v / c).max(MIN_THUMB).min(track.height());
        let y = track.y0 + (track.height() - len) * (f.offset / (c - v)).clamp(0.0, 1.0);
        Some((track, Rect::new(track.x0, y.round(), track.x1, (y + len).round())))
    }
}

impl FollowWidget {
    fn tell(&self, cx: &mut EventCx, now: bool) {
        let Some(f) = &self.on_read else { return };
        let st = cx.state::<FollowState>();
        let Some(follow) = st.follow else { return };
        let r = follow.reading();
        let flipped = st.told_end != Some(r.at_end);
        if now || flipped {
            st.told_end = Some(r.at_end);
            cx.emit(&f(r));
            cx.cancel_timer(SETTLE_TIMER);
        } else {
            cx.set_timer(SETTLE_TIMER, SETTLE);
        }
    }

    fn scroll_by(&self, cx: &mut EventCx, dy: f64) -> bool {
        let now = cx.now();
        let st = cx.state::<FollowState>();
        if !st.f().scroll_by(dy) {
            return false;
        }
        st.shown_until = Some(now + LINGER);
        cx.request_tick(LINGER + Duration::from_millis(20));
        cx.relayout();
        self.invalidate_bar(cx);
        self.tell(cx, false);
        true
    }

    fn invalidate_bar(&self, cx: &mut EventCx) {
        if let Some((t, _)) = cx.state::<FollowState>().bar(true) {
            cx.invalidate_rect(t.inflate(2.0, 2.0));
        }
    }

    fn drag_to(&self, cx: &mut EventCx, pos: Point) {
        let st = cx.state::<FollowState>();
        let (Some(grab), Some((track, thumb)), Some(f)) = (st.drag, st.bar(true), st.follow) else { return };
        let room = (track.height() - thumb.height()).max(1.0);
        let t = ((pos.y - grab - track.y0) / room).clamp(0.0, 1.0);
        let target = t * f.max();
        self.scroll_by(cx, target - f.offset);
    }
}

impl Widget for FollowWidget {
    fn name(&self) -> &'static str {
        "follow-scroll"
    }

    fn layout(&mut self, cx: &mut LayoutCx, bc: Constraints) -> Size {
        let unbounded = |v: f64| if v.is_finite() { v } else { 0.0 };
        let size = Size::new(unbounded(bc.max.width), unbounded(bc.max.height));
        let child_bc = Constraints { min: Size::new(size.width, 0.0), max: Size::new(size.width, f64::INFINITY) };
        let content = if cx.child_count() > 0 { cx.layout_child(0, child_bc) } else { Size::ZERO };
        let (token, restore) = (self.token, self.restore);
        let st = cx.state::<FollowState>();
        st.viewport_width = size.width;
        let f = st.follow.get_or_insert_with(|| Follow::restored(restore));
        f.laid_out(content.height, size.height, token);
        let offset = f.offset;
        let moved = st.laid.filter(|l| *l != offset).map(|l| l - offset);
        st.laid = Some(offset);
        let bars: Vec<Rect> = st.bar(true).map(|(t, _)| t.inflate(2.0, 2.0)).into_iter().collect();
        if cx.child_count() > 0 {
            cx.place_child(0, Point::new(0.0, -offset));
        }
        if let Some(d) = moved {
            cx.scrolled(Rect::from_origin_size(Point::ZERO, size), Vec2::new(0.0, d), &bars);
        }
        size
    }

    fn paint_over(&self, cx: &mut PaintCx) {
        let Some(st) = cx.state::<FollowState>() else { return };
        let visible = st.drag.is_some() || st.hover_bar || st.shown_until.is_some_and(|t| cx.now() < t);
        if !visible {
            return;
        }
        let t = cx.theme();
        let wide = st.hover_bar || st.drag.is_some();
        if let Some((track, thumb)) = st.bar(wide) {
            if wide {
                cx.canvas.fill_rect(track, t.sunk().with_alpha(0.7));
            }
            cx.canvas.fill_rect(thumb, if wide { t.text3() } else { t.line2().mix(t.text3(), 0.35) });
        }
    }

    fn hash(&self, h: &mut PaintHasher) {
        h.add(&"follow");
    }

    fn clips(&self) -> bool {
        true
    }

    fn solid(&self) -> bool {
        true
    }

    fn hover_paints(&self) -> bool {
        false
    }

    fn focusable(&self) -> Focusable {
        Focusable::Pointer
    }

    fn focus_ring(&self) -> FocusRing {
        FocusRing::Never
    }

    fn event(&mut self, cx: &mut EventCx, event: &Event) -> Status {
        let now = cx.now();
        match event {
            Event::Scroll { delta, .. } => {
                if self.scroll_by(cx, delta.y) {
                    Status::Handled
                } else {
                    Status::Ignored
                }
            }
            Event::PointerDown { pos, button: PointerButton::Left, .. } => {
                let st = cx.state::<FollowState>();
                if let Some((track, thumb)) = st.bar(true).filter(|(t, _)| t.inflate(2.0, 0.0).contains(*pos)) {
                    let _ = track;
                    st.drag = Some(if thumb.contains(*pos) { pos.y - thumb.y0 } else { thumb.height() / 2.0 });
                    cx.capture_pointer();
                    self.drag_to(cx, *pos);
                    return Status::Handled;
                }
                Status::Ignored
            }
            Event::PointerMove { pos, .. } => {
                if cx.state::<FollowState>().drag.is_some() {
                    self.drag_to(cx, *pos);
                    return Status::Handled;
                }
                let st = cx.state::<FollowState>();
                let near = st.bar(true).is_some_and(|(t, _)| t.inflate(4.0, 0.0).contains(*pos));
                if near != st.hover_bar {
                    st.hover_bar = near;
                    self.invalidate_bar(cx);
                }
                Status::Ignored
            }
            Event::PointerUp { .. } => {
                if cx.state::<FollowState>().drag.take().is_some() {
                    cx.release_pointer();
                    cx.state::<FollowState>().shown_until = Some(now + LINGER);
                    cx.request_tick(LINGER + Duration::from_millis(20));
                    self.invalidate_bar(cx);
                    return Status::Handled;
                }
                Status::Ignored
            }
            Event::PointerLeave => {
                let st = cx.state::<FollowState>();
                if st.hover_bar {
                    st.hover_bar = false;
                    self.invalidate_bar(cx);
                }
                Status::Ignored
            }
            Event::Tick => {
                self.invalidate_bar(cx);
                Status::Ignored
            }
            Event::Timer(SETTLE_TIMER) => {
                self.tell(cx, true);
                Status::Handled
            }
            Event::Key(k)
                if k.pressed
                    && !k.modifiers.super_key()
                    && !k.modifiers.control()
                    && (cx.is_focused() || cx.nothing_focused()) =>
            {
                let v = cx.state::<FollowState>().f().viewport;
                let page = (v - 40.0).max(40.0);
                let d = match &k.key {
                    Key::Named(NamedKey::PageDown) => page,
                    Key::Named(NamedKey::PageUp) => -page,
                    Key::Character(s) if s == " " && k.modifiers.shift() => -page,
                    Key::Character(s) if s == " " => page,
                    Key::Named(NamedKey::Home) => -1e9,
                    Key::Named(NamedKey::End) => 1e9,
                    Key::Named(NamedKey::ArrowDown) => 40.0,
                    Key::Named(NamedKey::ArrowUp) => -40.0,
                    _ => return Status::Ignored,
                };
                if self.scroll_by(cx, d) { Status::Handled } else { Status::Ignored }
            }
            _ => Status::Ignored,
        }
    }

    fn reveal(&mut self, cx: &mut EventCx, rect: Rect) -> bool {
        // `rect` is in window coordinates; the view is at its own.
        let view = cx.window_rect();
        let d = if rect.y0 < view.y0 {
            rect.y0 - view.y0 - 8.0
        } else if rect.y1 > view.y1 {
            (rect.y1 - view.y1 + 8.0).min(rect.y0 - view.y0)
        } else {
            0.0
        };
        d != 0.0 && self.scroll_by(cx, d)
    }

    fn semantics(&self) -> Option<Semantics> {
        Some(Semantics {
            role: SemanticRole::ScrollView,
            actions: vec![SemanticAction::ScrollIntoView],
            ..Default::default()
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn at_the_end_it_follows_what_arrives() {
        let mut f = Follow::restored(Reading::default());
        f.laid_out(300.0, 500.0, Some(1));
        assert_eq!((f.offset, f.at_end), (0.0, true), "short content: nothing to scroll");
        f.laid_out(900.0, 500.0, Some(1));
        assert_eq!(f.offset, 400.0);
        f.laid_out(1200.0, 500.0, Some(1));
        assert_eq!(f.offset, 700.0, "still at the end");
    }

    #[test]
    fn scrolled_up_it_stays_put_and_back_at_the_end_it_follows_again() {
        let mut f = Follow::restored(Reading::default());
        f.laid_out(1000.0, 500.0, Some(1));
        assert!(f.scroll_by(-200.0));
        assert!(!f.at_end);
        f.laid_out(1600.0, 500.0, Some(1));
        assert_eq!(f.offset, 300.0, "the reader's place, however much arrives");
        assert!(!f.at_end);
        f.scroll_by(1e9);
        assert!(f.at_end);
        f.laid_out(2000.0, 500.0, Some(1));
        assert_eq!(f.offset, 1500.0);
    }

    #[test]
    fn a_message_sent_goes_to_the_end() {
        let mut f = Follow::restored(Reading::default());
        f.laid_out(1000.0, 500.0, Some(1));
        f.scroll_by(-300.0);
        f.laid_out(1400.0, 500.0, Some(1));
        assert_eq!(f.offset, 200.0);
        f.laid_out(1500.0, 500.0, Some(2));
        assert_eq!((f.offset, f.at_end), (1000.0, true));
    }

    #[test]
    fn a_kept_place_comes_back() {
        let mut f = Follow::restored(Reading { offset: 240.0, at_end: false });
        f.laid_out(2000.0, 500.0, Some(7));
        assert_eq!((f.offset, f.at_end), (240.0, false));
        // The token it was shown with isn't a request to go to the end.
        f.laid_out(2100.0, 500.0, Some(7));
        assert_eq!(f.offset, 240.0);
    }
}

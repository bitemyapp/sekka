//! Utsushi's own elements: the timeline, where subtitles sit, and time as
//! it's written.

use std::rc::Rc;

use aizome_kit::draw;
use aizome_kit::element::MsgFn;
use aizome_kit::hash::PaintHasher;
use aizome_kit::layout::{Constraints, Point, Rect, Size};
use aizome_kit::sidestep_ui::{Cursor, PointerButton, TextLayout, TextStyle};
use aizome_kit::theme::FontSpec;
use aizome_kit::widget::{Event, EventCx, LayoutCx, PaintCx, SemanticRole, Semantics, Status, Widget};
use aizome_kit::{Element, Layoutable};

use crate::app::{Msg, Scrub};

/// `1:02:03`, or `2:03` under an hour (`0:00` for nothing known).
pub fn clock(secs: f64) -> String {
    let s = if secs.is_finite() { secs.max(0.0).floor() as u64 } else { 0 };
    let (h, m, s) = (s / 3600, s / 60 % 60, s % 60);
    if h > 0 { format!("{h}:{m:02}:{s:02}") } else { format!("{m}:{s:02}") }
}

/// The timeline: what's played, the chapters, and the knob; under the
/// pointer, the time there (and its chapter) above the track. Pressing
/// and dragging scrub; it redraws when the knob moves half a point.
pub fn timeline(position: f64, duration: f64, chapters: Rc<Vec<(f64, Option<String>)>>, enabled: bool) -> Element<Msg> {
    Element::new(Timeline { position, duration, chapters, enabled, width: 0.0 }, Vec::new())
        .key("timeline")
        .id("timeline")
        .width(aizome_kit::Length::Fill)
}

struct Timeline {
    position: f64,
    duration: f64,
    chapters: Rc<Vec<(f64, Option<String>)>>,
    enabled: bool,
    width: f64,
}

/// Where the pointer rests over the timeline.
#[derive(Default)]
struct Hover {
    x: Option<f64>,
}

/// The knob's size, as the kit's sliders.
const KNOB: (f64, f64) = (10.0, 16.0);
// Take the bar's height so the hover label fits above a centred track.
// A shorter box with a fixed low track misaligned it with every control.
const HEIGHT: f64 = crate::ui::BAR;

impl Timeline {
    fn usable(&self) -> bool {
        self.enabled && self.duration > 0.0
    }

    fn track(&self) -> (f64, f64) {
        (KNOB.0 / 2.0, (self.width - KNOB.0 / 2.0).max(KNOB.0 / 2.0 + 1.0))
    }

    fn time_at(&self, x: f64) -> f64 {
        let (a, b) = self.track();
        ((x - a) / (b - a)).clamp(0.0, 1.0) * self.duration
    }

    fn x_of(&self, t: f64) -> f64 {
        let (a, b) = self.track();
        if self.duration <= 0.0 { a } else { a + (t / self.duration).clamp(0.0, 1.0) * (b - a) }
    }

    fn chapter_at(&self, t: f64) -> Option<&str> {
        self.chapters.iter().rev().find(|(s, _)| *s <= t).and_then(|(_, title)| title.as_deref())
    }

    fn send(cx: &mut EventCx, s: Scrub) {
        cx.emit(&MsgFn::new(Msg::Scrub(s)));
    }
}

impl Widget for Timeline {
    fn name(&self) -> &'static str {
        "utsushi-timeline"
    }

    fn layout(&mut self, _cx: &mut LayoutCx, bc: Constraints) -> Size {
        let w = if bc.max.width.is_finite() { bc.max.width } else { 300.0 };
        let size = bc.constrain(Size::new(w.max(80.0), HEIGHT));
        self.width = size.width;
        size
    }

    fn paint(&self, cx: &mut PaintCx) {
        let t = cx.theme();
        let usable = self.usable();
        let (a, b) = self.track();
        let track_y = (cx.bounds().height() / 2.0).round();
        let track = Rect::new(a, track_y - 2.0, b, track_y + 2.0);
        draw::sunken(cx.canvas, t, track, t.sunk(), if usable { t.line2() } else { t.line() });
        let x = self.x_of(self.position).round();
        if usable {
            cx.canvas.fill_rect(Rect::new(track.x0 + 1.0, track.y0 + 1.0, x, track.y1 - 1.0), t.selection());
            // Chapters part the track.
            for (start, _) in self.chapters.iter().filter(|(s, _)| *s > 0.5) {
                let cxp = self.x_of(*start).round();
                cx.canvas.fill_rect(Rect::new(cxp - 0.5, track.y0, cxp + 1.0, track.y1), t.chrome());
            }
        }
        let hover = cx.state::<Hover>().and_then(|h| h.x).filter(|_| usable);
        if let Some(hx) = hover.filter(|_| !cx.is_pressed()) {
            let hx = hx.clamp(a, b).round();
            cx.canvas.fill_rect(Rect::new(hx, track.y0 - 3.0, hx + 1.0, track.y1 + 3.0), t.text3());
        }
        // The time under the pointer, or being dragged to, above the track.
        let label_at = if cx.is_pressed() { Some(x) } else { hover };
        if let Some(lx) = label_at {
            let at = self.time_at(lx);
            let mut s = clock(at);
            if let Some(c) = self.chapter_at(at) {
                s = format!("{s}  {c}");
            }
            let font = t.font(FontSpec { tabular: true, ..FontSpec::mono(t.metrics.small) });
            let layout = TextLayout::new(&s, &TextStyle::new(font, t.text2()));
            let w = layout.size().width;
            let left = (lx - w / 2.0).clamp(0.0, (self.width - w).max(0.0)).round();
            cx.canvas.draw_text(&layout, Point::new(left, 1.0));
        }
        if usable {
            let knob = Rect::new(x - KNOB.0 / 2.0, track_y - KNOB.1 / 2.0, x + KNOB.0 / 2.0, track_y + KNOB.1 / 2.0);
            let face = if cx.is_pressed() {
                t.sunk()
            } else if cx.is_hovered() {
                t.hover(t.raised())
            } else {
                t.raised()
            };
            cx.canvas.fill_rect(knob, face);
            draw::border(cx.canvas, knob, t.bevel_lo(), 1.0);
            if !cx.is_pressed() {
                draw::top(cx.canvas, knob.inset(-1.0), t.bevel_hi(), 1.0);
            }
        }
    }

    fn hash(&self, h: &mut PaintHasher) {
        // The knob's place to half a point: it redraws when that moves.
        let step = (self.x_of(self.position) * 2.0).round() as i64;
        h.add(&(step, self.usable(), self.chapters.len())).f64(self.duration).f64(self.width);
    }

    fn hit(&self) -> bool {
        true
    }

    fn hover_paints(&self) -> bool {
        true
    }

    fn event(&mut self, cx: &mut EventCx, event: &Event) -> Status {
        if !self.usable() {
            return Status::Ignored;
        }
        match event {
            Event::PointerDown { pos, button: PointerButton::Left, .. } => {
                Self::send(cx, Scrub::Start(self.time_at(pos.x)));
                cx.invalidate();
                Status::Handled
            }
            Event::PointerMove { pos, .. } => {
                cx.state::<Hover>().x = Some(pos.x);
                cx.invalidate();
                if cx.is_pressed() {
                    Self::send(cx, Scrub::Move(self.time_at(pos.x)));
                    return Status::Handled;
                }
                // On to what's around it: full screen's controls stay
                // while the pointer is over them, the timeline included.
                Status::Ignored
            }
            Event::PointerUp { pos, button: PointerButton::Left, .. } => {
                Self::send(cx, Scrub::End(self.time_at(pos.x)));
                cx.invalidate();
                Status::Handled
            }
            Event::PointerLeave => {
                cx.state::<Hover>().x = None;
                cx.invalidate();
                Status::Handled
            }
            _ => Status::Ignored,
        }
    }

    fn cursor(&self) -> Option<Cursor> {
        self.usable().then_some(Cursor::Pointer)
    }

    fn semantics(&self) -> Option<Semantics> {
        Some(Semantics {
            role: SemanticRole::Slider,
            label: Some("Timeline".into()),
            value: Some(clock(self.position)),
            range: Some((0.0, self.duration, self.position)),
            disabled: !self.usable(),
            ..Default::default()
        })
    }
}

/// Where subtitles sit: in the letterbox's bar under the picture when
/// they fit there (so the picture stays uncovered, on its own plane),
/// else over the picture's bottom, a little up from its edge. Lays its
/// one child (the cues' box, in `over_video`) out at its natural size.
pub fn subtitle_place(picture: Option<(f64, f64)>, top: bool, child: Element<Msg>) -> Element<Msg> {
    Element::new(SubtitlePlace { picture, top }, vec![child])
        .width(aizome_kit::Length::Fill)
        .height(aizome_kit::Length::Fill)
}

struct SubtitlePlace {
    picture: Option<(f64, f64)>,
    top: bool,
}

impl Widget for SubtitlePlace {
    fn name(&self) -> &'static str {
        "utsushi-subtitles"
    }

    fn layout(&mut self, cx: &mut LayoutCx, bc: Constraints) -> Size {
        let room = bc.constrain(bc.max);
        if cx.child_count() == 0 {
            return room;
        }
        let pic = aizome_kit::video::fit(room, self.picture.map(|(w, h)| Size::new(w, h)));
        let width = (pic.width() * 0.9).max(120.0).min(room.width);
        let s = cx.layout_child(0, Constraints { min: Size::ZERO, max: Size::new(width, room.height) });
        let x = (pic.x0 + (pic.width() - s.width) / 2.0).round();
        let margin = (pic.height() * 0.045).clamp(6.0, 40.0);
        let bar = room.height - pic.y1;
        let y = if self.top {
            if pic.y0 >= s.height + 8.0 { (pic.y0 - s.height - 4.0).round() } else { (pic.y0 + margin).round() }
        } else if bar >= s.height + 8.0 {
            // In the letterbox: the picture stays clear.
            (pic.y1 + (bar - s.height) / 2.0).round()
        } else {
            (pic.y1 - margin - s.height).round()
        };
        cx.place_child(0, Point::new(x, y.max(0.0)));
        room
    }
}

/// A time, a character to a cell: each cell its own element keyed by its
/// place from the right, so a second passing redraws its last digit
/// alone (the mono face's digits are one width).
pub fn digits(s: &str, font: FontSpec, role: aizome_kit::Role) -> Element<Msg> {
    let chars: Vec<char> = s.chars().collect();
    let n = chars.len();
    let cells: Vec<Element<Msg>> = chars
        .iter()
        .enumerate()
        .map(|(i, c)| aizome_kit::text(c.to_string()).font(font).role(role).key(n - i).into())
        .collect();
    aizome_kit::row(cells).label(s.to_owned()).into()
}

/// `child` on a dark, see-through box: subtitles over any picture.
pub fn scrim(child: Element<Msg>) -> Element<Msg> {
    Element::new(Scrim, vec![child])
}

struct Scrim;

impl Widget for Scrim {
    fn name(&self) -> &'static str {
        "utsushi-scrim"
    }

    fn paint(&self, cx: &mut PaintCx) {
        let b = cx.bounds();
        cx.canvas.fill_rect(b, aizome_kit::Color::rgba(0.0, 0.0, 0.0, 0.72));
    }
}

/// `child` as a square as large as the room allows, up to `max` points,
/// leaving `reserve` points of the height for what goes under it,
/// centred: a cover that shrinks with a small window rather than pushing
/// its title out of sight.
pub fn square(max: f64, reserve: f64, child: Element<Msg>) -> Element<Msg> {
    Element::new(Square { max, reserve }, vec![child]).width(aizome_kit::Length::Fill)
}

struct Square {
    max: f64,
    reserve: f64,
}

impl Widget for Square {
    fn name(&self) -> &'static str {
        "utsushi-square"
    }

    fn layout(&mut self, cx: &mut LayoutCx, bc: Constraints) -> Size {
        let room = bc.constrain(bc.max);
        let tall = if room.height.is_finite() { room.height - self.reserve } else { self.max };
        let side = room.width.min(tall).min(self.max).max(0.0).floor();
        if cx.child_count() > 0 {
            cx.layout_child(0, Constraints { min: Size::new(side, side), max: Size::new(side, side) });
            cx.place_child(0, Point::new(((room.width - side) / 2.0).round(), 0.0));
        }
        // Only as tall as the square: what's under it stays with it.
        Size::new(room.width, side)
    }
}

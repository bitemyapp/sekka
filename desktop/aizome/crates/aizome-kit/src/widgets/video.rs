//! A video in the window: where a player's frames show, shown by the
//! compositor and never drawn by the kit (see `sidestep_ui::video`).
//!
//! `video_surface(id)` takes the room it's given and fits the picture in
//! it (`.picture(w, h)`, its display size or just its shape), with the
//! rest of the room as `ground` (black by default): the bars of a
//! letterbox. After every layout the kit places video `id` of the window
//! on the picture's rectangle, in whole points.
//!
//! Over the window's drawing (the default) nothing of the window covers
//! it, and the compositor can show it on a display plane of its own,
//! composing nothing. What the view draws over a video (controls,
//! subtitles) goes in [`over_video`]: after each layout, a video that
//! one of those overlaps goes under the window's drawing, which draws it
//! clear, until none does; so subtitles set in a letterbox's bar, or
//! controls that hide, leave it on its plane. `.below(true)` keeps it
//! under always. A window with a video under it must be see-through
//! ([`WindowSpec::video`]).
//!
//! The frames themselves go through a [`VideoSink`](sidestep_ui::video::VideoSink)
//! from [`Cx::video_sink`](crate::Cx::video_sink), on any thread.
//!
//! [`WindowSpec::video`]: crate::WindowSpec::video

use std::marker::PhantomData;

use sidestep_ui::{BlendMode, Color};

use crate::element::{Common, Element, RawElement};
use crate::layout::{Constraints, Length, Point, Rect, Size};
use crate::widget::{LayoutCx, PaintCx, SemanticRole, Semantics, Widget};

pub use sidestep_ui::video::VideoId;

/// What the kit decided after the last layout: the video is under the
/// window's drawing (something over it overlaps it).
#[derive(Default)]
pub(crate) struct Placed {
    pub below: bool,
}

/// What a video element asks of its window: the video, where it shows in
/// the element (its points), and how.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct VideoSpec {
    pub id: VideoId,
    pub rect: Rect,
    pub below: bool,
    pub inhibit_idle: bool,
}

/// A video's place in a view. See the module's documentation.
pub struct VideoEl<Msg> {
    pub(crate) common: Common,
    id: VideoId,
    picture: Option<Size>,
    below: bool,
    inhibit_idle: bool,
    ground: Color,
    _m: PhantomData<fn() -> Msg>,
}

crate::builder!(VideoEl);

/// Video `id`'s place: all the room it's given (`Fill` both ways).
pub fn video_surface<Msg>(id: VideoId) -> VideoEl<Msg> {
    VideoEl {
        common: Common::sized(Length::Fill, Length::Fill),
        id,
        picture: None,
        below: false,
        inhibit_idle: false,
        ground: Color::BLACK,
        _m: PhantomData,
    }
}

impl<Msg> VideoEl<Msg> {
    /// The picture's display size (or any size of its shape): it's fitted
    /// in the room, centred. Without it the picture takes all the room.
    pub fn picture(mut self, width: f64, height: f64) -> Self {
        self.picture = (width > 0.0 && height > 0.0).then(|| Size::new(width, height));
        self
    }

    /// Under the window's drawing, which shows over it where the view
    /// stacks something over this element; else over everything.
    pub fn below(mut self, below: bool) -> Self {
        self.below = below;
        self
    }

    /// Keep the screen awake while it shows (a video playing).
    pub fn inhibit_idle(mut self, on: bool) -> Self {
        self.inhibit_idle = on;
        self
    }

    /// What shows around the picture.
    pub fn ground(mut self, color: Color) -> Self {
        self.ground = color;
        self
    }
}

impl<Msg> From<VideoEl<Msg>> for Element<Msg> {
    fn from(v: VideoEl<Msg>) -> Element<Msg> {
        Element::from_raw(RawElement::new(
            VideoWidget {
                id: v.id,
                picture: v.picture,
                below: v.below,
                inhibit_idle: v.inhibit_idle,
                ground: v.ground,
                size: Size::ZERO,
            },
            v.common,
            Vec::new(),
        ))
    }
}

struct VideoWidget {
    id: VideoId,
    picture: Option<Size>,
    below: bool,
    inhibit_idle: bool,
    ground: Color,
    size: Size,
}

/// Where a picture of `picture`'s shape fits in `room`, centred, on whole
/// points.
pub fn fit(room: Size, picture: Option<Size>) -> Rect {
    let Some(p) = picture else { return Rect::from_origin_size(Point::ZERO, room) };
    let s = (room.width / p.width).min(room.height / p.height);
    let (w, h) = ((p.width * s).round().min(room.width), (p.height * s).round().min(room.height));
    let x = ((room.width - w) / 2.0).round();
    let y = ((room.height - h) / 2.0).round();
    Rect::new(x, y, x + w, y + h)
}

impl Widget for VideoWidget {
    fn name(&self) -> &'static str {
        "video"
    }

    fn layout(&mut self, _cx: &mut LayoutCx, bc: Constraints) -> Size {
        let room = |max: f64, fallback: f64| if max.is_finite() { max } else { fallback };
        let size = bc.constrain(Size::new(room(bc.max.width, 640.0), room(bc.max.height, 360.0)));
        self.size = size;
        size
    }

    fn paint(&self, cx: &mut PaintCx) {
        let bounds = cx.bounds();
        cx.canvas.fill_rect(bounds, self.ground);
        if self.below || cx.state::<Placed>().is_some_and(|p| p.below) {
            let hole = fit(cx.size(), self.picture);
            cx.canvas.set_blend_mode(BlendMode::Clear);
            cx.canvas.fill_rect(hole, Color::TRANSPARENT);
            cx.canvas.set_blend_mode(BlendMode::Normal);
        }
    }

    fn hash(&self, h: &mut crate::hash::PaintHasher) {
        h.add(&self.below);
        h.color(self.ground);
        if let Some(p) = self.picture {
            h.add(&(p.width.to_bits(), p.height.to_bits()));
        }
    }

    fn video(&self) -> Option<VideoSpec> {
        Some(VideoSpec {
            id: self.id,
            rect: fit(self.size, self.picture),
            below: self.below,
            inhibit_idle: self.inhibit_idle,
        })
    }

    fn semantics(&self) -> Option<Semantics> {
        Some(Semantics { role: SemanticRole::Image, label: Some("Video".into()), ..Default::default() })
    }
}

/// `el`, drawn over the window's videos: a video it overlaps (its
/// rectangle as laid out, not what it paints) goes under the window's
/// drawing while it does. Wrap exactly what draws (a subtitle's box, a
/// bar of controls), not the room around it.
pub fn over_video<Msg: 'static>(el: impl Into<Element<Msg>>) -> Element<Msg> {
    Element::new(OverVideo, vec![el.into()])
}

struct OverVideo;

impl Widget for OverVideo {
    fn name(&self) -> &'static str {
        "over-video"
    }

    fn over_video(&self) -> bool {
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pictures_fit_centred_on_whole_points() {
        let room = Size::new(800.0, 600.0);
        assert_eq!(fit(room, None), Rect::new(0.0, 0.0, 800.0, 600.0));
        // 16:9 in 4:3: bars above and below.
        assert_eq!(fit(room, Some(Size::new(1920.0, 1080.0))), Rect::new(0.0, 75.0, 800.0, 525.0));
        // 9:16 in 4:3: bars at the sides.
        let r = fit(room, Some(Size::new(1080.0, 1920.0)));
        assert_eq!((r.width(), r.height()), (338.0, 600.0));
        assert_eq!(r.x0, 231.0);
    }
}

//! The partition map: the disk as a bar, each segment as wide as its
//! bytes (with a floor, so a 1 MiB BIOS boot partition can still be
//! clicked), coloured by its filesystem, its used space shaded,
//! unallocated space hatched, what pending operations change outlined in
//! dashes, and the selected segment outlined in the focus colour.

use std::cell::RefCell;
use std::hash::{Hash, Hasher};
use std::rc::Rc;

use aizome_kit::element::{MsgFn, MsgMap};
use aizome_kit::hash::PaintHasher;
use aizome_kit::layout::{Align, Constraints, Point, Rect, Size};
use aizome_kit::sidestep_ui::kurbo::{BezPath, Line, Shape};
use aizome_kit::sidestep_ui::{BlendMode, Image, ImageOptions, Key, NamedKey, PointerButton, StrokeStyle};
use aizome_kit::text::TextSpec;
use aizome_kit::theme::{FontSpec, Theme};
use aizome_kit::widget::{Event, EventCx, Focusable, LayoutCx, PaintCx, SemanticRole, Semantics, Status, Widget};
use aizome_kit::{Color, Element, Icon, Layoutable, Length};
use aizome_sys::disks::format_size;

use crate::fs;
use crate::layout::{Layout, SegKind};

pub const HEIGHT: f64 = 64.0;
/// The narrowest a segment is drawn.
const MIN_WIDTH: f64 = 10.0;

/// The map of `layout`, with `selected` outlined.
pub fn partition_map<Msg: Send + 'static>(
    layout: Rc<Layout>,
    selected: Option<usize>,
    on_select: impl Fn(usize) -> Msg + 'static,
    on_context: impl Fn((usize, Point)) -> Msg + 'static,
) -> Element<Msg> {
    let w = Map { layout, selected, on_select: MsgMap::new(on_select), on_context: MsgMap::new(on_context) };
    Element::new(w, Vec::new()).width(Length::Fill).height(HEIGHT).id("partition-map").label("Partition map")
}

struct Map {
    layout: Rc<Layout>,
    selected: Option<usize>,
    on_select: MsgMap<usize>,
    on_context: MsgMap<(usize, Point)>,
}

/// Widths for `sizes` filling `width`, none under the floor.
pub fn widths(sizes: &[u64], width: f64) -> Vec<f64> {
    let n = sizes.len();
    if n == 0 {
        return Vec::new();
    }
    let floor = MIN_WIDTH.min(width / n as f64);
    let mut fixed = vec![false; n];
    loop {
        let rest: u64 = sizes.iter().zip(&fixed).filter(|(_, f)| !**f).map(|(s, _)| *s).sum();
        let room = width - fixed.iter().filter(|f| **f).count() as f64 * floor;
        let mut changed = false;
        for (i, s) in sizes.iter().enumerate() {
            if !fixed[i] && (rest == 0 || (*s as f64 / rest as f64) * room < floor) {
                fixed[i] = true;
                changed = true;
            }
        }
        if !changed {
            return sizes
                .iter()
                .zip(&fixed)
                .map(|(s, f)| {
                    if *f {
                        floor
                    } else if rest == 0 {
                        0.0
                    } else {
                        *s as f64 / rest as f64 * room
                    }
                })
                .collect();
        }
    }
}

impl Map {
    /// Each segment's box, in the widget's coordinates.
    fn boxes(&self, size: Size) -> Vec<(usize, Rect)> {
        let segs = &self.layout.segs;
        let inner = Rect::new(1.0, 1.0, size.width - 1.0, size.height - 1.0);
        let top: Vec<usize> = (0..segs.len()).filter(|&i| !segs[i].logical).collect();
        let w = widths(&top.iter().map(|&i| segs[i].size).collect::<Vec<_>>(), inner.width());
        let mut out = Vec::new();
        let mut x = inner.x0;
        for (k, &i) in top.iter().enumerate() {
            let r = Rect::new(x.round(), inner.y0, (x + w[k]).round(), inner.y1);
            x += w[k];
            out.push((i, r));
            if segs[i].kind() == SegKind::Extended {
                let logical: Vec<usize> = (0..segs.len())
                    .filter(|&j| segs[j].logical && segs[j].offset >= segs[i].offset && segs[j].offset < segs[i].end())
                    .collect();
                let lr = Rect::new(r.x0 + 3.0, r.y0 + 14.0, r.x1 - 3.0, r.y1 - 3.0);
                let lw = widths(&logical.iter().map(|&j| segs[j].size).collect::<Vec<_>>(), lr.width());
                let mut lx = lr.x0;
                for (m, &j) in logical.iter().enumerate() {
                    out.push((j, Rect::new(lx.round(), lr.y0, (lx + lw[m]).round(), lr.y1)));
                    lx += lw[m];
                }
            }
        }
        out
    }

    fn hit(&self, size: Size, p: Point) -> Option<usize> {
        // Logical partitions lie over their extended one: the last box wins.
        self.boxes(size).into_iter().rev().find(|(_, r)| r.contains(p)).map(|(i, _)| i)
    }
}

thread_local! {
    /// Hatches drawn, by where and how they're drawn (the last few).
    static HATCHES: RefCell<Vec<(u64, Image)>> = const { RefCell::new(Vec::new()) };
}

/// The most hatches kept: a few drives' free space.
const HATCHES_KEPT: usize = 8;

/// `r` on `ground`, hatched in `color`. A hundred and fifty antialiased
/// lines across a large disk's free space take a few milliseconds to
/// draw (each crosses every row the rasterizer walks), so the hatch is
/// drawn once for its size, scale and colours, offscreen, landing on the
/// device's pixels as it would here, and copied one to one after.
fn hatch(canvas: &mut aizome_kit::sidestep_ui::Canvas, r: Rect, ground: Color, color: Color) {
    let scale = canvas.scale();
    let at = canvas.transform() * r.origin();
    let frac = |v: f64| v * scale - (v * scale).floor();
    let offset = aizome_kit::Vec2::new(frac(at.x) / scale, frac(at.y) / scale);
    let size =
        Size::new(((r.width() + offset.x) * scale).ceil() / scale, ((r.height() + offset.y) * scale).ceil() / scale);
    let key = {
        let mut h = std::collections::hash_map::DefaultHasher::new();
        for v in [r.width(), r.height(), scale, offset.x, offset.y] {
            v.to_bits().hash(&mut h);
        }
        for c in [ground, color] {
            [c.r, c.g, c.b, c.a].map(f32::to_bits).hash(&mut h);
        }
        h.finish()
    };
    let image = HATCHES.with(|kept| {
        let mut kept = kept.borrow_mut();
        if let Some(i) = kept.iter().position(|(k, _)| *k == key) {
            let hit = kept.remove(i);
            kept.push(hit);
        } else {
            let lines = hatch_lines(r.size());
            let appearance = aizome_kit::sidestep_ui::Appearance::DARK;
            let image = aizome_kit::sidestep_ui::offscreen::render(size, scale, appearance, |c| {
                c.fill_rect(Rect::from_origin_size(Point::ZERO, size), ground);
                c.translate(offset);
                c.clip_rect(Rect::from_origin_size(Point::ZERO, r.size()));
                c.stroke(&lines, &StrokeStyle::new(1.0), color);
            });
            kept.push((key, image));
            if kept.len() > HATCHES_KEPT {
                kept.remove(0);
            }
        }
        kept.last().map(|(_, image)| image.clone())
    });
    let Some(image) = image else { return };
    canvas.save();
    canvas.clip_rect(r);
    canvas.set_blend_mode(BlendMode::Copy);
    let place = Rect::from_origin_size(r.origin() - offset, image.size());
    canvas.draw_image_with(&image, None, place, ImageOptions { smooth: false, ..ImageOptions::default() });
    canvas.restore();
}

/// The hatch's lines over a rectangle `size` big at the origin: one path,
/// stroked once (they never touch, so it draws what a stroke for each
/// did).
fn hatch_lines(size: Size) -> BezPath {
    let mut lines = BezPath::new();
    let mut x = -size.height;
    while x < size.width {
        lines.extend(Line::new((x, size.height), (x + size.height, 0.0)).path_elements(0.1));
        x += 6.0;
    }
    lines
}

/// A segment's fills: its tint, its used part, its stripe.
fn tints(theme: &Theme, fs: &str) -> (Color, Color, Color) {
    let base = fs::color(fs, theme);
    let ground = theme.sunk();
    if theme.is_dark() {
        (ground.mix(base, 0.2), ground.mix(base, 0.5), base)
    } else {
        (ground.mix(base, 0.16), ground.mix(base, 0.36), base)
    }
}

impl Widget for Map {
    fn name(&self) -> &'static str {
        "tansu-map"
    }

    fn layout(&mut self, _cx: &mut LayoutCx, bc: Constraints) -> Size {
        bc.constrain(Size::new(if bc.max.width.is_finite() { bc.max.width } else { 600.0 }, HEIGHT))
    }

    fn paint(&self, cx: &mut PaintCx) {
        let t = cx.theme().clone();
        let b = cx.bounds();
        cx.canvas.fill_rect(b, t.sunk());
        let boxes = self.boxes(b.size());
        let small = |w: f32| FontSpec { weight: w, ..FontSpec::ui(t.metrics.small) };
        for (k, (i, r)) in boxes.iter().enumerate() {
            let seg = &self.layout.segs[*i];
            let r = *r;
            match seg.kind() {
                SegKind::Free | SegKind::Blank => {
                    hatch(cx.canvas, r, t.sunk(), t.line2().with_alpha(if t.is_dark() { 0.8 } else { 0.7 }));
                    if r.width() > 48.0 {
                        let label = if seg.kind() == SegKind::Blank { "No partition table" } else { "Unallocated" };
                        let pad = Rect::new(r.x0 + 6.0, r.y0 + 8.0, r.x1 - 4.0, r.y0 + 26.0);
                        let w = cx.layout_text(label, &TextSpec::new(small(500.0), t.text2()), None).size().width;
                        cx.canvas.fill_rect(
                            Rect::new(pad.x0 - 2.0, pad.y0 + 1.0, (pad.x0 + w + 2.0).min(pad.x1), pad.y1 - 1.0),
                            t.sunk(),
                        );
                        cx.draw_text_in(label, &TextSpec::new(small(500.0), t.text2()), pad, Align::Start);
                        let size = format_size(seg.size);
                        let pad2 = Rect::new(r.x0 + 6.0, r.y0 + 26.0, r.x1 - 4.0, r.y0 + 42.0);
                        let w = cx.layout_text(&size, &TextSpec::new(small(400.0), t.text3()), None).size().width;
                        cx.canvas.fill_rect(
                            Rect::new(pad2.x0 - 2.0, pad2.y0 + 1.0, (pad2.x0 + w + 2.0).min(pad2.x1), pad2.y1 - 1.0),
                            t.sunk(),
                        );
                        cx.draw_text_in(
                            &size,
                            &TextSpec::new(FontSpec { tabular: true, ..small(400.0) }, t.text3()),
                            pad2,
                            Align::Start,
                        );
                    }
                }
                SegKind::Extended => {
                    cx.canvas.fill_rect(r, t.sunk().mix(t.line(), 0.35));
                    cx.canvas.stroke(&r.inset(-0.5).to_path(0.1), &StrokeStyle::new(1.0), t.line2());
                    if r.width() > 60.0 {
                        let cap = Rect::new(r.x0 + 5.0, r.y0, r.x1 - 4.0, r.y0 + 14.0);
                        let label = format!("{} · extended", seg.short_name());
                        cx.draw_text_in(&label, &TextSpec::new(FontSpec::ui(10.0), t.text3()), cap, Align::Start);
                    }
                }
                SegKind::Partition | SegKind::Whole => {
                    let shown = seg.shown_fs().to_owned();
                    let (tint, used, stripe) = tints(&t, if seg.is_locked() { "crypto_LUKS" } else { &shown });
                    cx.canvas.fill_rect(r, tint);
                    if let Some(u) = seg.shown_used() {
                        let total =
                            seg.crypt.as_ref().and_then(|c| c.unlocked.as_ref()).map_or(seg.size, |i| i.size.max(1));
                        let f = (u as f64 / total as f64).clamp(0.0, 1.0);
                        cx.canvas.fill_rect(Rect::new(r.x0, r.y0 + 4.0, r.x0 + (r.width() * f).round(), r.y1), used);
                    }
                    cx.canvas.fill_rect(Rect::new(r.x0, r.y0, r.x1, r.y0 + 4.0), stripe);
                    if r.width() > 34.0 {
                        let mut x = r.x0 + 7.0;
                        if seg.crypt.is_some() && r.width() > 70.0 {
                            let icon = if seg.is_locked() { Icon::Lock } else { Icon::LockOpen };
                            icon.draw(cx.canvas, Point::new(x, r.y0 + 11.0), 12.0, t.text2());
                            x += 16.0;
                        }
                        let name = seg.short_name();
                        cx.draw_text_in(
                            &name,
                            &TextSpec::new(small(650.0), t.text()),
                            Rect::new(x, r.y0 + 8.0, r.x1 - 4.0, r.y0 + 26.0),
                            Align::Start,
                        );
                        let what = if seg.is_locked() { "LUKS, locked".to_owned() } else { fs::name(&shown) };
                        cx.draw_text_in(
                            &what,
                            &TextSpec::new(small(400.0), t.text2()),
                            Rect::new(r.x0 + 7.0, r.y0 + 25.0, r.x1 - 4.0, r.y0 + 41.0),
                            Align::Start,
                        );
                        let size = format_size(seg.size);
                        cx.draw_text_in(
                            &size,
                            &TextSpec::new(FontSpec { tabular: true, ..small(400.0) }, t.text3()),
                            Rect::new(r.x0 + 7.0, r.y0 + 41.0, r.x1 - 4.0, r.y0 + 57.0),
                            Align::Start,
                        );
                    }
                }
            }
            if !seg.pending.is_empty() {
                let style = StrokeStyle::new(1.5).with_dash(vec![4.0, 3.0], 0.0);
                cx.canvas.stroke(&r.inset(-2.5).to_path(0.1), &style, t.text());
            }
            // A rule between neighbours.
            if k > 0 && !seg.logical {
                cx.canvas.fill_rect(Rect::new(r.x0 - 0.5, r.y0, r.x0 + 0.5, r.y1), t.bevel_lo());
            }
        }
        if let Some((_, r)) = self.selected.and_then(|s| boxes.iter().find(|(i, _)| *i == s)) {
            cx.canvas.stroke(&r.inset(-1.0).to_path(0.1), &StrokeStyle::new(2.0), t.focus());
        }
        cx.canvas.stroke(&b.inset(-0.5).to_path(0.1), &StrokeStyle::new(1.0), t.bevel_lo());
    }

    fn hash(&self, h: &mut PaintHasher) {
        h.add(&*self.layout).add(&self.selected);
    }

    fn hit(&self) -> bool {
        true
    }

    fn focusable(&self) -> Focusable {
        Focusable::Click
    }

    fn event(&mut self, cx: &mut EventCx, event: &Event) -> Status {
        match event {
            Event::PointerDown { pos, button: PointerButton::Left, mods, .. } if !mods.control() => {
                cx.request_focus();
                if let Some(i) = self.hit(cx.size(), *pos) {
                    emit(cx, &self.on_select, i);
                }
                Status::Handled
            }
            Event::PointerDown { pos, button, mods, .. } if *button == PointerButton::Right || mods.control() => {
                if let Some(i) = self.hit(cx.size(), *pos) {
                    let at = cx.window_rect().origin() + pos.to_vec2();
                    emit(cx, &self.on_context, (i, at));
                }
                Status::Handled
            }
            Event::Key(k) if k.pressed => {
                let n = self.layout.segs.len();
                if n == 0 {
                    return Status::Ignored;
                }
                let cur = self.selected;
                let next = match &k.key {
                    Key::Named(NamedKey::ArrowRight) => Some(cur.map_or(0, |c| (c + 1).min(n - 1))),
                    Key::Named(NamedKey::ArrowLeft) => Some(cur.map_or(0, |c| c.saturating_sub(1))),
                    Key::Named(NamedKey::Home) => Some(0),
                    Key::Named(NamedKey::End) => Some(n - 1),
                    _ => None,
                };
                match next {
                    Some(i) => {
                        emit(cx, &self.on_select, i);
                        Status::Handled
                    }
                    None => Status::Ignored,
                }
            }
            _ => Status::Ignored,
        }
    }

    fn semantics(&self) -> Option<Semantics> {
        let value = self
            .selected
            .and_then(|i| self.layout.segs.get(i))
            .map(|s| format!("{}, {}", s.short_name(), format_size(s.size)));
        Some(Semantics { role: SemanticRole::Canvas, label: Some("Partition map".into()), value, ..Default::default() })
    }
}

/// Send what `map` makes of `value`.
pub fn emit<T: Clone + 'static>(cx: &mut EventCx, map: &MsgMap<T>, value: T) {
    let m = map.clone();
    cx.emit(&MsgFn::from_fn(move || m.make(value.clone())));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn small_segments_keep_a_floor() {
        let w = widths(&[1 << 20, 500 << 30, 30 << 30], 600.0);
        assert!((w.iter().sum::<f64>() - 600.0).abs() < 0.01);
        assert_eq!(w[0], MIN_WIDTH);
        assert!(w[1] > w[2] * 10.0);
    }
}

//! Small widgets the hardware and network panes share: a busy spinner for
//! rows that are joining, pairing or connecting; the Wi-Fi signal glyph
//! (arcs lit by strength, as macOS draws them); a level meter for sound
//! input; and the device's dye for a glyph tile.

use std::time::{Duration, Instant};

use aizome_kit::element::Element;
use aizome_kit::hash::PaintHasher;
use aizome_kit::layout::{Constraints, Point, Rect, Size};
use aizome_kit::sidestep_ui::kurbo::{Arc as KArc, Circle, Line, Shape};
use aizome_kit::sidestep_ui::{LineCap, StrokeStyle};
use aizome_kit::widget::{Event, EventCx, LayoutCx, PaintCx, SemanticRole, Semantics, Status, Widget};
use aizome_kit::{Color, Layoutable};

/// A busy spinner `size` points square: eight ticks going round, slowly;
/// under reduced motion, three still dots.
pub fn spinner<Msg>(size: f64) -> Element<Msg> {
    let mut e = Element::new(Spinner { size, moving: false }, Vec::new());
    e = e.label("Busy");
    e
}

struct Spinner {
    size: f64,
    moving: bool,
}

#[derive(Default)]
struct Spin {
    started: Option<Instant>,
}

const SPIN_FRAME: Duration = Duration::from_millis(100);

impl Widget for Spinner {
    fn name(&self) -> &'static str {
        "spinner"
    }

    fn layout(&mut self, cx: &mut LayoutCx, bc: Constraints) -> Size {
        self.moving = cx.theme().motion();
        if self.moving {
            let now = cx.now();
            cx.state::<Spin>().started.get_or_insert(now);
            cx.request_tick(SPIN_FRAME);
        }
        bc.constrain(Size::new(self.size, self.size))
    }

    fn paint(&self, cx: &mut PaintCx) {
        let t = cx.theme();
        let b = cx.bounds();
        let c = b.center();
        let r = self.size / 2.0;
        if !self.moving {
            for i in -1..=1 {
                let dot = Circle::new(Point::new(c.x + i as f64 * r * 0.6, c.y), (r * 0.16).max(1.0));
                cx.canvas.fill(&dot, t.text3());
            }
            return;
        }
        let started = cx.state::<Spin>().and_then(|s| s.started).unwrap_or(cx.now());
        let step = (cx.now().duration_since(started).as_millis() / SPIN_FRAME.as_millis()) as usize % 8;
        let style = StrokeStyle::new((self.size / 9.0).max(1.25)).with_cap(LineCap::Round);
        for i in 0..8 {
            let a = (i as f64) * std::f64::consts::TAU / 8.0 - std::f64::consts::FRAC_PI_2;
            let (s, co) = a.sin_cos();
            let from = Point::new(c.x + co * r * 0.45, c.y + s * r * 0.45);
            let to = Point::new(c.x + co * r * 0.9, c.y + s * r * 0.9);
            // The newest tick brightest, the rest fading behind it.
            let age = (step + 8 - i) % 8;
            let alpha = 1.0 - age as f32 * 0.11;
            cx.canvas.stroke(&Line::new(from, to), &style, t.text2().with_alpha(alpha.max(0.2)));
        }
    }

    fn hash(&self, h: &mut PaintHasher) {
        h.f64(self.size).add(&self.moving);
    }

    fn event(&mut self, cx: &mut EventCx, event: &Event) -> Status {
        if *event == Event::Tick && self.moving {
            cx.invalidate();
            cx.request_tick(SPIN_FRAME);
            return Status::Handled;
        }
        Status::Ignored
    }

    fn semantics(&self) -> Option<Semantics> {
        Some(Semantics { role: SemanticRole::ProgressIndicator, ..Default::default() })
    }
}

/// The Wi-Fi glyph `size` points square with `bars` (1–4) of 4 lit: a dot
/// and three arcs above it, the unlit ones faint. `strong` draws the lit
/// part in the text colour, else in `text2` (a network not in use).
pub fn signal<Msg>(bars: u8, size: f64, strong: bool) -> Element<Msg> {
    let label = format!("Signal {bars} of 4");
    Element::new(Signal { bars: bars.clamp(0, 4), size, strong }, Vec::new()).label(label)
}

struct Signal {
    bars: u8,
    size: f64,
    strong: bool,
}

impl Widget for Signal {
    fn name(&self) -> &'static str {
        "signal"
    }

    fn layout(&mut self, _cx: &mut LayoutCx, bc: Constraints) -> Size {
        bc.constrain(Size::new(self.size, self.size))
    }

    fn paint(&self, cx: &mut PaintCx) {
        let t = cx.theme();
        let s = self.size;
        let lit = if self.strong { t.text() } else { t.text2() };
        let dim = t.faint().with_alpha(0.55);
        // The fan's point a little above the bottom; arcs ±45° about up.
        let centre = Point::new(s / 2.0, s * 0.86);
        let w = (s / 8.0).max(1.4);
        let style = StrokeStyle::new(w).with_cap(LineCap::Round);
        let dot = Circle::new(centre, w * 0.75);
        cx.canvas.fill(&dot, if self.bars >= 1 { lit } else { dim });
        for (i, k) in [0.32, 0.56, 0.80].iter().enumerate() {
            let r = s * k;
            let arc = KArc {
                center: centre,
                radii: (r, r).into(),
                start_angle: -std::f64::consts::FRAC_PI_2 - std::f64::consts::FRAC_PI_4,
                sweep_angle: std::f64::consts::FRAC_PI_2,
                x_rotation: 0.0,
            };
            let on = self.bars as usize >= i + 2;
            cx.canvas.stroke(&arc.into_path(0.1), &style, if on { lit } else { dim });
        }
    }

    fn hash(&self, h: &mut PaintHasher) {
        h.add(&(self.bars, self.strong)).f64(self.size);
    }

    fn semantics(&self) -> Option<Semantics> {
        Some(Semantics { role: SemanticRole::Image, value: Some(format!("{} of 4", self.bars)), ..Default::default() })
    }
}

/// A sound level, 0.0–1.0, as a row of segments filling from the left:
/// green, then amber near the top, red at the very top; the peak held
/// a moment as one lit segment.
pub fn level_meter<Msg>(level: f32, peak: f32, segments: usize) -> Element<Msg> {
    let w = Level { level: level.clamp(0.0, 1.0), peak: peak.clamp(0.0, 1.0), segments: segments.max(4) };
    Element::new(w, Vec::new()).width(aizome_kit::Length::Fill).label("Input level")
}

struct Level {
    level: f32,
    peak: f32,
    segments: usize,
}

impl Widget for Level {
    fn name(&self) -> &'static str {
        "level"
    }

    fn layout(&mut self, _cx: &mut LayoutCx, bc: Constraints) -> Size {
        let w = if bc.max.width.is_finite() { bc.max.width } else { 220.0 };
        bc.constrain(Size::new(w, 10.0))
    }

    fn paint(&self, cx: &mut PaintCx) {
        let t = cx.theme();
        let b = cx.bounds();
        let n = self.segments;
        let gap = 2.0;
        let seg = ((b.width() - gap * (n - 1) as f64) / n as f64).floor().max(2.0);
        let lit = (self.level * n as f32).round() as usize;
        let peak = ((self.peak * n as f32).ceil() as usize).min(n);
        let p = &t.palette;
        let green = aizome_kit::theme::color(p.green);
        let amber = aizome_kit::theme::color(p.amber);
        let red = aizome_kit::theme::color(p.red);
        for i in 0..n {
            let x = (b.x0 + i as f64 * (seg + gap)).round();
            let r = Rect::new(x, b.y0 + 1.0, x + seg, b.y1 - 1.0);
            let frac = (i + 1) as f32 / n as f32;
            let hue = if frac > 0.92 {
                red
            } else if frac > 0.75 {
                amber
            } else {
                green
            };
            let on = i < lit || (peak > 0 && i + 1 == peak);
            let fill: Color = if on { hue } else { t.sunk() };
            cx.canvas.fill_rect(r, fill);
            if !on {
                aizome_kit::draw::border(cx.canvas, r, t.line(), 1.0);
            }
        }
    }

    fn hash(&self, h: &mut PaintHasher) {
        h.f64(self.level as f64).f64(self.peak as f64).add(&self.segments);
    }

    fn semantics(&self) -> Option<Semantics> {
        Some(Semantics { role: SemanticRole::Meter, range: Some((0.0, 1.0, self.level as f64)), ..Default::default() })
    }
}

/// With `SETTEI_LIVE_SNAPSHOT=1` (and `--sample`), a pane's sample is a
/// copy of this machine's state, read once through `start`'s service and
/// let go: what the pane does changes only the copy. For looking at the
/// panes with real devices and networks without touching them.
pub fn live_snapshot<S: Clone, H>(
    start: impl FnOnce() -> H,
    ready: impl Fn(&H) -> bool,
    snapshot: impl Fn(&H) -> std::sync::Arc<S>,
) -> Option<S> {
    std::env::var_os("SETTEI_LIVE_SNAPSHOT")?;
    let service = start();
    let deadline = Instant::now() + Duration::from_secs(4);
    while !ready(&service) && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(25));
    }
    // A moment more for the rest of the objects to arrive.
    std::thread::sleep(Duration::from_millis(300));
    Some((*snapshot(&service)).clone())
}

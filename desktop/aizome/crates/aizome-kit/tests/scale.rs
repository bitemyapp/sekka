//! What a change costs in a window far larger than an app should build:
//! thousands of live widgets, none of them virtualised. A message runs
//! `update` and `view`, then the kit mounts, lays out and compares the
//! whole tree whatever changed, so this is where a cost per widget shows.
//!
//! The scenes are the ones a cross-toolkit benchmark draws: a column of
//! labelled rows, and boxes nested eight to a box, each casting a soft
//! shadow. Times are from the message to its damage drawn, at scale 2 in
//! a 1200 × 800 point window.
//!
//! `cargo test --release -p aizome-kit --test scale -- --ignored --nocapture`

use std::time::Instant;

use aizome_kit::hash::PaintHasher;
use aizome_kit::layout::{Constraints, Padding, Point, Rect, Size};
use aizome_kit::prelude::*;
use aizome_kit::sidestep_ui::kurbo::RoundedRect;
use aizome_kit::sidestep_ui::{Shadow, StrokeStyle};
use aizome_kit::testing::Harness;
use aizome_kit::widget::{LayoutCx, PaintCx, Widget};
use aizome_kit::widgets::layout::column;

const SCALE: f64 = 2.0;
const WINDOW: Size = Size::new(1200.0, 800.0);

const GROUND: Color = Color::hex(0x0b1020);
const ITEM: Color = Color::hex(0x18243a);
const CHANGED: Color = Color::hex(0x357d74);
const TEXT: Color = Color::hex(0xe8eefc);
const BORDER: Color = Color::hex(0x3b4d6b);

#[derive(Clone, Copy, PartialEq)]
enum Shape {
    /// Every row a child of the first.
    Column,
    /// Eight to a box, each a soft-shadowed card.
    Cards,
}

struct Scene {
    shape: Shape,
    /// The tick that last changed each node's label, and its fill.
    labels: Vec<u64>,
    fills: Vec<u64>,
    children: Vec<Vec<usize>>,
}

#[derive(Clone, Debug)]
enum Msg {
    Label(usize, u64),
    Fill(usize, u64),
    Labels(u64),
    Fills(u64),
}

/// A box with rounded corners, a border and a soft shadow, drawn on the
/// canvas: what the kit's own look doesn't have.
struct Card {
    fill: Color,
    padding: Padding,
}

impl Widget for Card {
    fn name(&self) -> &'static str {
        "card"
    }

    fn layout(&mut self, cx: &mut LayoutCx, bc: Constraints) -> Size {
        let p = self.padding;
        let child = cx.layout_child(0, bc.shrink(p).loosen());
        cx.place_child(0, Point::new(p.left, p.top));
        bc.constrain(Size::new(child.width + p.horizontal(), child.height + p.vertical()))
    }

    fn paint(&self, cx: &mut PaintCx) {
        let r = cx.bounds();
        cx.canvas.set_shadow(Some(Shadow {
            offset: Vec2::new(0.0, 2.0),
            blur: 4.0,
            color: Color::rgba(0.0, 0.0, 0.0, 0.35),
        }));
        cx.canvas.fill(&RoundedRect::from_rect(r, 4.0), self.fill);
        cx.canvas.set_shadow(None);
        cx.canvas.stroke(&RoundedRect::from_rect(r.inset(-0.5), 3.5), &StrokeStyle::new(1.0), BORDER);
    }

    fn hash(&self, h: &mut PaintHasher) {
        h.color(self.fill);
    }

    fn overflow(&self) -> f64 {
        6.0
    }
}

impl Scene {
    fn new(shape: Shape, nodes: usize) -> Scene {
        let mut children = vec![Vec::new(); nodes];
        if shape == Shape::Cards {
            for id in 1..nodes {
                children[(id - 1) / 8].push(id);
            }
        }
        Scene { shape, labels: vec![0; nodes], fills: vec![0; nodes], children }
    }

    fn item(&self, id: usize, children: Vec<Element<Msg>>) -> Element<Msg> {
        let fill = if self.fills[id] % 2 == 1 { CHANGED } else { ITEM };
        let label = match self.labels[id] {
            0 => format!("item {id:05}"),
            tick => format!("item {id:05} · {tick:04}"),
        };
        let label = text(label).size(12.0).color(TEXT);
        let body: Element<Msg> = if children.is_empty() {
            label.into()
        } else {
            let mut all = Vec::with_capacity(children.len() + 1);
            all.push(label.into());
            all.extend(children);
            column(all).into()
        };
        let padding = Padding { top: 0.0, right: 4.0, bottom: 0.0, left: 4.0 };
        let item: Element<Msg> = match self.shape {
            Shape::Column => container(body).padding(padding).fill(Tone::Color(fill)).into(),
            Shape::Cards => Element::new(Card { fill, padding }, vec![body]),
        };
        item.min_width(18.0).min_height(16.0)
    }

    fn nested(&self, id: usize) -> Element<Msg> {
        self.item(id, self.children[id].iter().map(|&c| self.nested(c)).collect())
    }
}

impl Program for Scene {
    type Msg = Msg;

    fn info(&self) -> AppInfo {
        AppInfo::new("org.aizome.Scale", "Scale", "Benchmark")
    }

    fn launched(&mut self, cx: &mut Cx<Msg>) {
        cx.open_window(WindowSpec::new("Scale").size(WINDOW.width, WINDOW.height));
    }

    fn update(&mut self, msg: Msg, _cx: &mut Cx<Msg>) {
        match msg {
            Msg::Label(i, tick) => self.labels[i] = tick,
            Msg::Fill(i, tick) => self.fills[i] = tick,
            Msg::Labels(tick) => self.labels.fill(tick),
            Msg::Fills(tick) => self.fills.fill(tick),
        }
    }

    fn view(&self, _w: WindowKey) -> Element<Msg> {
        let nodes = self.labels.len();
        let tree = match self.shape {
            Shape::Column => self.item(0, (1..nodes).map(|id| self.item(id, Vec::new())).collect()),
            Shape::Cards => self.nested(0),
        };
        // In a scroll, as content taller than its window is: every row keeps
        // its height, and those out of sight are laid out but not drawn.
        container(scroll(tree)).padding(8.0).fill(Tone::Color(GROUND)).width(Fill).height(Fill).into()
    }
}

/// `runs` messages, each followed by drawing its damage: the median time
/// to handle one (update, view, mount, layout, diff) and to draw it, in
/// milliseconds.
fn measure(h: &mut Harness<Scene>, w: WindowKey, runs: u64, msg: impl Fn(u64) -> Msg) -> (f64, f64) {
    let window = Rect::from_origin_size(Point::ZERO, WINDOW);
    h.take_damage(w);
    let (mut handling, mut drawing) = (Vec::new(), Vec::new());
    // The first few fill caches a steady run has.
    for tick in 1..=runs + 3 {
        let started = Instant::now();
        h.send(msg(tick));
        let handled = started.elapsed();
        let mut damage: Vec<Rect> = Vec::new();
        for r in h.take_damage(w).into_iter().map(|r| r.intersect(window)).filter(|r| r.area() > 0.0) {
            if !damage.iter().any(|d| d.contains_rect(r)) {
                damage.retain(|d| !r.contains_rect(*d));
                damage.push(r);
            }
        }
        for r in damage {
            let _ = h.render_rect(w, r, SCALE);
        }
        if tick > 3 {
            handling.push(handled.as_secs_f64() * 1e3);
            drawing.push((started.elapsed() - handled).as_secs_f64() * 1e3);
        }
    }
    let median = |mut v: Vec<f64>| {
        v.sort_by(f64::total_cmp);
        v[v.len() / 2]
    };
    (median(handling), median(drawing))
}

fn scene(shape: Shape, nodes: usize) -> (Harness<Scene>, WindowKey, f64) {
    let started = Instant::now();
    let mut h = Harness::new(Scene::new(shape, nodes));
    let w = h.window();
    let _ = h.render(w, SCALE);
    (h, w, started.elapsed().as_secs_f64() * 1e3)
}

#[test]
#[ignore = "a measurement for docs/kit.md, run in a release build"]
fn changes_in_very_large_windows() {
    aizome_kit::fonts::install();
    // For a profiler: `SCALE_ONLY="3000 rows, one label"` runs the rows
    // whose "scene, change" has that in it, `SCALE_RUNS=20` twenty times
    // as many of each.
    let only = std::env::var("SCALE_ONLY").unwrap_or_default();
    let times: u64 = std::env::var("SCALE_RUNS").ok().and_then(|v| v.parse().ok()).unwrap_or(1);
    println!("| Scene | Change | Handle ms | Draw ms |");
    println!("|---|---|---|---|");
    type Change = (&'static str, u64, Box<dyn Fn(u64) -> Msg>);
    let run = |shape: Shape, name: String, nodes: usize, changes: Vec<Change>| {
        let wanted: Vec<Change> =
            changes.into_iter().filter(|(change, ..)| format!("{name}, {change}").contains(&only)).collect();
        if wanted.is_empty() {
            return;
        }
        let (mut h, w, first) = scene(shape, nodes);
        println!("| {name} | (opened and drawn) | {first:.2} | |");
        for (change, runs, msg) in wanted {
            let (handle, draw) = measure(&mut h, w, runs * times, msg);
            println!("| {name} | {change} | {handle:.2} | {draw:.2} |");
        }
    };
    for nodes in [1_000, 3_000, 10_000] {
        run(
            Shape::Column,
            format!("{nodes} rows"),
            nodes,
            vec![
                ("one label", 60, Box::new(move |t| Msg::Label(nodes / 2, t))),
                ("one fill", 60, Box::new(move |t| Msg::Fill(nodes / 2, t))),
                ("one label in sight", 60, Box::new(|t| Msg::Label(3, t))),
                ("every fill", 30, Box::new(Msg::Fills)),
                ("every label", 12, Box::new(Msg::Labels)),
            ],
        );
    }
    for nodes in [64, 1_024, 8_192] {
        run(
            Shape::Cards,
            format!("{nodes} shadowed cards"),
            nodes,
            vec![("one fill", 30, Box::new(|t| Msg::Fill(1, t))), ("every fill", 12, Box::new(Msg::Fills))],
        );
    }
}

//! What a change costs in a window far larger than an app should build:
//! thousands of live widgets, none of them virtualised. A message runs
//! `update` and `view`, then the kit mounts, lays out and compares the
//! whole tree whatever changed, so this is where a cost per widget shows;
//! and where keeping what didn't change ([`lazy`]) shows what it saves.
//!
//! The scenes are the ones a cross-toolkit benchmark draws: a column of
//! labelled rows, and boxes nested eight to a box, each casting a soft
//! shadow. The rows come three ways: built outright at every view, each a
//! lazy element kept by what it shows, and those gathered a hundred to a
//! lazy group. Times are from the message to its damage drawn, at scale 2
//! in a 1200 × 800 point window. Each scene's changes run in turn in one
//! window, so the later ones find it as a session leaves it: its memory
//! scattered by the views before. The first change is run again at the end
//! to show what that costs, with and without the pointer in the window.
//!
//! `cargo test --release -p aizome-kit --test scale -- --ignored --nocapture`

use std::rc::Rc;
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

/// Rows to a lazy group.
const GROUP: usize = 100;

#[derive(Clone, Copy, PartialEq)]
enum Shape {
    /// Every row a child of the first, built at every view.
    Column,
    /// The same, each row a lazy element.
    LazyColumn,
    /// The same, the lazy rows in lazy groups of [`GROUP`].
    GroupedColumn,
    /// Eight to a box, each a soft-shadowed card.
    Cards,
}

/// What an item shows: which it is, and the ticks that last changed its
/// label and its fill.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Item {
    id: usize,
    label: u64,
    fill: u64,
}

struct Scene {
    shape: Shape,
    items: Vec<Item>,
    /// The rows after the first by the hundred, each hundred shared with
    /// the view: a group whose rows didn't change is the same allocation.
    groups: Vec<Rc<Vec<Item>>>,
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

/// An item of the scene: its box, its label, and the items inside it.
fn item(card: bool, it: Item, children: Vec<Element<Msg>>) -> Element<Msg> {
    let fill = if it.fill % 2 == 1 { CHANGED } else { ITEM };
    let label = match it.label {
        0 => format!("item {:05}", it.id),
        tick => format!("item {:05} · {tick:04}", it.id),
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
    let item: Element<Msg> = if card {
        Element::new(Card { fill, padding }, vec![body])
    } else {
        container(body).padding(padding).fill(Tone::Color(fill)).into()
    };
    item.min_width(18.0).min_height(16.0)
}

/// A row, from what it shows: what a lazy row builds.
fn row(it: &Item) -> Element<Msg> {
    item(false, *it, Vec::new())
}

/// A hundred rows, each lazy: what a lazy group builds.
fn group(rows: &Rc<Vec<Item>>) -> Element<Msg> {
    column(rows.iter().map(|it| lazy(*it, row)).collect()).into()
}

impl Scene {
    fn new(shape: Shape, nodes: usize) -> Scene {
        let mut children = vec![Vec::new(); nodes];
        if shape == Shape::Cards {
            for id in 1..nodes {
                children[(id - 1) / 8].push(id);
            }
        }
        let items: Vec<Item> = (0..nodes).map(|id| Item { id, label: 0, fill: 0 }).collect();
        let groups = match shape {
            Shape::GroupedColumn => items[1..].chunks(GROUP).map(|rows| Rc::new(rows.to_vec())).collect(),
            _ => Vec::new(),
        };
        Scene { shape, items, groups, children }
    }

    /// Change item `id`, and its group's copy of it.
    fn change(&mut self, id: usize, to: impl Fn(&mut Item)) {
        to(&mut self.items[id]);
        if id > 0
            && let Some(rows) = self.groups.get_mut((id - 1) / GROUP)
        {
            to(&mut Rc::make_mut(rows)[(id - 1) % GROUP]);
        }
    }

    fn nested(&self, id: usize) -> Element<Msg> {
        item(true, self.items[id], self.children[id].iter().map(|&c| self.nested(c)).collect())
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
            Msg::Label(i, tick) => self.change(i, |it| it.label = tick),
            Msg::Fill(i, tick) => self.change(i, |it| it.fill = tick),
            Msg::Labels(tick) => (0..self.items.len()).for_each(|i| self.change(i, |it| it.label = tick)),
            Msg::Fills(tick) => (0..self.items.len()).for_each(|i| self.change(i, |it| it.fill = tick)),
        }
    }

    fn view(&self, _w: WindowKey) -> Element<Msg> {
        let (first, rows) = (self.items[0], &self.items[1..]);
        let tree = match self.shape {
            Shape::Column => item(false, first, rows.iter().map(|it| item(false, *it, Vec::new())).collect()),
            Shape::LazyColumn => item(false, first, rows.iter().map(|it| lazy(*it, row)).collect()),
            Shape::GroupedColumn => item(false, first, self.groups.iter().map(|g| lazy(g.clone(), group)).collect()),
            Shape::Cards => self.nested(0),
        };
        // In a scroll, as content taller than its window is: every row keeps
        // its height, and those out of sight are laid out but not drawn.
        container(scroll(tree)).padding(8.0).fill(Tone::Color(GROUND)).width(Fill).height(Fill).into()
    }
}

/// One change to a scene: the `tick`th of its kind.
type Act = dyn Fn(&mut Harness<Scene>, WindowKey, u64);

/// A message to the program.
fn send(msg: impl Fn(u64) -> Msg + 'static) -> Box<Act> {
    Box::new(move |h, _, tick| h.send(msg(tick)))
}

/// The wheel turned a notch over the rows, down and back up by turns: no
/// message and no view, the tree laid out again where it scrolled.
fn notch(h: &mut Harness<Scene>, w: WindowKey, tick: u64) {
    let down = (tick / 20).is_multiple_of(2);
    h.scroll(w, Point::new(WINDOW.width / 2.0, WINDOW.height / 2.0), Vec2::new(0.0, if down { 40.0 } else { -40.0 }));
}

/// `runs` changes, each followed by drawing its damage: the median time
/// to handle one (update, view, mount, layout, diff) and to draw it, in
/// milliseconds.
fn measure(h: &mut Harness<Scene>, w: WindowKey, runs: u64, act: &Act) -> (f64, f64) {
    let window = Rect::from_origin_size(Point::ZERO, WINDOW);
    h.take_damage(w);
    let (mut handling, mut drawing) = (Vec::new(), Vec::new());
    // The first few fill caches a steady run has.
    for tick in 1..=runs + 3 {
        let started = Instant::now();
        act(h, w, tick);
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
    type Change = (&'static str, u64, Box<Act>);
    let run = |shape: Shape, name: String, nodes: usize, changes: Vec<Change>| {
        let wanted: Vec<Change> =
            changes.into_iter().filter(|(change, ..)| format!("{name}, {change}").contains(&only)).collect();
        if wanted.is_empty() {
            return;
        }
        let (mut h, w, first) = scene(shape, nodes);
        println!("| {name} | (opened and drawn) | {first:.2} | |");
        for (change, runs, act) in wanted {
            let (handle, draw) = measure(&mut h, w, runs * times, &act);
            println!("| {name} | {change} | {handle:.2} | {draw:.2} |");
        }
    };
    for (shape, kind) in
        [(Shape::Column, "rows"), (Shape::LazyColumn, "lazy rows"), (Shape::GroupedColumn, "lazy rows in hundreds")]
    {
        for nodes in [1_000, 3_000, 10_000] {
            run(
                shape,
                format!("{nodes} {kind}"),
                nodes,
                vec![
                    ("one label", 60, send(move |t| Msg::Label(nodes / 2, t))),
                    ("one fill", 60, send(move |t| Msg::Fill(nodes / 2, t))),
                    ("one label in sight", 60, send(|t| Msg::Label(3, t))),
                    ("every fill", 30, send(Msg::Fills)),
                    ("every label", 12, send(Msg::Labels)),
                    ("scrolled a notch", 60, Box::new(notch)),
                    // The first change again, now that the window has
                    // lived a while: with the pointer out of it, and with
                    // it over the rows, where every build looks for what's
                    // under it.
                    (
                        "one label, later",
                        60,
                        Box::new(move |h: &mut Harness<Scene>, w: WindowKey, t: u64| {
                            if t == 1 {
                                h.leave(w);
                            }
                            h.send(Msg::Label(nodes / 2, t));
                        }),
                    ),
                    (
                        "one label, later, the pointer over the rows",
                        60,
                        Box::new(move |h: &mut Harness<Scene>, w: WindowKey, t: u64| {
                            if t == 1 {
                                h.move_to(w, Point::new(WINDOW.width / 2.0, WINDOW.height / 2.0));
                            }
                            h.send(Msg::Label(nodes / 2, t));
                        }),
                    ),
                ],
            );
        }
    }
    for nodes in [64, 1_024, 8_192] {
        run(
            Shape::Cards,
            format!("{nodes} shadowed cards"),
            nodes,
            vec![("one fill", 30, send(|t| Msg::Fill(1, t))), ("every fill", 12, send(Msg::Fills))],
        );
    }
}

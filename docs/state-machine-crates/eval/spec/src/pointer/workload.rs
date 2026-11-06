//! A synthetic but realistic pointer stream for benchmarks: 1 kHz motion
//! over a window with a toolbar, a list with a drags-only column edge over
//! its rows, a slider that captures, and file icons that start compositor
//! drags; with clicks, row drags, edge drags and edge clicks, leaves and
//! re-entries, and rebuilds.

use super::{Button, DragFrom, Event, Hit, Node, Point};
use crate::SplitMix;

pub const WIDTH: i32 = 1000;
pub const HEIGHT: i32 = 700;

/// The window's widgets, hit-tested topmost first.
#[derive(Clone, Copy, Debug, Default)]
pub struct Layout;

impl Layout {
    /// Toolbar buttons 1..=10 at the top.
    fn button(p: Point) -> Option<u16> {
        if !(4..36).contains(&p.y) || p.x < 10 {
            return None;
        }
        let i = (p.x - 10) / 95;
        (i < 10 && (p.x - 10) % 95 < 90).then_some(1 + i as u16)
    }
    /// List rows 100.. in x 0..600, y 60..660, 24 px each.
    fn row(p: Point) -> Option<u16> {
        ((0..600).contains(&p.x) && (60..660).contains(&p.y)).then(|| 100 + ((p.y - 60) / 24) as u16)
    }
    /// The column edge (drags-only) at x 296..304 over the rows.
    fn edge(p: Point) -> bool {
        (296..304).contains(&p.x) && (60..660).contains(&p.y)
    }
    /// Node 60: the slider.
    fn slider(p: Point) -> bool {
        (650..950).contains(&p.x) && (100..130).contains(&p.y)
    }
    /// File icons 200.. in x 650..950, y 200..600 (80 px cells, 72 px icons).
    fn icon(p: Point) -> Option<u16> {
        if !(650..970).contains(&p.x) || !(200..600).contains(&p.y) {
            return None;
        }
        let (cx, cy) = ((p.x - 650) / 80, (p.y - 200) / 80);
        ((p.x - 650) % 80 < 72 && (p.y - 200) % 80 < 72).then(|| 200 + (cy * 4 + cx) as u16)
    }

    pub fn hit(&self, p: Point) -> Hit {
        if Self::edge(p) {
            return Hit { node: Some(Node(50)), drags_only: true, under: Self::row(p).map(Node) };
        }
        if let Some(b) = Self::button(p) {
            return Hit::on(b);
        }
        if let Some(r) = Self::row(p) {
            return Hit::on(r);
        }
        if Self::slider(p) {
            return Hit::on(60);
        }
        if let Some(i) = Self::icon(p) {
            return Hit::on(i);
        }
        Hit::NOTHING
    }
}

struct Gen {
    rng: SplitMix,
    at: Point,
    inside: bool,
    out: Vec<Event>,
    layout: Layout,
}

impl Gen {
    fn hit(&self) -> Hit {
        self.layout.hit(self.at)
    }
    fn mv(&mut self) {
        self.inside = true;
        let hit = self.hit();
        self.out.push(Event::Move { at: self.at, hit });
    }
    fn walk_to(&mut self, to: Point, steps: u32) {
        let from = self.at;
        for i in 1..=steps.max(1) {
            let t = i as i64;
            let n = steps.max(1) as i64;
            self.at = Point::new(
                from.x + ((to.x - from.x) as i64 * t / n) as i32,
                from.y + ((to.y - from.y) as i64 * t / n) as i32,
            );
            self.mv();
        }
    }
    fn wander(&mut self, steps: u32) {
        for _ in 0..steps {
            let dx = self.rng.below(17) as i32 - 8;
            let dy = self.rng.below(17) as i32 - 8;
            self.at = Point::new((self.at.x + dx).clamp(0, WIDTH - 1), (self.at.y + dy).clamp(0, HEIGHT - 1));
            self.mv();
        }
    }
    fn press(&mut self, button: Button) {
        let hit = self.hit();
        self.out.push(Event::Press { at: self.at, button, clicks: 1, hit });
    }
    fn release(&mut self, button: Button) {
        let hit = self.hit();
        self.out.push(Event::Release { at: self.at, button, hit });
    }
    fn point_in(&mut self, x: (i32, i32), y: (i32, i32)) -> Point {
        Point::new(x.0 + self.rng.below((x.1 - x.0) as u64) as i32, y.0 + self.rng.below((y.1 - y.0) as u64) as i32)
    }
    fn scenario(&mut self) {
        if !self.inside {
            // Re-enter from an edge.
            self.at = Point::new(0, self.rng.below(HEIGHT as u64) as i32);
            self.mv();
        }
        match self.rng.below(100) {
            0..60 => {
                let n = 50 + self.rng.below(250) as u32;
                self.wander(n);
            }
            60..75 => {
                // A click on a toolbar button or a row.
                let to = if self.rng.chance(500) {
                    self.point_in((10, 940), (6, 34))
                } else {
                    self.point_in((0, 290), (60, 660))
                };
                let steps = 10 + self.rng.below(30) as u32;
                self.walk_to(to, steps);
                self.press(Button::Left);
                let jitter = self.rng.below(3) as u32;
                self.wander(jitter);
                self.release(Button::Left);
            }
            75..83 => {
                // A row drag (selection).
                let to = self.point_in((310, 590), (60, 640));
                self.walk_to(to, 20);
                self.press(Button::Left);
                let n = 50 + self.rng.below(100) as u32;
                self.wander(n);
                self.release(Button::Left);
            }
            83..88 => {
                // The column edge: a drag, or a click that reaches the row under it.
                let to = self.point_in((297, 303), (70, 640));
                self.walk_to(to, 20);
                self.press(Button::Left);
                if self.rng.chance(600) {
                    let n = 30 + self.rng.below(70) as u32;
                    let dest = Point::new(self.at.x + 20 + self.rng.below(100) as i32, self.at.y);
                    self.walk_to(dest, n);
                }
                self.release(Button::Left);
            }
            88..92 => {
                // The slider captures the pointer for its drag.
                let to = self.point_in((660, 940), (102, 128));
                self.walk_to(to, 20);
                self.press(Button::Left);
                self.out.push(Event::Capture(Node(60)));
                let n = 50 + self.rng.below(150) as u32;
                self.wander(n);
                self.release(Button::Left);
            }
            92..95 => {
                // A file icon dragged out: the program starts a compositor drag.
                let to = self.point_in((660, 940), (210, 590));
                self.walk_to(to, 20);
                self.press(Button::Left);
                self.wander(3);
                self.out.push(Event::StartDrag(if self.rng.chance(500) {
                    DragFrom::Program
                } else {
                    DragFrom::Widget
                }));
                self.out.push(Event::Leave);
                self.inside = false;
                self.out.push(Event::DragEnded);
            }
            95..98 => {
                self.out.push(Event::Leave);
                self.inside = false;
            }
            _ => {
                // A rebuild drops whatever is hovered.
                if let Some(n) = self.hit().node {
                    self.out.push(Event::Removed(n));
                }
            }
        }
    }
}

/// About `events` events of pointer input (deterministic for a seed).
pub fn stream(seed: u64, events: usize) -> Vec<Event> {
    let mut g = Gen {
        rng: SplitMix(seed),
        at: Point::new(500, 350),
        inside: false,
        out: Vec::with_capacity(events + 400),
        layout: Layout,
    };
    while g.out.len() < events {
        g.scenario();
    }
    g.out.truncate(events);
    g.out
}

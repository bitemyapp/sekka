//! Random pointer input for the differential property tests: a small node
//! domain (1..=6, edge 9) and a small plane so that hits, the drag
//! threshold, captures and removals collide often.

use super::{Button, DragFrom, Event, Hit, Node, Point};
use crate::testing::Tape;

fn hit(t: &mut Tape) -> Hit {
    match t.below(20) {
        0..3 => Hit::NOTHING,
        3..15 => Hit::on(1 + t.below(6) as u16),
        _ => Hit::edge(9, t.below(7) as u16),
    }
}

fn point(t: &mut Tape) -> Point {
    Point::new(t.below(12) as i32, t.below(12) as i32)
}

fn button(t: &mut Tape) -> Button {
    match t.below(6) {
        0..4 => Button::Left,
        4 => Button::Right,
        _ => Button::Middle,
    }
}

fn node(t: &mut Tape) -> Node {
    if t.chance(150) { Node(9) } else { Node(1 + t.below(6) as u16) }
}

pub fn event(t: &mut Tape) -> Event {
    match t.below(100) {
        0..40 => Event::Move { at: point(t), hit: hit(t) },
        40..55 => Event::Press { at: point(t), button: button(t), clicks: 1 + t.below(2) as u8, hit: hit(t) },
        55..70 => Event::Release { at: point(t), button: button(t), hit: hit(t) },
        70..76 => Event::Leave,
        76..80 => Event::Capture(node(t)),
        80..84 => Event::Uncapture(node(t)),
        84..88 => Event::StartDrag(if t.chance(500) { DragFrom::Widget } else { DragFrom::Program }),
        88..93 => Event::DragEnded,
        _ => Event::Removed(node(t)),
    }
}

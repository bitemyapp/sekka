//! Random terminal output for the differential property tests.

use super::{Config, Event};
use crate::testing::Tape;

pub fn event(c: &Config, t: &mut Tape) -> Event {
    match t.below(100) {
        0..45 => Event::Print { wide: false },
        45..55 => Event::Print { wide: true },
        55..59 => Event::Repeat(t.below(4) as u16),
        59..64 => Event::CarriageReturn,
        64..69 => Event::LineFeed,
        69..73 => Event::Backspace,
        73..77 => Event::Tab,
        77..81 => Event::CursorTo { row: t.below(c.rows as u32 + 2) as u16, col: t.below(c.cols as u32 + 2) as u16 },
        81..85 => Event::ReverseIndex,
        85..89 => Event::EraseLine(t.below(4) as u8),
        89..93 => Event::SetAutowrap(t.chance(600)),
        93..96 => Event::SaveCursor,
        96..99 => Event::RestoreCursor,
        _ => Event::FullReset,
    }
}

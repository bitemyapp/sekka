//! Terminal output for benchmarks: mostly printed cells, with the controls
//! and modes real output mixes in. Three kinds of stretch alternate:
//!
//! - shell output: lines of 0–300 cells (so many wrap at 80 and some at
//!   200 columns), CR LF, a few wide scalars and tabs;
//! - progress lines: CR and a rewrite of the same line, EL 0;
//! - a full-screen program: CUP, EL, text with autowrap off (DECAWM),
//!   saved cursors, RI, REP.

use super::Event;
use crate::SplitMix;

struct Gen {
    rng: SplitMix,
    out: Vec<Event>,
    cells: usize,
    cols: u16,
    rows: u16,
}

impl Gen {
    fn print(&mut self, wide: bool) {
        self.out.push(Event::Print { wide });
        self.cells += 1;
    }
    fn text(&mut self, n: usize) {
        for _ in 0..n {
            let wide = self.rng.chance(15);
            if self.rng.chance(8) {
                self.out.push(Event::Tab);
            }
            self.print(wide);
        }
    }
    fn shell(&mut self) {
        for _ in 0..(5 + self.rng.below(60)) {
            let len = match self.rng.below(10) {
                0..6 => self.rng.below(70),
                6..9 => 70 + self.rng.below(150),
                _ => 220 + self.rng.below(80),
            } as usize;
            self.text(len);
            if self.rng.chance(20) {
                self.out.push(Event::Backspace);
                self.print(false);
            }
            self.out.push(Event::CarriageReturn);
            self.out.push(Event::LineFeed);
        }
    }
    fn progress(&mut self) {
        let width = (self.cols as usize).saturating_sub(1).min(60 + self.rng.below(40) as usize);
        for _ in 0..(10 + self.rng.below(40)) {
            self.out.push(Event::CarriageReturn);
            self.text(width);
            self.out.push(Event::EraseLine(0));
        }
        self.out.push(Event::CarriageReturn);
        self.out.push(Event::LineFeed);
    }
    fn full_screen(&mut self) {
        self.out.push(Event::SaveCursor);
        self.out.push(Event::SetAutowrap(false));
        for _ in 0..(3 + self.rng.below(10)) {
            for r in 0..self.rows {
                self.out.push(Event::CursorTo { row: r, col: 0 });
                self.out.push(Event::EraseLine(2));
                let n = self.rng.below(self.cols as u64 + 10) as usize;
                self.text(n);
                if self.rng.chance(50) {
                    self.out.push(Event::Repeat(1 + self.rng.below(20) as u16));
                    self.cells += 10;
                }
            }
            if self.rng.chance(300) {
                self.out.push(Event::CursorTo { row: 0, col: 0 });
                self.out.push(Event::ReverseIndex);
            }
        }
        self.out.push(Event::SetAutowrap(true));
        self.out.push(Event::RestoreCursor);
        self.out.push(Event::CursorTo { row: self.rows - 1, col: 0 });
        self.out.push(Event::LineFeed);
    }
}

/// About `cells` printed cells of output for a `cols`×`rows` screen
/// (deterministic for a seed).
pub fn stream(seed: u64, cols: u16, rows: u16, cells: usize) -> Vec<Event> {
    let mut g = Gen { rng: SplitMix(seed), out: Vec::with_capacity(cells + cells / 8), cells: 0, cols, rows };
    while g.cells < cells {
        match g.rng.below(10) {
            0..7 => g.shell(),
            7..9 => g.progress(),
            _ => g.full_screen(),
        }
    }
    g.out
}

//! Conformance traces for `wrap`, on small screens. They follow Hotaru's
//! `Screen` behaviour at `8e0acbe5` (there is no confirmed bug here).

use super::{Config, Effect as F, Event as E, StateName as S};
use crate::testing::{Driven, Tape, Target, Trace};

/// The `wrap` target.
pub struct Spec;

impl<M: super::Machine> Driven<Spec> for M {
    fn boot(config: Config) -> Self {
        M::new(config)
    }
    fn step(&mut self, event: E, out: &mut Vec<F>) {
        self.handle(event, out);
    }
    fn observe(&self) -> S {
        self.state()
    }
}

impl Target for Spec {
    const NAME: &'static str = "wrap";
    type Config = Config;
    type Event = E;
    type Effect = F;
    type State = S;
    type Env = Config;

    fn traces() -> Vec<Trace<Self>> {
        traces()
    }
    fn configs() -> Vec<Config> {
        vec![
            Config { cols: 2, rows: 1 },
            Config { cols: 3, rows: 2 },
            Config { cols: 5, rows: 3 },
            Config { cols: 10, rows: 4 },
            Config { cols: 17, rows: 128 },
        ]
    }
    fn env(config: &Config) -> Config {
        *config
    }
    fn next(config: &mut Config, tape: &mut Tape, _: S, _: &[F]) -> E {
        super::env::event(config, tape)
    }
}

const N: E = E::Print { wide: false };
const W: E = E::Print { wide: true };

fn put(row: u16, col: u16) -> F {
    F::Put { row, col, wide: false }
}
fn put_w(row: u16, col: u16) -> F {
    F::Put { row, col, wide: true }
}

/// Narrow prints at `row`, columns `from..to`; the last one leaves the
/// state `last` (the others `Normal`).
fn fill(mut t: Trace<Spec>, row: u16, from: u16, to: u16, mid: S, last: S) -> Trace<Spec> {
    for c in from..to {
        t = t.step(N, [put(row, c)], if c + 1 == to { last } else { mid });
    }
    t
}

pub fn traces() -> Vec<Trace<Spec>> {
    let c5 = Config { cols: 5, rows: 3 };
    let c3 = Config { cols: 3, rows: 2 };
    let c10 = Config { cols: 10, rows: 3 };
    let mut v = Vec::new();

    let t = Trace::new(
        "print-fills-the-line-then-pends",
        "the last column sets the pending wrap; the next print wraps",
        c5,
    );
    let t = fill(t, 0, 0, 5, S::Normal, S::PendingWrap);
    v.push(t.step(N, [F::SoftWrap { row: 0 }, put(1, 0)], S::Normal).step(N, [put(1, 1)], S::Normal));

    let t = Trace::new("wrap-at-the-bottom-scrolls", "a wrap on the last row scrolls; links move with their rows", c3)
        .step(E::CursorTo { row: 1, col: 0 }, [], S::Normal);
    let t = fill(t, 1, 0, 3, S::Normal, S::PendingWrap);
    v.push(
        t.step(N, [F::SoftWrap { row: 1 }, F::ScrollUp, put(1, 0)], S::Normal)
            .step(E::EraseLine(2), [], S::Normal)
            .step(E::CursorTo { row: 0, col: 0 }, [], S::Normal)
            .step(E::EraseLine(0), [F::Unlink { row: 0 }], S::Normal)
            .step(E::EraseLine(0), [], S::Normal),
    );

    let t = Trace::new("cr-lf-bs-clear-pending", "CR, BS and LF each clear the pending wrap", c5);
    let t = fill(t, 0, 0, 5, S::Normal, S::PendingWrap).step(E::CarriageReturn, [], S::Normal);
    let t = fill(t, 0, 0, 5, S::Normal, S::PendingWrap)
        .step(E::Backspace, [], S::Normal)
        .step(N, [put(0, 3)], S::Normal)
        .step(N, [put(0, 4)], S::PendingWrap)
        .step(E::LineFeed, [], S::Normal)
        .step(N, [put(1, 4)], S::PendingWrap)
        .step(E::Backspace, [], S::Normal)
        .step(E::Backspace, [], S::Normal)
        .step(N, [put(1, 2)], S::Normal);
    v.push(t);

    let t =
        Trace::new("tab-keeps-pending", "HT moves to the next stop or the last column and keeps a pending wrap", c10);
    let t = fill(t, 0, 0, 10, S::Normal, S::PendingWrap)
        .step(E::Tab, [], S::PendingWrap)
        .step(N, [F::SoftWrap { row: 0 }, put(1, 0)], S::Normal)
        .step(E::Tab, [], S::Normal)
        .step(N, [put(1, 8)], S::Normal)
        .step(E::Tab, [], S::Normal)
        .step(E::Tab, [], S::Normal)
        .step(N, [put(1, 9)], S::PendingWrap);
    v.push(t);

    let t = Trace::new(
        "autowrap-off-overwrites-the-last-column",
        "without DECAWM the pending flag still sets, and the next print overwrites the last column",
        c5,
    )
    .step(E::SetAutowrap(false), [], S::NormalNoAutowrap);
    let t = fill(t, 0, 0, 5, S::NormalNoAutowrap, S::PendingNoAutowrap)
        .step(N, [put(0, 4)], S::PendingNoAutowrap)
        .step(E::SetAutowrap(true), [], S::PendingWrap)
        .step(N, [F::SoftWrap { row: 0 }, put(1, 0)], S::Normal)
        .step(E::SetAutowrap(false), [], S::NormalNoAutowrap)
        .step(E::SetAutowrap(false), [], S::NormalNoAutowrap);
    v.push(t);

    v.push(
        Trace::new("wide-scalar-at-the-edge", "a wide scalar that doesn't fit leaves a spacer head and wraps", c5)
            .step(E::CursorTo { row: 0, col: 4 }, [], S::Normal)
            .step(W, [F::SpacerHead { row: 0, col: 4 }, F::SoftWrap { row: 0 }, put_w(1, 0)], S::Normal)
            .step(W, [put_w(1, 2)], S::Normal)
            .step(W, [F::SpacerHead { row: 1, col: 4 }, F::SoftWrap { row: 1 }, put_w(2, 0)], S::Normal)
            .step(E::CursorTo { row: 2, col: 3 }, [], S::Normal)
            .step(W, [put_w(2, 3)], S::PendingWrap)
            .step(W, [F::SoftWrap { row: 2 }, F::ScrollUp, put_w(2, 0)], S::Normal),
    );

    v.push(
        Trace::new("wide-scalar-dropped-without-autowrap", "no room and no autowrap: the wide scalar is dropped", c5)
            .step(E::SetAutowrap(false), [], S::NormalNoAutowrap)
            .step(E::CursorTo { row: 0, col: 4 }, [], S::NormalNoAutowrap)
            .step(W, [], S::NormalNoAutowrap)
            .step(N, [put(0, 4)], S::PendingNoAutowrap)
            .step(W, [], S::NormalNoAutowrap)
            .step(E::Repeat(1), [], S::NormalNoAutowrap),
    );

    let t =
        Trace::new("save-restore-carries-pending", "DECSC/DECRC save and restore the pending wrap with the cursor", c5);
    let t = fill(t, 0, 0, 5, S::Normal, S::PendingWrap)
        .step(E::SaveCursor, [], S::PendingWrap)
        .step(E::CarriageReturn, [], S::Normal)
        .step(N, [put(0, 0)], S::Normal)
        .step(E::RestoreCursor, [], S::PendingWrap)
        .step(N, [F::SoftWrap { row: 0 }, put(1, 0)], S::Normal)
        .step(E::RestoreCursor, [], S::PendingWrap)
        .step(E::SetAutowrap(false), [], S::PendingNoAutowrap)
        .step(E::RestoreCursor, [], S::PendingNoAutowrap)
        .step(E::FullReset, [], S::Normal)
        .step(E::RestoreCursor, [], S::Normal)
        .step(N, [put(0, 0)], S::Normal);
    v.push(t);

    v.push(
        Trace::new(
            "repeat-prints-the-last-scalar",
            "REP repeats the last printed scalar across controls, until RIS",
            c5,
        )
        .step(E::Repeat(3), [], S::Normal)
        .step(W, [put_w(0, 0)], S::Normal)
        .step(
            E::Repeat(2),
            [put_w(0, 2), F::SpacerHead { row: 0, col: 4 }, F::SoftWrap { row: 0 }, put_w(1, 0)],
            S::Normal,
        )
        .step(E::CarriageReturn, [], S::Normal)
        .step(E::LineFeed, [], S::Normal)
        .step(E::Repeat(0), [put_w(2, 0)], S::Normal)
        .step(N, [put(2, 2)], S::Normal)
        .step(E::Repeat(2), [put(2, 3), put(2, 4)], S::PendingWrap)
        .step(E::FullReset, [], S::Normal)
        .step(E::Repeat(1), [], S::Normal),
    );

    let t =
        Trace::new("reverse-index", "RI at the top scrolls down and keeps a pending wrap; elsewhere it clears it", c3);
    let t = fill(t, 0, 0, 3, S::Normal, S::PendingWrap)
        .step(E::ReverseIndex, [F::ScrollDown], S::PendingWrap)
        .step(N, [F::SoftWrap { row: 0 }, put(1, 0)], S::Normal)
        .step(N, [put(1, 1)], S::Normal)
        .step(N, [put(1, 2)], S::PendingWrap)
        .step(E::ReverseIndex, [], S::Normal)
        .step(E::ReverseIndex, [F::ScrollDown], S::Normal)
        .step(E::CursorTo { row: 1, col: 0 }, [], S::Normal)
        .step(E::EraseLine(0), [F::Unlink { row: 1 }], S::Normal);
    v.push(t);

    let t = Trace::new(
        "erase-line-modes",
        "EL 0 and 2 unlink a wrapped row; EL 1 doesn't; unknown modes change nothing",
        c3,
    );
    let t = fill(t, 0, 0, 3, S::Normal, S::PendingWrap)
        .step(N, [F::SoftWrap { row: 0 }, put(1, 0)], S::Normal)
        .step(E::CursorTo { row: 0, col: 2 }, [], S::Normal)
        .step(N, [put(0, 2)], S::PendingWrap)
        .step(E::EraseLine(3), [], S::PendingWrap)
        .step(E::EraseLine(1), [], S::Normal)
        .step(N, [put(0, 2)], S::PendingWrap)
        .step(E::EraseLine(2), [F::Unlink { row: 0 }], S::Normal);
    v.push(t);

    let t = Trace::new("linefeed-at-the-bottom", "LF scrolls at the bottom; the wrapped row's link leaves with it", c3);
    let t = fill(t, 0, 0, 3, S::Normal, S::PendingWrap)
        .step(N, [F::SoftWrap { row: 0 }, put(1, 0)], S::Normal)
        .step(E::LineFeed, [F::ScrollUp], S::Normal)
        .step(E::EraseLine(0), [], S::Normal)
        .step(E::CursorTo { row: 0, col: 0 }, [], S::Normal)
        .step(E::EraseLine(0), [], S::Normal);
    v.push(t);

    v.push(
        Trace::new("cursor-moves-clamp-and-clear", "CUP clamps to the screen and clears the pending wrap", c5)
            .step(E::CursorTo { row: 99, col: 99 }, [], S::Normal)
            .step(N, [put(2, 4)], S::PendingWrap)
            .step(E::CursorTo { row: 2, col: 4 }, [], S::Normal)
            .step(N, [put(2, 4)], S::PendingWrap)
            .step(N, [F::SoftWrap { row: 2 }, F::ScrollUp, put(2, 0)], S::Normal)
            .step(E::CursorTo { row: 1, col: 0 }, [], S::Normal)
            .step(E::EraseLine(2), [F::Unlink { row: 1 }], S::Normal),
    );

    v.push(
        Trace::new(
            "two-columns",
            "the narrowest screen: a wide scalar at column 0 fills the row",
            Config { cols: 2, rows: 1 },
        )
        .step(W, [put_w(0, 0)], S::PendingWrap)
        .step(W, [F::SoftWrap { row: 0 }, F::ScrollUp, put_w(0, 0)], S::PendingWrap)
        .step(E::Backspace, [], S::Normal)
        .step(W, [put_w(0, 0)], S::PendingWrap)
        .step(E::CursorTo { row: 0, col: 1 }, [], S::Normal)
        .step(
            W,
            [F::SpacerHead { row: 0, col: 1 }, F::SoftWrap { row: 0 }, F::ScrollUp, put_w(0, 0)],
            S::PendingWrap,
        ),
    );
    v
}

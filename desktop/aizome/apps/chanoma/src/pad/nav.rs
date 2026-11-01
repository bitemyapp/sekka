//! Moving around menus with a controller: the pad of arrows or the left
//! stick gives a direction, and a direction held repeats, slowly at first
//! and then faster, as a remote control's arrows do.
//!
//! The stick has some hysteresis (it must lean well over to move, and come
//! well back to stop), so a stick resting off centre doesn't wander.

use std::time::{Duration, Instant};

/// A direction.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Dir {
    Up,
    Down,
    Left,
    Right,
}

/// How far the stick leans before it moves, and how far back it comes to
/// stop.
const LEAN: f32 = 0.55;
const RELAX: f32 = 0.35;
/// The first repeat, then the pace, and the pace once it has gone on.
pub const FIRST: Duration = Duration::from_millis(420);
pub const PACE: Duration = Duration::from_millis(130);
pub const FAST: Duration = Duration::from_millis(75);
/// Repeats before the pace quickens.
const QUICKEN_AFTER: u32 = 6;

/// One controller's direction and its repeats.
#[derive(Clone, Debug, Default)]
pub struct Repeater {
    stick: Option<Dir>,
    held: Option<Dir>,
    next: Option<Instant>,
    repeats: u32,
}

impl Repeater {
    /// What the stick says, with hysteresis: `x` right and `y` down, −1 to 1.
    fn stick_dir(&mut self, x: f32, y: f32) -> Option<Dir> {
        let (ax, ay) = (x.abs(), y.abs());
        let along = |d: Dir| match d {
            Dir::Left => -x,
            Dir::Right => x,
            Dir::Up => -y,
            Dir::Down => y,
        };
        if let Some(d) = self.stick
            && along(d) > RELAX
        {
            return Some(d);
        }
        self.stick = if ax.max(ay) < LEAN {
            None
        } else if ax > ay {
            Some(if x > 0.0 { Dir::Right } else { Dir::Left })
        } else {
            Some(if y > 0.0 { Dir::Down } else { Dir::Up })
        };
        self.stick
    }

    /// The controller's arrows (`pad`, the one pressed last wins where
    /// several are) and stick, at `now`: a direction to go now, if one
    /// starts.
    pub fn update(&mut self, pad: Option<Dir>, x: f32, y: f32, now: Instant) -> Option<Dir> {
        let stick = self.stick_dir(x, y);
        let want = pad.or(stick);
        if want == self.held {
            return None;
        }
        self.held = want;
        self.repeats = 0;
        self.next = want.map(|_| now + FIRST);
        want
    }

    /// The repeat due by `now`, if one is.
    pub fn tick(&mut self, now: Instant) -> Option<Dir> {
        let (Some(d), Some(at)) = (self.held, self.next) else { return None };
        if now < at {
            return None;
        }
        self.repeats += 1;
        let pace = if self.repeats > QUICKEN_AFTER { FAST } else { PACE };
        // From when it was due, not now, so a late wake doesn't slow it;
        // but never more than one behind.
        self.next = Some((at + pace).max(now));
        Some(d)
    }

    /// When the next repeat is due.
    pub fn deadline(&self) -> Option<Instant> {
        self.next
    }

    /// Let go (the controller went, or the screen changed under it).
    pub fn release(&mut self) {
        *self = Repeater::default();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_held_arrow_repeats_and_quickens() {
        let t = Instant::now();
        let mut r = Repeater::default();
        assert_eq!(r.update(Some(Dir::Down), 0.0, 0.0, t), Some(Dir::Down));
        assert_eq!(r.update(Some(Dir::Down), 0.0, 0.0, t + Duration::from_millis(100)), None);
        assert_eq!(r.tick(t + Duration::from_millis(400)), None);
        assert_eq!(r.tick(t + FIRST), Some(Dir::Down));
        let mut at = t + FIRST;
        for _ in 0..QUICKEN_AFTER - 1 {
            at += PACE;
            assert_eq!(r.tick(at), Some(Dir::Down));
        }
        at += PACE;
        assert_eq!(r.tick(at), Some(Dir::Down));
        assert_eq!(r.deadline(), Some(at + FAST));
        assert_eq!(r.update(None, 0.0, 0.0, at), None);
        assert_eq!(r.tick(at + Duration::from_secs(1)), None);
    }

    #[test]
    fn the_stick_needs_a_lean_and_holds_until_it_relaxes() {
        let t = Instant::now();
        let mut r = Repeater::default();
        assert_eq!(r.update(None, 0.4, 0.0, t), None, "resting off centre");
        assert_eq!(r.update(None, 0.7, 0.2, t), Some(Dir::Right));
        assert_eq!(r.update(None, 0.45, 0.4, t), None, "still right: above where it lets go");
        assert_eq!(r.update(None, 0.2, 0.1, t), None);
        assert_eq!(r.tick(t + Duration::from_secs(2)), None, "let go");
        assert_eq!(r.update(None, 0.1, -0.9, t), Some(Dir::Up));
    }

    #[test]
    fn the_arrows_win_over_the_stick() {
        let t = Instant::now();
        let mut r = Repeater::default();
        assert_eq!(r.update(Some(Dir::Left), 0.9, 0.0, t), Some(Dir::Left));
        assert_eq!(r.update(None, 0.9, 0.0, t), Some(Dir::Right));
    }
}

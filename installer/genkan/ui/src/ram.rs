// SPDX-License-Identifier: GPL-3.0-or-later
//! The installed-memory read, which sizes the swap partition (GEN-28).
//!
//! A read under way and a read that failed are different states. The
//! failure stays, says why, and keeps swap off, rather than reading as
//! "still reading" for ever, as a missing value once did. The states are
//! this module's: the rest of the window asks what it needs (the swap
//! partition's size, or why there is none; the pages' lines), and this
//! module alone decides what each state means.
// Every stage and answer is matched by name: a new one forces a decision.
#![deny(clippy::wildcard_enum_match_arm)]
use crate::format::size;
use aizome_typestate::{Answer, Fresh, Generation, Ticket};
use calamares_nixos::memory::{self, MemInfo};

/// Names the memory read: its answer carries the ticket.
pub enum MemoryRead {}

/// The read's answer: what /proc/meminfo says, or why it couldn't be read.
#[derive(Clone, Debug)]
pub struct MemoryAnswer {
    ticket: Ticket<MemoryRead>,
    result: Result<MemInfo, String>,
}

impl MemoryAnswer {
    pub fn new(ticket: Ticket<MemoryRead>, result: Result<MemInfo, String>) -> Self {
        Self { ticket, result }
    }
}

impl Answer<MemoryRead> for MemoryAnswer {
    fn ticket(&self) -> Ticket<MemoryRead> {
        self.ticket
    }
}

/// The swap partition, sized to installed memory, as far as that's known.
/// It has no shortcut that answers "is it known?": callers match all four,
/// so a read under way and a failed one can't fall together (GEN-28):
///
/// ```compile_fail,E0599
/// use genkan::ram::Memory;
/// fn still_reading(memory: &Memory) -> bool {
///     memory.swap_size(true).is_err()
/// }
/// ```
///
/// ```
/// use genkan::ram::{Memory, SwapSize};
/// fn still_reading(memory: &Memory) -> bool {
///     match memory.swap_size(true) {
///         SwapSize::Reading => true,
///         SwapSize::Off | SwapSize::Known(_) | SwapSize::Unreadable(_) => false,
///     }
/// }
/// ```
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SwapSize {
    /// Swap is off: there's no partition to size.
    Off,
    /// Its size, matching installed memory.
    Known(u64),
    /// The read hasn't answered yet.
    Reading,
    /// It failed, and why: it isn't read again, so swap stays off.
    Unreadable(String),
}

aizome_typestate::machine! {
    /// The installed-memory read. Its state is private: code outside this
    /// module can't match it, as the check that took a failed read for one
    /// under way once matched `None` (GEN-28). It gets a [`SwapSize`], with no
    /// shortcut past its states; a `_` arm in a match on it is the one way
    /// left to collapse them, which no type stops:
    ///
    /// ```compile_fail,E0603,E0624
    /// use genkan::ram::{Memory, MemoryState};
    /// fn still_reading(memory: &Memory) -> bool {
    ///     !matches!(memory.stored(), MemoryState::Known(_))
    /// }
    /// ```
    ///
    /// ```
    /// use genkan::ram::{Memory, SwapSize};
    /// fn still_reading(memory: &Memory) -> bool {
    ///     memory.swap_size(true) == SwapSize::Reading
    /// }
    /// ```
    pub struct Memory over Generation<MemoryRead> {
        /// What's known about installed memory.
        #[derive(Clone, Debug, PartialEq)]
        stored MemoryState;
        /// The typed state, holding the read's tickets.
        typed MemoryAny;
        states {
            /// Not asked yet: before the window opens.
            Unasked,
            /// The read is under way.
            Reading,
            /// What /proc/meminfo says.
            Known(MemInfo),
            /// It couldn't be read: why.
            Unreadable(String),
        }
        branches {
            Read = Known | Unreadable;
        }
        access(private);
        new(pub(crate));
    }
}

impl Memory {
    /// Starts the one read: its ticket, the first time.
    pub(crate) fn ask(&mut self) -> Option<Ticket<MemoryRead>> {
        self.with_typed(|state| match state {
            MemoryAny::Unasked(unasked) => {
                let (reading, ticket) = unasked.ask();
                (reading.into(), Some(ticket))
            }
            other @ (MemoryAny::Reading(_) | MemoryAny::Known(_) | MemoryAny::Unreadable(_)) => {
                (other, None)
            }
        })
    }

    /// Applies the read's answer, if it's the latest read's.
    pub(crate) fn answer(&mut self, answer: MemoryAnswer) {
        let Ok(fresh) = self.ctx().check_answer(answer) else {
            return;
        };
        self.with_typed(|state| match state {
            MemoryAny::Reading(reading) => (reading.answered(fresh).into(), ()),
            other @ (MemoryAny::Unasked(_) | MemoryAny::Known(_) | MemoryAny::Unreadable(_)) => {
                (other, ())
            }
        });
    }

    /// The swap partition when swap is `on`: its size, or why it can't be
    /// sized yet, or at all.
    pub fn swap_size(&self, on: bool) -> SwapSize {
        if !on {
            return SwapSize::Off;
        }
        match self.stored() {
            MemoryState::Known(info) => SwapSize::Known(memory::swap_bytes(info.total)),
            MemoryState::Unasked(()) | MemoryState::Reading(()) => SwapSize::Reading,
            MemoryState::Unreadable(error) => SwapSize::Unreadable(error.clone()),
        }
    }

    /// The Disk page's line about the swap partition.
    pub fn swap_note(&self) -> String {
        const ZSWAP: &str = "zswap compresses pages in RAM before they reach the disk, and hibernation can resume from it.";
        match self.stored() {
            MemoryState::Known(info) => format!(
                "A {} swap partition matching installed memory. {ZSWAP}",
                size(memory::swap_bytes(info.total))
            ),
            MemoryState::Unasked(()) | MemoryState::Reading(()) => {
                format!("Matches installed memory. {ZSWAP}")
            }
            MemoryState::Unreadable(error) => format!(
                "Installed memory could not be read ({error}), so a swap partition can't be sized and stays off."
            ),
        }
    }

    /// The sidebar's line about this computer's memory.
    pub fn summary(&self) -> String {
        match self.stored() {
            MemoryState::Known(info) => format!("{} of memory", size(info.total)),
            MemoryState::Unasked(()) | MemoryState::Reading(()) => {
                "Reading installed memory…".into()
            }
            MemoryState::Unreadable(_) => "Installed memory unknown".into(),
        }
    }
}

impl<'s> Unasked<'s> {
    /// Starts the read: its answer carries the ticket.
    fn ask(self) -> (Reading<'s>, Ticket<MemoryRead>) {
        let ticket = self.ctx.issue();
        (Reading::enter(self.ctx, ()), ticket)
    }
}

impl<'s> Reading<'s> {
    /// The read answered, bound to the proof that it's the latest read's:
    /// what it found, or why it found nothing.
    fn answered(self, answer: Fresh<MemoryRead, MemoryAnswer>) -> Read<'s> {
        match answer.into_answer().result {
            Ok(info) => Read::Known(Known::enter(self.ctx, info)),
            Err(error) => Read::Unreadable(Unreadable::enter(self.ctx, error)),
        }
    }
}

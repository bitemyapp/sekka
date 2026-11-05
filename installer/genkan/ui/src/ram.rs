// SPDX-License-Identifier: GPL-3.0-or-later
//! The installed-memory read, which sizes the swap partition (GEN-28).
//!
//! A read under way and a read that failed are different states. The
//! failure stays, says why, and keeps swap off, rather than reading as
//! "still reading" for ever, as a missing value once did. The states are
//! this module's: the rest of the window asks what it needs (the swap
//! partition's size, or why there is none; the pages' lines), and this
//! module alone decides what each state means.
use crate::format::size;
use aizome_typestate::{Answer, Fresh, Generation, Ticket};
use calamares_nixos::memory::{self, MemInfo};
use std::fmt;

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

/// Why the swap partition can't be sized.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SwapUnsized {
    /// The read hasn't answered yet.
    Reading,
    /// It failed, and why: it isn't read again, so swap stays off.
    Unreadable(String),
}

impl fmt::Display for SwapUnsized {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Reading => f.write_str("Still reading installed memory; try again in a moment."),
            Self::Unreadable(error) => write!(
                f,
                "Installed memory could not be read ({error}), so a swap partition can't be sized. Turn swap off to continue."
            ),
        }
    }
}

aizome_typestate::machine! {
    /// The installed-memory read. Its state is private: code outside this
    /// module can't match it, as the check that took a failed read for one
    /// under way once matched `None` (GEN-28). It gets [`SwapUnsized`], whose
    /// two reasons have their own words here; it could still collapse them
    /// into words of its own (`swap_size(..).is_err()`), which no type stops:
    ///
    /// ```compile_fail,E0603,E0624
    /// use genkan::ram::{Memory, MemoryState};
    /// fn still_reading(memory: &Memory) -> bool {
    ///     !matches!(memory.stored(), MemoryState::Known(_))
    /// }
    /// ```
    ///
    /// ```
    /// use genkan::ram::{Memory, SwapUnsized};
    /// fn still_reading(memory: &Memory) -> bool {
    ///     memory.swap_size(true) == Err(SwapUnsized::Reading)
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
            other => (other, None),
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

    /// The swap partition's size, matching installed memory, when swap is
    /// `on`; why there can't be one yet, or at all.
    pub fn swap_size(&self, on: bool) -> Result<Option<u64>, SwapUnsized> {
        if !on {
            return Ok(None);
        }
        match self.stored() {
            MemoryState::Known(info) => Ok(Some(memory::swap_bytes(info.total))),
            MemoryState::Unasked(()) | MemoryState::Reading(()) => Err(SwapUnsized::Reading),
            MemoryState::Unreadable(error) => Err(SwapUnsized::Unreadable(error.clone())),
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

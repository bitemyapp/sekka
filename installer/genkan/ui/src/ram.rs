// SPDX-License-Identifier: GPL-3.0-or-later
//! The installed-memory read, which sizes the swap partition (GEN-28).
//!
//! A read under way and a read that failed are different states. The
//! failure stays, says why, and keeps swap off, rather than reading as
//! "still reading" for ever, as a missing value once did.
use aizome_typestate::{Current, Generation, Ticket};
use calamares_nixos::memory::MemInfo;

/// Names the memory read: its answer carries the ticket.
pub enum MemoryRead {}

aizome_typestate::machine! {
    /// The installed-memory read.
    pub struct Memory over Generation<MemoryRead> {
        /// What's known about installed memory. Code that decides from it
        /// must say what each state means, so a failed read can't pass for
        /// one under way (GEN-28):
        ///
        /// ```compile_fail,E0004
        /// use genkan::ram::MemoryState;
        /// fn swap_note(memory: &MemoryState) -> &'static str {
        ///     match memory {
        ///         MemoryState::Known(_) => "",
        ///         MemoryState::Unasked(()) | MemoryState::Reading(()) => "Reading installed memory…",
        ///     }
        /// }
        /// ```
        ///
        /// ```
        /// use genkan::ram::MemoryState;
        /// fn swap_note(memory: &MemoryState) -> &'static str {
        ///     match memory {
        ///         MemoryState::Known(_) => "",
        ///         MemoryState::Unasked(()) | MemoryState::Reading(()) => "Reading installed memory…",
        ///         MemoryState::Unreadable(_) => "Could not read installed memory",
        ///     }
        /// }
        /// ```
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
        new(pub(crate));
        with_typed(pub(crate));
    }
}

impl<'s> Unasked<'s> {
    /// Starts the read: its answer carries the ticket.
    pub fn ask(self) -> (Reading<'s>, Ticket<MemoryRead>) {
        let ticket = self.ctx.issue();
        (Reading::enter(self.ctx, ()), ticket)
    }
}

impl<'s> Reading<'s> {
    /// The read answered: what it found, or why it found nothing.
    pub fn answered(self, _: Current<MemoryRead>, answer: Result<MemInfo, String>) -> Read<'s> {
        match answer {
            Ok(info) => Read::Known(Known::enter(self.ctx, info)),
            Err(error) => Read::Unreadable(Unreadable::enter(self.ctx, error)),
        }
    }
}

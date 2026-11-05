//! Typestate support for Sekka's state machines.
//!
//! Sekka writes its state machines as hand-written typestate: one type per
//! state, and transitions that consume the state they leave and return the
//! state they enter, so an illegal transition does not compile. The design
//! rules, and the measurements behind them, are in the state-machine crate
//! evaluation (`docs/state-machine-crates.md`):
//!
//! 1. State types hold only their own state's data. Shared context lives in
//!    the machine, and a state borrows it, so a state is a pointer plus its
//!    data and moving it is cheap ([`machine!`]).
//! 2. Instance checks are impossible to forget. A transition that acts on an
//!    answer to an earlier request takes a proof that the answer is current
//!    ([`Generation`], [`Fresh`]); a destructive step takes a proof that
//!    its target is the one the person confirmed ([`Reviewed`],
//!    [`Confirmed`]).
//! 3. Signatures are narrow: a transition takes what it needs to decide, not
//!    a value the caller picked for it.
//! 4. The hottest event is routed first.
//! 5. The typed transitions are the only way to change a state: fields stay
//!    private, and code outside the machine's module reaches the typed states
//!    only through `with_typed`. A step that must end in a commit or a restore
//!    is a consuming method, with [`Unsettled`] running the restore on the
//!    paths the types can't close (an early return, a panic).
//!
//! Everything here is `#[inline]` or generic, so a program compiles only the
//! parts it uses.
//!
//! # Showing that a mistake doesn't compile
//!
//! A machine shows each guarantee with a `compile_fail` doctest that names
//! the error it expects (` ```compile_fail,E0599 `), beside a doctest that
//! makes the legal call and compiles. Stable rustdoc doesn't check the error
//! code, so a snippet that fails for another reason (a typo, a missing
//! import) would still pass. Run the doctests as rustdoc does on nightly,
//! which checks the codes:
//!
//! ```sh
//! RUSTC_BOOTSTRAP=1 cargo test --doc -p <crate>
//! ```
//!
//! Errors without a code, such as "lifetime may not live long enough", rely
//! on the compiling twin to show the snippet is otherwise sound.

#![no_std]
#![forbid(unsafe_code)]

mod confirm;
mod generation;
mod machine;
mod settle;

pub use confirm::{Changed, Confirmed, Reviewed, SameTarget};
pub use generation::{Answer, Current, Fresh, Generation, Ticket};
pub use settle::Unsettled;

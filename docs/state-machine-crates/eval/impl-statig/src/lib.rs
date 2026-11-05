//! The six targets with statig 0.4.1 (`#[state_machine]` on an impl block: hierarchical states with state-local storage).
//! One module per target; each exports its machine type (`pointer::Pointer`,
//! `wrap::Wrap`, `polkit::Polkit`, `transfer::Transfer`, `lock::Lock`,
//! `install::Install`) and `IMPLEMENTED`. See `IMPLEMENTING.md`.

pub mod install;
pub mod lock;
pub mod pointer;
pub mod polkit;
pub mod transfer;
pub mod wrap;

use spec::Sink;

/// statig's external context, where a machine's effects go. statig fixes the
/// context type per machine (`IntoStateMachine::Context<'ctx>`), so the
/// generic `out` of `Machine::handle` comes in as a `dyn Sink`: one dynamic
/// call per effect.
pub struct Out<'a, E>(pub &'a mut dyn Sink<E>);

impl<E> Out<'_, E> {
    #[inline]
    pub fn push(&mut self, effect: E) {
        self.0.push(effect);
    }
}

/// A sink for `init_with_context` of machines whose initial state has no
/// entry action (nothing reaches it).
pub(crate) struct Nowhere;

impl<E> Sink<E> for Nowhere {
    fn push(&mut self, _: E) {
        unreachable!("an initial state with an entry action")
    }
}

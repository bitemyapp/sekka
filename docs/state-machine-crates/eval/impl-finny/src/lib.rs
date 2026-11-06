//! The six targets with finny 0.3.0 (`#[finny_fsm]` builder function: sub-machines, timers, queue, regions).
//! One module per target; each exports its machine type (`pointer::Pointer`,
//! `wrap::Wrap`, `polkit::Polkit`, `transfer::Transfer`, `lock::Lock`,
//! `install::Install`) and `IMPLEMENTED`. See `IMPLEMENTING.md`.
//!
//! finny's context is one concrete type per machine, owned by the machine
//! (and a sub-machine's is a separate value, built once from its parent's),
//! so it can't borrow the caller's `out` for one `handle`. Every target
//! collects its effects in an [`Effects`] buffer inside a context and drains
//! it into `out` when `dispatch` returns. The machines run on finny's
//! fixed-size [`finny::FsmEventQueueArray`] ([`Front`]; [`Unqueued`] for the
//! two that never queue an event), no inspector and no timers service.
//!
//! Each wrapper keeps its finny frontend private and offers a typed
//! `dispatch` that forwards to finny's (and drains the effects): finny makes
//! the frontend's and backend's fields public, so whoever holds the frontend
//! `&mut` can set any state, its data and the context. Every context type
//! has private fields and no public constructor, so no machine can be built
//! from outside either.

pub mod install;
pub mod lock;
pub mod pointer;
pub mod polkit;
pub mod transfer;
pub mod wrap;

/// For the compile-fail cases, which may only use this crate, `spec` and `std`
/// (they name `finny::FsmError`).
pub use finny;

use finny::inspect::null::InspectNull;
use finny::{FsmBackend, FsmEventQueueArray, FsmEventQueueNull, FsmFrontend, FsmTimersNull};
use spec::Sink;

/// A machine with finny's fixed-capacity queue of `N` events (no allocation
/// per event, unlike the default `VecDeque`), no inspection and no timers:
/// the targets' timers are effects and their firing is an event.
pub type Front<F, const N: usize> =
    FsmFrontend<F, FsmEventQueueArray<F, [<F as FsmBackend>::Events; N]>, InspectNull, FsmTimersNull>;

/// A machine with no queue, for those that never queue an event. finny's
/// null queue accepts an `enqueue` and drops the event, so a re-queueing
/// action added later would be lost silently: switch to [`Front`] then.
pub type Unqueued<F> = FsmFrontend<F, FsmEventQueueNull<F>, InspectNull, FsmTimersNull>;

/// A fixed-capacity effect buffer: the actions push, `handle` drains.
/// Pushing past `N` panics (each target sizes `N` for its longest step).
pub struct Effects<E: Copy, const N: usize> {
    buf: [E; N],
    len: usize,
}

impl<E: Copy, const N: usize> Effects<E, N> {
    /// `fill` only initializes the unused slots.
    pub const fn new(fill: E) -> Self {
        Self { buf: [fill; N], len: 0 }
    }

    #[inline]
    pub fn push(&mut self, e: E) {
        self.buf[self.len] = e;
        self.len += 1;
    }

    #[inline]
    pub fn drain(&mut self, out: &mut impl Sink<E>) {
        for e in &self.buf[..self.len] {
            out.push(*e);
        }
        self.len = 0;
    }
}
